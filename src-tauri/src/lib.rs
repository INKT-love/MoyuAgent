mod services;

use serde::Serialize;
use services::{
    api_client::{self, ApiClient, Endpoint},
    auth::{self, AuthService, Group, GroupModel, LoginResult, User},
    config::{self, ConfigSummary, Preferences, WorkspaceActionResult},
    engines::EnginePool,
    harness::{self, PluginInfo, PluginMixin, PluginUi},
    history::{self, HistorySnapshot},
    opencode::{Engine, EngineHealth, RunEnvironment, RunRequest},
    plugin_settings::{self, PluginSettingsView},
    skills::{self, SkillInfo},
    store::{self, CatalogItem, Inspection, Kind},
    store_client,
    stream::StreamEvent,
    wasm_ipc,
};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tokio::sync::{Mutex, OnceCell};

#[derive(Default)]
struct AppState {
    services: OnceCell<Services>,
}

struct Services {
    api: Arc<ApiClient>,
    auth: AuthService,
    preferences: Mutex<Preferences>,
    user: Mutex<Option<User>>,
    engines: EnginePool,
    mutation: Mutex<()>,
    /// Downloaded and verified packages waiting for the user's confirmation, by token.
    staged: Mutex<HashMap<String, StagedPackage>>,
}

struct StagedPackage {
    directory: std::path::PathBuf,
    kind: Kind,
    id: String,
}

impl AppState {
    async fn services(&self, app: &AppHandle) -> Result<&Services, String> {
        self.services
            .get_or_try_init(|| async {
                let preferences = config::load(app).await?;
                harness::seed_bundled(app).await?;
                // Packages downloaded but never confirmed (crash, closed dialog) are leftovers.
                if let Ok(root) = config::app_directory(app) {
                    let _ = tokio::fs::remove_dir_all(root.join("store-staging")).await;
                }
                wasm_ipc::reload(app, &preferences.enabled_plugins).await?;
                let api = Arc::new(ApiClient::new(preferences.endpoint_index.min(1))?);
                Ok(Services {
                    auth: AuthService::new(api.clone()),
                    api,
                    preferences: Mutex::new(preferences),
                    user: Mutex::new(None),
                    engines: EnginePool::new(),
                    mutation: Mutex::new(()),
                    staged: Mutex::new(HashMap::new()),
                })
            })
            .await
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    endpoint: Endpoint,
    endpoints: Vec<Endpoint>,
    authenticated: bool,
    user: Option<User>,
    config: Option<ConfigSummary>,
    recent_workspaces: Vec<String>,
    pinned_workspaces: Vec<String>,
    workspace_labels: HashMap<String, String>,
}

impl Services {
    async fn snapshot(&self) -> Result<Snapshot, String> {
        let user = self.user.lock().await.clone();
        let prefs = self.preferences.lock().await;
        let config = prefs
            .config
            .clone()
            .filter(|c| user.as_ref().is_some_and(|u| u.id == c.user_id));
        Ok(Snapshot {
            endpoint: self.api.endpoint(),
            endpoints: api_client::endpoints(),
            authenticated: user.is_some(),
            user,
            config,
            recent_workspaces: prefs.recent_workspaces.clone(),
            pinned_workspaces: prefs.pinned_workspaces.clone(),
            workspace_labels: prefs.workspace_labels.clone(),
        })
    }

    async fn ensure_idle(&self) -> Result<(), String> {
        if self.engines.active_requests().await > 0 {
            return Err("Stop the current response before changing configuration".into());
        }
        Ok(())
    }

    async fn plugin_runtime(
        &self,
        app: &AppHandle,
        user_id: i64,
    ) -> Result<
        (
            Vec<String>,
            HashMap<String, serde_json::Value>,
            Vec<(String, String)>,
            bool,
        ),
        String,
    > {
        let prefs = self.preferences.lock().await;
        let (enabled, settings, names) = plugin_settings::collect(&prefs, user_id);
        drop(prefs);
        let secrets = plugin_settings::load_secrets(user_id, &enabled, &names).await;
        let extra_env = plugin_settings::extra_env(
            &enabled,
            &settings,
            &secrets,
            &plugin_settings::state_dir(app)?,
        );
        let has_spm_key = secrets.contains_key("spm.apiKey");
        Ok((enabled, settings, extra_env, has_spm_key))
    }

    async fn write_agent_config(
        &self,
        app: &AppHandle,
        summary: &ConfigSummary,
        base_url: &str,
    ) -> Result<(), String> {
        let (enabled, settings, _, has_spm_key) = self.plugin_runtime(app, summary.user_id).await?;
        let (available, disabled_skills): (Vec<String>, Vec<String>) = {
            let prefs = self.preferences.lock().await;
            (
                prefs
                    .models_for(summary.user_id, summary.group_id)
                    .map(|models| models.iter().map(|model| model.id.clone()).collect())
                    .unwrap_or_default(),
                prefs.disabled_skills.clone(),
            )
        };
        config::write_opencode_config(
            app,
            summary,
            base_url,
            &enabled,
            &settings,
            has_spm_key,
            &available,
            &disabled_skills,
        )
        .await?;
        Ok(())
    }

