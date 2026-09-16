mod services;

use serde::Serialize;
use services::{
    api_client::{self, ApiClient, Endpoint},
    auth::{self, AuthService, Group, LoginResult, User},
    config::{self, ConfigSummary, Preferences},
    opencode::{Engine, EngineHealth, RunEnvironment, RunRequest},
    stream::StreamEvent,
};
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
    engine: Mutex<Option<Arc<Engine>>>,
    mutation: Mutex<()>,
}

impl AppState {
    async fn services(&self, app: &AppHandle) -> Result<&Services, String> {
        self.services
            .get_or_try_init(|| async {
                let preferences = config::load(app).await?;
                let api = Arc::new(ApiClient::new(preferences.endpoint_index.min(1))?);
                Ok(Services {
                    auth: AuthService::new(api.clone()),
                    api,
                    preferences: Mutex::new(preferences),
                    user: Mutex::new(None),
                    engine: Mutex::new(None),
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
}

impl Services {
    async fn snapshot(&self, app: &AppHandle) -> Result<Snapshot, String> {
        let user = self.user.lock().await.clone();
        let mut prefs = self.preferences.lock().await;
        if prefs.endpoint_index != self.api.endpoint().index {
            prefs.endpoint_index = self.api.endpoint().index;
            config::save(app, prefs.clone()).await?;
        }
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
        })
    }

    async fn ensure_idle(&self) -> Result<(), String> {
        if let Some(engine) = self.engine.lock().await.as_ref() {
            if engine.health().await.active_requests > 0 {
                return Err("Stop the current response before changing configuration".into());
            }
        }
        Ok(())
    }

    async fn engine(&self, app: &AppHandle, config: &ConfigSummary) -> Result<Arc<Engine>, String> {
        let mut engine = self.engine.lock().await;
        if engine.is_none() {
            *engine = Some(Arc::new(Engine::new(
                config::sidecar_path(app)?,
                config.working_directory.clone().into(),
            )));
        }
        Ok(engine.as_ref().unwrap().clone())
    }
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
    services.snapshot(&app).await
}

#[tauri::command]
async fn switch_endpoint(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<Endpoint, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services.ensure_idle().await?;
    let endpoint = services.api.switch_endpoint(Some(index))?;
    let mut prefs = services.preferences.lock().await;
    prefs.endpoint_index = index;
    config::save(&app, prefs.clone()).await?;
    if let Some(config) = &prefs.config {
        config::write_opencode_config(&app, config, &endpoint.base_url).await?;
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
    let _ = services.snapshot(&app).await?;
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
    let _ = services.snapshot(&app).await?;
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
    let _ = services.snapshot(&app).await?;
    Ok(result)
}

#[tauri::command]
async fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    if let Some(engine) = services.engine.lock().await.take() {
        engine.cancel_all().await;
    }
    services.auth.logout().await?;
    *services.user.lock().await = None;
    Ok(())
}

#[tauri::command]
async fn get_groups(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<Group>, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    let groups = services.auth.groups().await?;
    let _ = services.snapshot(&app).await?;
    Ok(groups
        .into_iter()
        .filter(|g| matches!(g.platform.as_str(), "anthropic" | "claude" | "openai"))
        .collect())
}

#[tauri::command]
async fn configure(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: Option<i64>,
    model: Option<String>,
    workspace: Option<String>,
) -> Result<ConfigSummary, String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services.ensure_idle().await?;
    let user = services
        .auth
        .current_user()
        .await?
        .ok_or("Sign in before configuring OpenCode")?;
    *services.user.lock().await = Some(user.clone());
    let groups = services.auth.groups().await?;
    let group = groups
        .iter()
        .find(|g| {
            group_id.is_none_or(|id| id == g.id)
                && matches!(g.platform.as_str(), "anthropic" | "claude" | "openai")
        })
        .ok_or("No supported API group is available for this account")?;
    let model = model.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| {
        if group.platform == "openai" {
            "gpt-5.2".into()
        } else {
            "claude-sonnet-4-6".into()
        }
    });
    if model.len() > 160
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:/".contains(&b))
    {
        return Err("Invalid model identifier".into());
    }
    let directory = config::working_directory(&app, workspace).await?;
    let _api_key = zeroize::Zeroizing::new(services.auth.ensure_api_key(group.id, user.id).await?);
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
    };
    config::write_opencode_config(&app, &summary, services.api.get_current_base_url()).await?;
    let mut prefs = services.preferences.lock().await;
    prefs.config = Some(summary.clone());
    prefs.endpoint_index = services.api.endpoint().index;
    config::save(&app, prefs.clone()).await?;
    *services.engine.lock().await = None;
    Ok(summary)
}

#[tauri::command]
async fn start_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
    prompt: String,
    session_id: Option<String>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    let _guard = services.mutation.lock().await;
    services.ensure_idle().await?;
    let user = services
        .user
        .lock()
        .await
        .clone()
        .ok_or("Sign in before starting the agent")?;
    let summary = services
        .preferences
        .lock()
        .await
        .config
        .clone()
        .filter(|c| c.user_id == user.id)
        .ok_or("Configure your API group before starting the agent")?;
    let key = tauri_plugin_secure_store::get_secret(&auth::api_key_secret_name(
        user.id,
        summary.group_id,
    ))
    .await?
    .ok_or("The API key is unavailable. Run configuration again.")?;
    config::write_opencode_config(&app, &summary, services.api.get_current_base_url()).await?;
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
            },
            on_event,
        )
        .await
}

#[tauri::command]
async fn ack_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
    sequence: u64,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    if let Some(engine) = services.engine.lock().await.clone() {
        engine.acknowledge(&request_id, sequence).await?;
    }
    Ok(())
}

#[tauri::command]
async fn cancel_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    let services = state.services(&app).await?;
    if let Some(engine) = services.engine.lock().await.clone() {
        engine.cancel(&request_id).await?;
    }
    Ok(())
}

#[tauri::command]
async fn get_engine_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<EngineHealth, String> {
    let services = state.services(&app).await?;
    if let Some(engine) = services.engine.lock().await.clone() {
        return Ok(engine.health().await);
    }
    Ok(
        Engine::new(config::sidecar_path(&app)?, config::app_directory(&app)?)
            .health()
            .await,
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_secure_store::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            switch_endpoint,
            get_public_settings,
            login,
            complete_two_factor,
            logout,
            get_groups,
            configure,
            start_stream,
            ack_stream,
            cancel_stream,
            get_engine_status
        ])
        .build(tauri::generate_context!())
        .expect("Failed to build Moyu Agent")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(services) = app.state::<AppState>().services.get() {
                    tauri::async_runtime::block_on(async {
                        if let Some(engine) = services.engine.lock().await.as_ref() {
                            engine.cancel_all().await;
                        }
                    });
                }
            }
        });
}
