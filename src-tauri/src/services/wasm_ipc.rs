use super::harness;
use extism::{Manifest, Plugin, PluginBuilder, Wasm, convert::Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use tauri::ipc::{Invoke, InvokeBody};
use tauri::{AppHandle, Runtime};

const MAX_BACKEND_BYTES: usize = 8 * 1024 * 1024;
const HOOK_EXPORT: &str = "before";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HookInput {
    command: String,
    args: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct HookOutput {
    #[serde(default)]
    cancel: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    result: Option<Value>,
}

#[derive(Debug)]
enum Before {
    Continue,
    Cancel(String),
    Replace(Value),
}

fn parse_before(output: HookOutput) -> Before {
    if output.cancel {
        Before::Cancel(output.error.unwrap_or_else(|| "cancelled".into()))
    } else if let Some(result) = output.result {
        Before::Replace(result)
    } else {
        Before::Continue
    }
}

struct WasmEngine {
    plugins: Vec<(String, Plugin)>,
}

impl WasmEngine {
    fn empty() -> Self {
        Self { plugins: Vec::new() }
    }

    fn load(&mut self, id: String, bytes: Vec<u8>) -> Result<(), String> {
        let manifest = Manifest::new([Wasm::data(bytes)]);
        let plugin = PluginBuilder::new(manifest)
            .with_wasi(false)
            .build()
            .map_err(|error| format!("{id} backend.wasm: {error}"))?;
        self.plugins.push((id, plugin));
        Ok(())
    }

    fn before(&mut self, command: &str, args: &Value) -> Before {
        let input = HookInput {
            command: command.to_owned(),
            args: args.clone(),
        };
        for (id, plugin) in &mut self.plugins {
            if !plugin.function_exists(HOOK_EXPORT) {
                continue;
            }
            let output = match plugin.call::<_, Json<HookOutput>>(HOOK_EXPORT, Json(&input)) {
                Ok(Json(output)) => output,
                Err(error) => {
                    tracing::warn!(plugin = %id, %error, "backend.wasm before failed");
                    continue;
                }
            };
            match parse_before(output) {
                Before::Continue => {}
                other => return other,
            }
        }
        Before::Continue
    }
}

static ENGINE: Mutex<WasmEngine> = Mutex::new(WasmEngine {
    plugins: Vec::new(),
});

pub async fn reload(app: &AppHandle, enabled: &[String]) -> Result<(), String> {
    let mut engine = WasmEngine::empty();
    let root = harness::plugins_dir(app)?;
    for id in enabled {
        if !harness::valid_plugin_id(id) {
            continue;
        }
        let path = root.join(id).join("backend.wasm");
        if !path.is_file() {
            continue;
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_BACKEND_BYTES {
            return Err(format!("{id} backend.wasm exceeded the size limit"));
        }
        engine.load(id.clone(), bytes)?;
    }
    *ENGINE.lock().map_err(|error| error.to_string())? = engine;
    Ok(())
}

pub fn intercept<R: Runtime>(
    invoke: Invoke<R>,
    inner: impl Fn(Invoke<R>) -> bool,
) -> bool {
    let command = invoke.message.command().to_string();
    let args = match invoke.message.payload() {
        InvokeBody::Json(value) => value.clone(),
        InvokeBody::Raw(_) => return inner(invoke),
    };
    let decision = match ENGINE.lock() {
        Ok(mut engine) => engine.before(&command, &args),
        Err(_) => Before::Continue,
    };
    match decision {
        Before::Continue => inner(invoke),
        Before::Cancel(error) => {
            invoke.resolver.reject(error);
            true
        }
        Before::Replace(value) => {
            invoke.resolver.respond(Ok(value));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_before, Before, HookOutput, WasmEngine};
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn wasm_hook_output_continue_is_default() {
        assert!(matches!(parse_before(HookOutput::default()), Before::Continue));
    }

    #[test]
    fn wasm_hook_output_can_cancel_any_command() {
        let decision = parse_before(HookOutput {
            cancel: true,
            error: Some("blocked".into()),
            result: None,
        });
        assert!(matches!(decision, Before::Cancel(error) if error == "blocked"));
    }

    #[test]
    fn wasm_hook_output_can_replace_result() {
        let decision = parse_before(HookOutput {
            cancel: false,
            error: None,
            result: Some(json!([{ "id": 1, "name": "wasm" }])),
        });
        match decision {
            Before::Replace(value) => assert_eq!(value[0]["name"], "wasm"),
            other => panic!("expected replace, got {other:?}"),
        }
    }

    #[test]
    fn bundled_probe_wasm_hooks_engine_status() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/harness/probe/backend.wasm");
        let bytes = std::fs::read(&path).expect("probe backend.wasm must be built");
        let mut engine = WasmEngine::empty();
        engine.load("probe".into(), bytes).unwrap();
        match engine.before("get_engine_status", &json!({})) {
            Before::Replace(value) => assert_eq!(value["probe"], "wasm-ok"),
            other => panic!("expected replace, got {other:?}"),
        }
        assert!(matches!(
            engine.before("login", &json!({ "email": "a@b.c" })),
            Before::Continue
        ));
    }
}