    fn run_environment(
        summary: &ConfigSummary,
        api_key: String,
        extra_env: Vec<(String, String)>,
    ) -> RunEnvironment {
        RunEnvironment {
            config_path: summary.config_path.clone().into(),
            api_key,
            model: Some(format!("sub2api/{}", summary.model)),
            reasoning_effort: Some(config::normalize_reasoning_effort(Some(
                &summary.reasoning_effort,
            ))),
            extra_env,
        }
    }

    async fn engine(&self, app: &AppHandle, config: &ConfigSummary) -> Result<Arc<Engine>, String> {
        self.engines
            .get_or_create(
                config::sidecar_path(app)?,
                config::display_path(std::path::PathBuf::from(&config.working_directory)),
            )
            .await
    }

    async fn drop_engine(&self) {
        self.engines.shutdown_all().await;
    }

    async fn boot_engine(&self, app: &AppHandle) {
        let Some(user) = self.user.lock().await.clone() else {
            return;
        };
        let Some(summary) = self
            .preferences
            .lock()
            .await
            .config
            .clone()
            .filter(|config| config.user_id == user.id)
        else {
            return;
        };
        let Ok(Some(key)) = tauri_plugin_secure_store::get_secret(&auth::api_key_secret_name(
            user.id,
            summary.group_id,
        ))
        .await
        else {
            return;
        };
        let Ok(engine) = self.engine(app, &summary).await else {
            return;
        };
        if engine.health().await.active_requests > 0 {
            return;
        }
        let extra_env = self
            .plugin_runtime(app, user.id)
            .await
            .ok()
            .map(|runtime| runtime.2)
            .unwrap_or_default();
        if let Err(error) = engine
            .ensure_server(&Self::run_environment(&summary, key, extra_env))
            .await
        {
            tracing::warn!(%error, "OpenCode server did not start with the application");
        }
    }

    async fn require_user(&self, action: &str) -> Result<User, String> {
        if let Some(user) = self.user.lock().await.clone() {
            return Ok(user);
        }
        let user = self
            .auth
            .current_user()
            .await?
            .ok_or_else(|| format!("请先登录后再{action}"))?;
        *self.user.lock().await = Some(user.clone());
        Ok(user)
    }

    async fn catalog_groups(&self, app: &AppHandle, refresh: bool) -> Result<Vec<Group>, String> {
        let user = self.require_user("查看分组").await?;
        if !refresh {
            let cached = self
                .preferences
                .lock()
                .await
                .groups_for(user.id)
                .map(|groups| groups.to_vec());
            if let Some(groups) = cached {
                return Ok(groups);
            }
        }
        let groups = self.auth.groups().await?;
        let mut prefs = self.preferences.lock().await;
        prefs.set_groups(user.id, groups.clone());
        config::save(app, prefs.clone()).await?;
        Ok(groups)
    }

    async fn catalog_models(
        &self,
        app: &AppHandle,
        group_id: i64,
        refresh: bool,
    ) -> Result<Vec<GroupModel>, String> {
        let user = self.require_user("查看模型").await?;
        if group_id <= 0 {
            return Err("The selected group is not available for this account".into());
        }
        if !refresh {
            let cached = self
                .preferences
                .lock()
                .await
                .models_for(user.id, group_id)
                .map(|models| models.to_vec());
            if let Some(models) = cached {
                return Ok(models);
            }
        }
        let groups = self.catalog_groups(app, false).await?;
        if !groups.iter().any(|group| group.id == group_id) {
            return Err("The selected group is not available for this account".into());
        }
        let models = self.auth.group_models(group_id, user.id).await?;
        let mut prefs = self.preferences.lock().await;
        prefs.set_models(user.id, group_id, models.clone());
        config::save(app, prefs.clone()).await?;
        Ok(models)
    }
}

fn spawn_boot(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Ok(services) = state.services(&app).await {
            services.boot_engine(&app).await;
        }
    });
}

#[tauri::command]
async fn get_app_state(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    if services.user.lock().await.is_none() {
        let user = match services.auth.current_user().await {
            Ok(user) => user,
            Err(error) if error == api_client::UNAUTHORIZED => {
                tauri_plugin_secure_store::delete_secret("auth-token").await?;
                tauri_plugin_secure_store::delete_secret("auth-refresh").await?;
                None
            }
            Err(error) => return Err(error),
        };
        *services.user.lock().await = user;
    }
    let snapshot = services.snapshot().await?;
    if snapshot.authenticated && snapshot.config.is_some() {
        spawn_boot(app.clone());
    }
    Ok(snapshot)
}

