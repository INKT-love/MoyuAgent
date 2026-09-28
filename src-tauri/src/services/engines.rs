use super::config;
use super::opencode::{Engine, EngineHealth};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

/// One OpenCode process per workspace. The map is separate from prompt/stream
/// internals so a later host can replace just this pool.
pub struct EnginePool {
    engines: Mutex<HashMap<String, Arc<Engine>>>,
}

impl Default for EnginePool {
    fn default() -> Self {
        Self::new()
    }
}

impl EnginePool {
    pub fn new() -> Self {
        Self {
            engines: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_or_create(
        &self,
        executable: PathBuf,
        workspace: PathBuf,
    ) -> Result<Arc<Engine>, String> {
        let workspace = config::display_path(workspace);
        let key = config::workspace_key(&workspace.to_string_lossy());
        if key.is_empty() {
            return Err("Choose a workspace before starting the agent".into());
        }
        let mut engines = self.engines.lock().await;
        if let Some(engine) = engines.get(&key) {
            return Ok(engine.clone());
        }
        let engine = Arc::new(Engine::new(executable, workspace));
        engines.insert(key, engine.clone());
        Ok(engine)
    }

    pub async fn acknowledge(&self, request_id: &str, sequence: u64) -> Result<(), String> {
        let engines = self.snapshot().await;
        for engine in engines {
            engine.acknowledge(request_id, sequence).await?;
        }
        Ok(())
    }

    pub async fn cancel(&self, request_id: &str) -> Result<(), String> {
        let engines = self.snapshot().await;
        for engine in engines {
            engine.cancel(request_id).await?;
        }
        Ok(())
    }

    pub async fn health(&self, executable: &Path) -> EngineHealth {
        let engines = self.snapshot().await;
        let mut active_requests = 0usize;
        let mut executable_available = tokio::fs::metadata(executable).await.is_ok();
        for engine in engines {
            let health = engine.health().await;
            active_requests = active_requests.saturating_add(health.active_requests);
            executable_available = executable_available || health.executable_available;
        }
        EngineHealth {
            executable_available,
            active_requests,
        }
    }

    pub async fn active_requests(&self) -> usize {
        let mut total = 0usize;
        for engine in self.snapshot().await {
            total = total.saturating_add(engine.health().await.active_requests);
        }
        total
    }

    pub async fn shutdown_workspace(&self, workspace: &Path) {
        let key = config::workspace_key(&workspace.to_string_lossy());
        let engine = self.engines.lock().await.remove(&key);
        if let Some(engine) = engine {
            engine.shutdown().await;
        }
    }

    pub async fn shutdown_idle(&self) {
        let snapshot = self.snapshot_with_keys().await;
        let mut idle_keys = Vec::new();
        for (key, engine) in &snapshot {
            if engine.health().await.active_requests == 0 {
                idle_keys.push(key.clone());
            }
        }
        let mut stopping = Vec::new();
        {
            let mut engines = self.engines.lock().await;
            for key in idle_keys {
                if let Some(engine) = engines.remove(&key) {
                    if engine.health().await.active_requests == 0 {
                        stopping.push(engine);
                    } else {
                        engines.insert(key, engine);
                    }
                }
            }
        }
        for engine in stopping {
            engine.shutdown().await;
        }
    }

    pub async fn shutdown_all(&self) {
        let engines: Vec<Arc<Engine>> = {
            let mut slot = self.engines.lock().await;
            slot.drain().map(|(_, engine)| engine).collect()
        };
        for engine in engines {
            engine.shutdown().await;
        }
    }

    async fn snapshot(&self) -> Vec<Arc<Engine>> {
        self.engines.lock().await.values().cloned().collect()
    }

    async fn snapshot_with_keys(&self) -> Vec<(String, Arc<Engine>)> {
        self.engines
            .lock()
            .await
            .iter()
            .map(|(key, engine)| (key.clone(), engine.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn reuses_normalized_workspace_paths_and_isolates_others() {
        let pool = EnginePool::new();
        let exe = PathBuf::from("opencode");
        let first = pool
            .get_or_create(exe.clone(), PathBuf::from(r"D:\alpha"))
            .await
            .unwrap();
        let again = pool
            .get_or_create(exe.clone(), PathBuf::from("D:/alpha/"))
            .await
            .unwrap();
        let other = pool
            .get_or_create(exe, PathBuf::from(r"D:\beta"))
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &other));
        assert_eq!(first.workspace(), Path::new(r"D:\alpha"));
        assert_eq!(other.workspace(), Path::new(r"D:\beta"));
        assert_eq!(pool.active_requests().await, 0);
    }
}
