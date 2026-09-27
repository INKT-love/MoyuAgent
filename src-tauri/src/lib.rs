mod services;

use serde::Serialize;
use services::{
    api_client::{self, ApiClient, Endpoint},
    auth::{self, AuthService, Group, GroupModel, LoginResult, User},
    config::{self, ConfigSummary, Preferences, WorkspaceActionResult},
    harness::{self, PluginInfo, PluginMixin, PluginUi},
    engines::EnginePool,
    history::{self, HistorySnapshot},
    opencode::{Engine, EngineHealth, RunEnvironment, RunRequest},
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
}

impl AppState {
    async fn services(&self, app: &AppHandle) -> Result<&Services, String> {
        self.services
            .get_or_try_init(|| async {
                let preferences = config::load(app).await?;
                harness::seed_bundled(app).await?;
                wasm_ipc::reload(app, &preferences.enabled_plugins).await?;
                let api = Arc::new(ApiClient::new(preferences.endpoint_index.min(1))?);
                Ok(Services {
                    auth: AuthService::new(api.clone()),
                    api,
                    preferences: Mutex::new(preferences),
                    user: Mutex::new(None),
                    engines: EnginePool::new(),
                    mutation: Mutex::new(()),
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

    async fn write_agent_config(
        &self,
        app: &AppHandle,
        summary: &ConfigSummary,
        base_url: &str,
    ) -> Result<(), String> {
        let enabled = self.preferences.lock().await.enabled_plugins.clone();
        config::write_opencode_config(app, summary, base_url, &enabled).await?;
        Ok(())
    }

    async fn engine(&self, app: &AppHandle, config: &ConfigSummary) -> Result<Arc<Engine>, String> {
        self.engines
            .get_or_create(
                config::sidecar_path(app)?,
                std::path::PathBuf::from(&config.working_directory),
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
        if let Err(error) = engine
            .ensure_server(&RunEnvironment {
                config_path: summary.config_path.into(),
                api_key: key,
                model: Some(format!("sub2api/{}", summary.model)),
                reasoning_effort: Some(config::normalize_reasoning_effort(Some(
                    &summary.reasoning_effort,
                ))),
            })
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
    let (enabled, previous, previous_effort, previous_permission) = {
        let prefs = services.preferences.lock().await;
        (
            prefs.enabled_plugins.clone(),
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
            reasoning_effort
                .as_deref()
                .or(previous_effort.as_deref()),
        ),
        permission_mode: config::normalize_permission_mode(
            permission_mode
                .as_deref()
                .or(previous_permission.as_deref()),
        ),
    };
    config::write_opencode_config(
        &app,
        &summary,
        services.api.get_current_base_url(),
        &enabled,
    )
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
    prefs.enabled_plugins.retain(|item| item != &id);
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
    let enabled = {
        let mut prefs = services.preferences.lock().await;
        if prefs.endpoint_index != endpoint.index {
            prefs.endpoint_index = endpoint.index;
            config::save(&app, prefs.clone()).await?;
        }
        prefs.enabled_plugins.clone()
    };
    config::write_opencode_config(&app, &summary, &endpoint.base_url, &enabled).await?;
    let engine = services.engine(&app, &summary).await?;
    engine
        .start(
            RunRequest {
                request_id,
                prompt,
                session_id,
            },
            RunEnvironment {
                config_path: summary.config_path.into(),
                api_key: key,
                model: Some(format!("sub2api/{}", summary.model)),
                reasoning_effort: Some(config::normalize_reasoning_effort(Some(
                    &summary.reasoning_effort,
                ))),
            },
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
) -> Result<EngineHealth, String> {
    let services = state.services(&app).await?;
    Ok(services
        .engines
        .health(&config::sidecar_path(&app)?)
        .await)
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
            list_plugin_uis,
            list_plugin_mixins,
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