#[tauri::command]
async fn switch_endpoint(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<Endpoint, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let endpoint = services.api.switch_endpoint(Some(index))?;
    let mut prefs = services.preferences.lock().await;
    prefs.endpoint_index = index;
    config::save(&app, prefs.clone()).await?;
    let summary = prefs.config.clone();
    drop(prefs);
    if let Some(config) = summary {
        services
            .write_agent_config(&app, &config, &endpoint.base_url)
            .await?;
        services.engines.shutdown_idle().await;
        spawn_boot(app.clone());
    }
    Ok(endpoint)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicSettings {
    login_agreement_required: bool,
    login_agreement_url: Option<String>,
    registration_url: Option<String>,
    agreement_documents: Vec<AgreementDocument>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgreementDocument {
    title: String,
    content: String,
}

#[tauri::command]
async fn get_public_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PublicSettings, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let settings = services.auth.public_settings().await?;
    Ok(PublicSettings {
        login_agreement_required: settings["login_agreement_enabled"]
            .as_bool()
            .unwrap_or(false),
        login_agreement_url: None,
        registration_url: None,
        agreement_documents: settings["login_agreement_documents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|document| {
                Some(AgreementDocument {
                    title: document["title"].as_str()?.to_owned(),
                    content: document["content_md"].as_str()?.to_owned(),
                })
            })
            .collect(),
    })
}

#[tauri::command]
async fn login(
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
    password: String,
    accepted_agreement: bool,
) -> Result<LoginResult, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services.ensure_idle().await?;
    let settings = services.auth.public_settings().await?;
    if settings["login_agreement_enabled"].as_bool() == Some(true) && !accepted_agreement {
        return Err("Accept the account service agreement before signing in".into());
    }
    let result = services.auth.login(email, password, None, None).await?;
    *services.user.lock().await = result.user.clone();
    if result.user.is_some() {
        spawn_boot(app.clone());
    }
    Ok(result)
}

#[tauri::command]
async fn complete_two_factor(
    app: AppHandle,
    state: State<'_, AppState>,
    temp_token: String,
    totp_code: String,
) -> Result<LoginResult, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services.ensure_idle().await?;
    let result = services
        .auth
        .login(
            String::new(),
            String::new(),
            Some(totp_code),
            Some(temp_token),
        )
        .await?;
    *services.user.lock().await = result.user.clone();
    if result.user.is_some() {
        spawn_boot(app.clone());
    }
    Ok(result)
}

#[tauri::command]
async fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services.drop_engine().await;
    services.auth.logout().await?;
    *services.user.lock().await = None;
    Ok(())
}

#[tauri::command]
async fn get_groups(
    app: AppHandle,
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> Result<Vec<Group>, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services
        .catalog_groups(&app, refresh.unwrap_or(false))
        .await
}

#[tauri::command]
async fn get_group_models(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: i64,
    refresh: Option<bool>,
) -> Result<Vec<GroupModel>, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services
        .catalog_models(&app, group_id, refresh.unwrap_or(false))
        .await
}

#[tauri::command]
async fn configure(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: Option<i64>,
    model: Option<String>,
    workspace: Option<String>,
    reasoning_effort: Option<String>,
    permission_mode: Option<String>,
) -> Result<ConfigSummary, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let user = services.require_user("配置模型").await?;
    let mut groups = services.catalog_groups(&app, false).await?;
    if let Some(id) = group_id {
        if !groups.iter().any(|group| group.id == id) {
            groups = services.catalog_groups(&app, true).await?;
        }
    }
    let group = groups
        .iter()
        .find(|group| group_id.is_none_or(|id| id == group.id))
        .cloned()
        .ok_or("No supported API group is available for this account")?;
    let mut model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config::default_model_for(&group.platform));
    if model.is_empty() {
        model = services
            .catalog_models(&app, group.id, false)
            .await?
            .into_iter()
            .next()
            .map(|item| item.id)
            .ok_or_else(|| "Select a model".to_owned())?;
    }
    if model.len() > 160
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:/".contains(&b))
    {
        return Err("Invalid model identifier".into());
    }
    let directory = config::working_directory(&app, workspace).await?;
    let _api_key = zeroize::Zeroizing::new(services.auth.ensure_api_key(group.id, user.id).await?);
    let (previous, previous_effort, previous_permission) = {
        let prefs = services.preferences.lock().await;
        (
            prefs.config.clone(),
            prefs
                .config
                .as_ref()
                .map(|config| config.reasoning_effort.clone()),
            prefs
                .config
                .as_ref()
                .map(|config| config.permission_mode.clone()),
        )
    };
    let summary = ConfigSummary {
        group_id: group.id,
        group_name: group.name.clone(),
        platform: group.platform.clone(),
        model,
        config_path: config::app_directory(&app)?
            .join("opencode.json")
            .to_string_lossy()
            .into_owned(),
        working_directory: directory.to_string_lossy().into_owned(),
        user_id: user.id,
        reasoning_effort: config::normalize_reasoning_effort(
            reasoning_effort.as_deref().or(previous_effort.as_deref()),
        ),
        permission_mode: config::normalize_permission_mode(
            permission_mode
                .as_deref()
                .or(previous_permission.as_deref()),
        ),
    };
    services
        .write_agent_config(&app, &summary, services.api.get_current_base_url())
        .await?;
    let mut prefs = services.preferences.lock().await;
    prefs.config = Some(summary.clone());
    prefs.endpoint_index = services.api.endpoint().index;
    config::remember_workspace(&mut prefs, summary.working_directory.clone());
    config::save(&app, prefs.clone()).await?;
    drop(prefs);
    let workspace_only = previous.as_ref().is_some_and(|prev| {
        prev.group_id == summary.group_id
            && prev.model == summary.model
            && prev.reasoning_effort == summary.reasoning_effort
            && prev.permission_mode == summary.permission_mode
    });
    if !workspace_only {
        services.engines.shutdown_idle().await;
    }
    spawn_boot(app.clone());
    Ok(summary)
}

async fn pick_folder_path(
    app: &AppHandle,
    title: &str,
    start: Option<String>,
) -> Result<Option<std::path::PathBuf>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_title(title);
    if let Some(directory) = start.filter(|path| !path.trim().is_empty()) {
        dialog = dialog.set_directory(directory);
    }
    dialog.pick_folder(move |folder| {
        let _ = sender.send(folder.map(|file| file.into_path()));
    });
    match receiver
        .await
        .map_err(|_| "Folder picker was interrupted".to_owned())?
    {
        None => Ok(None),
        Some(Ok(path)) => Ok(Some(path)),
        Some(Err(error)) => Err(error.to_string()),
    }
}

#[tauri::command]
async fn choose_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<ConfigSummary>, String> {
    let services = state.services(&app).await?;
    let current = services
        .preferences
        .lock()
        .await
        .config
        .as_ref()
        .map(|config| config.working_directory.clone());
    let Some(path) = pick_folder_path(&app, "选择工作区", current).await? else {
        return Ok(None);
    };
    let workspace = path.to_string_lossy().into_owned();
    let (group_id, model) = {
        let prefs = services.preferences.lock().await;
        prefs
            .config
            .as_ref()
            .map(|config| (Some(config.group_id), Some(config.model.clone())))
            .unwrap_or((None, None))
    };
    Ok(Some(
        configure(app, state, group_id, model, Some(workspace), None, None).await?,
    ))
}

#[tauri::command]
async fn reveal_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    let requested = path.filter(|value| !value.trim().is_empty());
    let target = match requested {
        Some(value) => value,
        None => services
            .preferences
            .lock()
            .await
            .config
            .as_ref()
            .map(|config| config.working_directory.clone())
            .ok_or_else(|| "Choose a workspace before opening it".to_owned())?,
    };
    open_directory(&app, target).await
}

async fn open_directory(app: &AppHandle, path: String) -> Result<(), String> {
    let directory = config::working_directory(app, Some(path)).await?;
    let display = directory.to_string_lossy().into_owned();
    tokio::task::spawn_blocking(move || open::that(&display))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn workspace_action(
    app: AppHandle,
    state: State<'_, AppState>,
    action: String,
    path: String,
    name: Option<String>,
) -> Result<WorkspaceActionResult, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Choose a workspace".into());
    }
    if path.len() > 1024 {
        return Err("Invalid workspace path".into());
    }
    let path = path.to_owned();
    let services = state.services(&app).await?;
    match action.as_str() {
        "reveal" => {
            open_directory(&app, path).await?;
            let prefs = services.preferences.lock().await;
            Ok(config::workspace_action_result(&prefs, None))
        }
        "pin" | "unpin" | "rename" | "remove" | "create_worktree" => {
            let _guard = services.mutation.lock().await;
            if action == "remove" {
                services
                    .engines
                    .shutdown_workspace(std::path::Path::new(&path))
                    .await;
            }
            let mut switch_to = None;
            if action == "create_worktree" {
                let dest = config::create_git_worktree(std::path::Path::new(&path)).await?;
                switch_to = Some(dest.to_string_lossy().into_owned());
            }
            let mut prefs = services.preferences.lock().await;
            match action.as_str() {
                "pin" => config::pin_workspace(&mut prefs, &path, true),
                "unpin" => config::pin_workspace(&mut prefs, &path, false),
                "rename" => {
                    config::rename_workspace(&mut prefs, &path, name.as_deref().unwrap_or(""))
                }
                "remove" => {
                    if config::forget_workspace(&mut prefs, &path) {
                        switch_to = Some(config::next_workspace(&prefs).unwrap_or_default());
                    }
                }
                "create_worktree" => {
                    if let Some(dest) = switch_to.as_ref() {
                        config::remember_workspace(&mut prefs, dest.clone());
                    }
                }
                _ => {}
            }
            config::save(&app, prefs.clone()).await?;
            Ok(config::workspace_action_result(&prefs, switch_to))
        }
        _ => Err("Unknown workspace action".into()),
    }
}

#[tauri::command]
async fn list_plugins(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<PluginInfo>, String> {
    let services = state.services(&app).await?;
    let enabled = services.preferences.lock().await.enabled_plugins.clone();
    harness::list_plugins(&app, &enabled).await
}

#[tauri::command]
async fn install_plugin(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<PluginInfo>, String> {
    let services = state.services(&app).await?;
    let Some(source) = pick_folder_path(&app, "选择插件目录", None).await? else {
        return harness::list_plugins(&app, &services.preferences.lock().await.enabled_plugins)
            .await;
    };
    let id = harness::install_from_directory(&app, &source).await?;
    let mut prefs = services.preferences.lock().await;
    if !prefs.enabled_plugins.iter().any(|item| item == &id) {
        prefs.enabled_plugins.push(id);
    }
    let enabled = prefs.enabled_plugins.clone();
    let summary = prefs.config.clone();
    config::save(&app, prefs.clone()).await?;
    drop(prefs);
    if let Some(summary) = summary {
        services
            .write_agent_config(&app, &summary, services.api.get_current_base_url())
            .await?;
        services.engines.shutdown_idle().await;
        spawn_boot(app.clone());
    }
    wasm_ipc::reload(&app, &enabled).await?;
    harness::list_plugins(&app, &enabled).await
}

#[tauri::command]
async fn set_plugin_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<Vec<PluginInfo>, String> {
    if !harness::valid_plugin_id(&id) {
        return Err("Invalid plugin id".into());
    }
    let services = state.services(&app).await?;
    let mut prefs = services.preferences.lock().await;
    if enabled {
        if !prefs.enabled_plugins.iter().any(|item| item == &id) {
            prefs.enabled_plugins.push(id);
        }
    } else {
        prefs.enabled_plugins.retain(|item| item != &id);
    }
    let enabled_ids = prefs.enabled_plugins.clone();
    let summary = prefs.config.clone();
    config::save(&app, prefs.clone()).await?;
    drop(prefs);
    if let Some(summary) = summary {
        services
            .write_agent_config(&app, &summary, services.api.get_current_base_url())
            .await?;
        services.engines.shutdown_idle().await;
        spawn_boot(app.clone());
    }
    wasm_ipc::reload(&app, &enabled_ids).await?;
    harness::list_plugins(&app, &enabled_ids).await
}

#[tauri::command]
async fn uninstall_plugin(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<PluginInfo>, String> {
    if !harness::valid_plugin_id(&id) {
        return Err("Invalid plugin id".into());
    }
    let services = state.services(&app).await?;
    harness::uninstall(&app, &id).await?;
    let mut prefs = services.preferences.lock().await;
    let secret_map = prefs.plugin_secret_names.clone();
    prefs.enabled_plugins.retain(|item| item != &id);
    plugin_settings::remove_plugin(&mut prefs, &id);
    let enabled = prefs.enabled_plugins.clone();
    let summary = prefs.config.clone();
    config::save(&app, prefs.clone()).await?;
    drop(prefs);
    for (user_key, plugins) in secret_map {
        if let (Ok(user_id), Some(names)) = (user_key.parse::<i64>(), plugins.get(&id)) {
            plugin_settings::delete_secrets(user_id, &id, names).await;
        }
    }
    if let Ok(directory) = plugin_settings::state_dir(&app) {
        let _ = tokio::fs::remove_file(directory.join(format!("{id}.json"))).await;
    }
    if let Some(summary) = summary {
        services
            .write_agent_config(&app, &summary, services.api.get_current_base_url())
            .await?;
        services.engines.shutdown_idle().await;
        spawn_boot(app.clone());
    }
    wasm_ipc::reload(&app, &enabled).await?;
    harness::list_plugins(&app, &enabled).await
}

const DEFAULT_STORE_URL: &str = "https://store.moyu.app/index.json";
const MAX_STAGED: usize = 8;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreItemView {
    #[serde(flatten)]
    item: CatalogItem,
    installed: bool,
    installed_version: Option<String>,
    update_available: bool,
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreView {
    url: String,
    items: Vec<StoreItemView>,
    skills: Vec<SkillInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StorePreview {
    token: String,
    #[serde(flatten)]
    inspection: Inspection,
    replaces: Option<String>,
}

fn store_address(prefs: &Preferences) -> String {
    let value = prefs.store_url.trim();
    if value.is_empty() {
        DEFAULT_STORE_URL.to_owned()
    } else {
        value.to_owned()
    }
}

async fn store_staging_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let directory = config::app_directory(app)?.join("store-staging");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| error.to_string())?;
    Ok(directory)
}

async fn build_store_view(
    app: &AppHandle,
    services: &Services,
    url: String,
    items: Vec<CatalogItem>,
) -> Result<StoreView, String> {
    let (enabled_plugins, disabled_skills) = {
        let prefs = services.preferences.lock().await;
        (prefs.enabled_plugins.clone(), prefs.disabled_skills.clone())
    };
    let plugins = harness::list_plugins(app, &enabled_plugins).await?;
    let skills = skills::list(&skills::skills_dir(app)?, &disabled_skills).await;
    let items = items
        .into_iter()
        .map(|item| {
            let (installed_version, enabled) = match item.kind {
                Kind::Plugin => plugins
                    .iter()
                    .find(|plugin| plugin.id == item.id && !plugin.bundled)
                    .map(|plugin| (Some(plugin.version.clone()), plugin.enabled))
                    .unwrap_or((None, false)),
                // Skills carry no version of their own; the catalog version is recorded at install.
                Kind::Skill => skills
                    .iter()
                    .find(|skill| skill.name == item.id)
                    .map(|skill| (Some(String::new()), skill.enabled))
                    .unwrap_or((None, false)),
            };
            let update_available = installed_version.as_deref().is_some_and(|version| {
                !version.is_empty() && store::is_newer(version, &item.version)
            });
            StoreItemView {
                installed: installed_version.is_some(),
                installed_version: installed_version.filter(|version| !version.is_empty()),
                update_available,
                enabled,
                item,
            }
        })
        .collect();
    Ok(StoreView { url, items, skills })
}

#[tauri::command]
async fn store_catalog(app: AppHandle, state: State<'_, AppState>) -> Result<StoreView, String> {
    let services = state.services(&app).await?;
    let url = store_address(&*services.preferences.lock().await);
    let parsed = store::parse_catalog_url(&url)?;
    let items = store_client::fetch_catalog(&store_client::client()?, &parsed).await?;
    build_store_view(&app, services, url, items).await
}

#[tauri::command]
async fn store_set_url(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<String, String> {
    let trimmed = url.trim();
    if !trimmed.is_empty() {
        store::parse_catalog_url(trimmed)?;
    }
    let services = state.services(&app).await?;
    let mut prefs = services.preferences.lock().await;
    prefs.store_url = trimmed.to_owned();
    config::save(&app, prefs.clone()).await?;
    Ok(store_address(&prefs))
}

/// Step one: download, verify and inspect. Nothing is installed until `store_install`.
#[tauri::command]
async fn store_prepare(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: Kind,
    id: String,
) -> Result<StorePreview, String> {
    if !store::valid_id(kind, &id) {
        return Err("名称不合法".into());
    }
    let services = state.services(&app).await?;
    let url = store_address(&*services.preferences.lock().await);
    let client = store_client::client()?;
    let catalog = store::parse_catalog_url(&url)?;
    let items = store_client::fetch_catalog(&client, &catalog).await?;
    let item = items
        .into_iter()
        .find(|item| item.kind == kind && item.id == id)
        .ok_or("商店目录里没有这个条目")?;
    let staging = store_staging_dir(&app).await?;
    let staged = store_client::stage(&client, &item, &staging).await?;
    let replaces = match kind {
        Kind::Plugin => harness::plugins_dir(&app)?
            .join(&id)
            .exists()
            .then(|| id.clone()),
        Kind::Skill => skills::skills_dir(&app)?
            .join(&id)
            .exists()
            .then(|| id.clone()),
    };
    if kind == Kind::Plugin
        && harness::bundled_plugins_root(&app).is_some_and(|root| root.join(&id).exists())
    {
        let _ = tokio::fs::remove_dir_all(&staged.directory).await;
        return Err("这个名称属于内置插件，不能用商店里的同名包替换".into());
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut pending = services.staged.lock().await;
    while pending.len() >= MAX_STAGED {
        let Some(oldest) = pending.keys().next().cloned() else {
            break;
        };
        if let Some(old) = pending.remove(&oldest) {
            let _ = tokio::fs::remove_dir_all(&old.directory).await;
        }
    }
    pending.insert(
        token.clone(),
        StagedPackage {
            directory: staged.directory,
            kind,
            id,
        },
    );
    Ok(StorePreview {
        token,
        inspection: staged.inspection,
        replaces,
    })
}

#[tauri::command]
async fn store_cancel(
    state: State<'_, AppState>,
    app: AppHandle,
    token: String,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    if let Some(staged) = services.staged.lock().await.remove(&token) {
        let _ = tokio::fs::remove_dir_all(&staged.directory).await;
    }
    Ok(())
}

async fn apply_agent_changes(
    app: &AppHandle,
    services: &Services,
    enabled: &[String],
) -> Result<(), String> {
    let summary = services.preferences.lock().await.config.clone();
    if let Some(summary) = summary {
        services
            .write_agent_config(app, &summary, services.api.get_current_base_url())
            .await?;
        services.engines.shutdown_idle().await;
        spawn_boot(app.clone());
    }
    wasm_ipc::reload(app, enabled).await
}

/// Step two: move the confirmed package into place and refresh the agent configuration.
#[tauri::command]
async fn store_install(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<StoreView, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let staged = services
        .staged
        .lock()
        .await
        .remove(&token)
        .ok_or("安装确认已过期，请重新点击安装")?;
    let result = async {
        match staged.kind {
            Kind::Plugin => {
                let id = harness::install_from_directory(&app, &staged.directory).await?;
                let mut prefs = services.preferences.lock().await;
                if !prefs.enabled_plugins.contains(&id) {
                    prefs.enabled_plugins.push(id);
                }
                config::save(&app, prefs.clone()).await
            }
            Kind::Skill => {
                let destination = skills::skills_dir(&app)?.join(&staged.id);
                if let Some(parent) = destination.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                if destination.exists() {
                    tokio::fs::remove_dir_all(&destination)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                tokio::fs::rename(&staged.directory, &destination)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut prefs = services.preferences.lock().await;
                prefs.disabled_skills.retain(|name| name != &staged.id);
                config::save(&app, prefs.clone()).await
            }
        }
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&staged.directory).await;
    result?;
    let enabled = services.preferences.lock().await.enabled_plugins.clone();
    apply_agent_changes(&app, services, &enabled).await?;
    let url = store_address(&*services.preferences.lock().await);
    let items =
        store_client::fetch_catalog(&store_client::client()?, &store::parse_catalog_url(&url)?)
            .await
            .unwrap_or_default();
    build_store_view(&app, services, url, items).await
}

#[tauri::command]
async fn set_skill_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<Vec<SkillInfo>, String> {
    if !store::valid_skill_name(&name) {
        return Err("名称不合法".into());
    }
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let disabled = {
        let mut prefs = services.preferences.lock().await;
        prefs.disabled_skills.retain(|item| item != &name);
        if !enabled {
            prefs.disabled_skills.push(name);
        }
        config::save(&app, prefs.clone()).await?;
        prefs.disabled_skills.clone()
    };
    let plugins = services.preferences.lock().await.enabled_plugins.clone();
    apply_agent_changes(&app, services, &plugins).await?;
    Ok(skills::list(&skills::skills_dir(&app)?, &disabled).await)
}

#[tauri::command]
async fn uninstall_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<SkillInfo>, String> {
    if !store::valid_skill_name(&name) {
        return Err("名称不合法".into());
    }
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let directory = skills::skills_dir(&app)?.join(&name);
    if directory.exists() {
        tokio::fs::remove_dir_all(&directory)
            .await
            .map_err(|error| error.to_string())?;
    }
    let disabled = {
        let mut prefs = services.preferences.lock().await;
        prefs.disabled_skills.retain(|item| item != &name);
        config::save(&app, prefs.clone()).await?;
        prefs.disabled_skills.clone()
    };
    let plugins = services.preferences.lock().await.enabled_plugins.clone();
    apply_agent_changes(&app, services, &plugins).await?;
    Ok(skills::list(&skills::skills_dir(&app)?, &disabled).await)
}

#[tauri::command]
async fn list_skills(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<SkillInfo>, String> {
    let services = state.services(&app).await?;
    let disabled = services.preferences.lock().await.disabled_skills.clone();
    Ok(skills::list(&skills::skills_dir(&app)?, &disabled).await)
}

#[tauri::command]
async fn list_plugin_uis(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<PluginUi>, String> {
    let services = state.services(&app).await?;
    let enabled = services.preferences.lock().await.enabled_plugins.clone();
    harness::list_plugin_uis(&app, &enabled).await
}

#[tauri::command]
async fn list_plugin_mixins(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<PluginMixin>, String> {
    let services = state.services(&app).await?;
    let enabled = services.preferences.lock().await.enabled_plugins.clone();
    harness::list_plugin_mixins(&app, &enabled).await
}

async fn plugin_settings_view(
    app: &AppHandle,
    services: &Services,
    user_id: i64,
    plugin_id: &str,
) -> Result<PluginSettingsView, String> {
    let prefs = services.preferences.lock().await;
    let enabled = prefs.enabled_plugins.iter().any(|item| item == plugin_id);
    let values = plugin_settings::settings_for(&prefs, user_id, plugin_id);
    let names = plugin_settings::secret_names_for(&prefs, user_id, plugin_id);
    let config_path = prefs
        .config
        .as_ref()
        .filter(|config| config.user_id == user_id)
        .map(|config| config.config_path.clone());
    drop(prefs);
    let secrets = plugin_settings::secret_presence(user_id, plugin_id, &names).await;
    let has_secret = secrets.values().copied().any(|present| present);
    let config = match config_path {
        Some(path) => plugin_settings::read_json_file(std::path::Path::new(&path)).await,
        None => None,
    };
    let instructions = plugin_settings::read_text_file(&harness::instructions_path(app)?).await;
    let state = plugin_settings::read_json_file(
        &plugin_settings::state_dir(app)?.join(format!("{plugin_id}.json")),
    )
    .await;
    Ok(PluginSettingsView {
        id: plugin_id.to_owned(),
        values: values.clone(),
        secrets,
        applied: plugin_settings::inspect_applied(
            plugin_id,
            enabled,
            &values,
            has_secret,
            config.as_ref(),
            &instructions,
            state.as_ref(),
        ),
    })
}

#[tauri::command]
async fn get_plugin_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<PluginSettingsView, String> {
    if !harness::valid_plugin_id(&id) {
        return Err("Invalid plugin id".into());
    }
    let services = state.services(&app).await?;
    let user = services.require_user("查看插件设置").await?;
    let enabled = services.preferences.lock().await.enabled_plugins.clone();
    let plugins = harness::list_plugins(&app, &enabled).await?;
    if !plugins.iter().any(|plugin| plugin.id == id) {
        return Err("Plugin not found".into());
    }
    plugin_settings_view(&app, services, user.id, &id).await
}

#[tauri::command]
async fn set_plugin_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    values: serde_json::Value,
    secrets: Option<serde_json::Value>,
) -> Result<PluginSettingsView, String> {
    if !harness::valid_plugin_id(&id) {
        return Err("Invalid plugin id".into());
    }
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let user = services.require_user("保存插件设置").await?;
    let enabled = services.preferences.lock().await.enabled_plugins.clone();
    let plugins = harness::list_plugins(&app, &enabled).await?;
    if !plugins.iter().any(|plugin| plugin.id == id) {
        return Err("Plugin not found".into());
    }
    let values = plugin_settings::sanitize_values(values)?;
    let secret_updates = plugin_settings::sanitize_secrets(secrets)?;
    let mut prefs = services.preferences.lock().await;
    let mut names = plugin_settings::secret_names_for(&prefs, user.id, &id);
    for (key, value) in &secret_updates {
        names.retain(|item| item != key);
        if value.is_some() {
            names.push(key.clone());
        }
    }
    plugin_settings::write_secrets(user.id, &id, secret_updates).await?;
    plugin_settings::put_settings(&mut prefs, user.id, &id, values, names);
    let summary = prefs.config.clone();
    config::save(&app, prefs.clone()).await?;
    drop(prefs);
    if let Some(summary) = summary.filter(|config| config.user_id == user.id) {
        services
            .write_agent_config(&app, &summary, services.api.get_current_base_url())
            .await?;
        services.engines.shutdown_idle().await;
        spawn_boot(app.clone());
    }
    plugin_settings_view(&app, services, user.id, &id).await
}

#[tauri::command]
async fn start_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
    prompt: String,
    session_id: Option<String>,
    workspace: Option<String>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let user = services.require_user("开始任务").await?;
    let mut summary = services
        .preferences
        .lock()
        .await
        .config
        .clone()
        .filter(|c| c.user_id == user.id)
        .ok_or("Configure your API group before starting the agent")?;
    if let Some(path) = workspace.filter(|value| !value.trim().is_empty()) {
        summary.working_directory = config::working_directory(&app, Some(path))
            .await?
            .to_string_lossy()
            .into_owned();
    }
    let key = tauri_plugin_secure_store::get_secret(&auth::api_key_secret_name(
        user.id,
        summary.group_id,
    ))
    .await?
    .ok_or("The API key is unavailable. Run configuration again.")?;
    let endpoint = services.api.reachable_endpoint().await?;
    {
        let mut prefs = services.preferences.lock().await;
        if prefs.endpoint_index != endpoint.index {
            prefs.endpoint_index = endpoint.index;
            config::save(&app, prefs.clone()).await?;
        }
    }
    services
        .write_agent_config(&app, &summary, &endpoint.base_url)
        .await?;
    let extra_env = services
        .plugin_runtime(&app, user.id)
        .await
        .map(|runtime| runtime.2)
        .unwrap_or_default();
    let engine = services.engine(&app, &summary).await?;
    engine
        .start(
            RunRequest {
                request_id,
                prompt,
                session_id,
            },
            Services::run_environment(&summary, key, extra_env),
            on_event,
        )
        .await
}

#[tauri::command]
async fn list_conversations(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<HistorySnapshot, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let user = services.require_user("查看对话").await?;
    history::load(&app, user.id).await
}

#[tauri::command]
async fn save_conversations(
    app: AppHandle,
    state: State<'_, AppState>,
    history: HistorySnapshot,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let user = services.require_user("保存对话").await?;
    history::save(&app, user.id, history).await
}

#[tauri::command]
async fn ack_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
    sequence: u64,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    services.engines.acknowledge(&request_id, sequence).await
}

#[tauri::command]
async fn cancel_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    services.engines.cancel(&request_id).await
}

#[tauri::command]
async fn get_engine_status(
    app: AppHandle,
    state: State<'_, AppState>,
    probe: Option<bool>,
) -> Result<EngineHealth, String> {
    let _ = probe;
    let services = state.services(&app).await?;
    Ok(services.engines.health(&config::sidecar_path(&app)?).await)
}

fn native_commands(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    fn call<F>(handler: F, invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool
    where
        F: Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool,
    {
        handler(invoke)
    }
    call(
        tauri::generate_handler![
            get_app_state,
            switch_endpoint,
            get_public_settings,
            login,
            complete_two_factor,
            logout,
            get_groups,
            get_group_models,
            configure,
            choose_workspace,
            reveal_workspace,
            workspace_action,
            list_plugins,
            install_plugin,
            set_plugin_enabled,
            uninstall_plugin,
            store_catalog,
            store_set_url,
            store_prepare,
            store_cancel,
            store_install,
            list_skills,
            set_skill_enabled,
            uninstall_skill,
            list_plugin_uis,
            list_plugin_mixins,
            get_plugin_settings,
            set_plugin_settings,
            list_conversations,
            save_conversations,
            start_stream,
            ack_stream,
            cancel_stream,
            get_engine_status
        ],
        invoke,
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_secure_store::init())
        .manage(AppState::default())
        .invoke_handler(|invoke| wasm_ipc::intercept(invoke, native_commands))
        .build(tauri::generate_context!())
        .expect("Failed to build Moyu Agent")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(services) = app.state::<AppState>().services.get() {
                    tauri::async_runtime::block_on(async {
                        services.engines.shutdown_all().await;
                    });
                }
            }
        });
}
