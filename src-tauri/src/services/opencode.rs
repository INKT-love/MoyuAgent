use super::stream::{
    DeliveryError, FlowControl, StreamEvent, StreamPayload, StreamWriter, BUFFER_CAPACITY,
    MAX_CHUNK_BYTES,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::ipc::Channel;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, Mutex},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 64 * 1024;
const MAX_PARTS: usize = 4096;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub request_id: String,
    pub prompt: String,
    pub session_id: Option<String>,
}

// Deliberately no Debug implementation: this contains a decrypted API key.
pub struct RunEnvironment {
    pub config_path: PathBuf,
    pub api_key: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl Drop for RunEnvironment {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.api_key.zeroize();
    }
}

#[derive(Clone)]
pub struct EngineLimits {
    pub idle_timeout: Duration,
    pub max_duration: Duration,
    pub acknowledgement_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub cleanup_timeout: Duration,
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(180),
            max_duration: Duration::from_secs(30 * 60),
            acknowledgement_timeout: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(5),
            cleanup_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineHealth {
    pub executable_available: bool,
    pub active_requests: usize,
}

pub const MAX_PARALLEL_REQUESTS: usize = 8;

struct ActiveRequest {
    control: Arc<FlowControl>,
    session_id: Option<String>,
}

pub fn admit_request<'a>(
    active_count: usize,
    mut session_ids: impl Iterator<Item = Option<&'a str>>,
    session_id: Option<&str>,
) -> Result<(), String> {
    if active_count >= MAX_PARALLEL_REQUESTS {
        return Err("Too many agent requests are already running".into());
    }
    if let Some(session_id) = session_id {
        if session_ids.any(|id| id == Some(session_id)) {
            return Err("This conversation is already running".into());
        }
    }
    Ok(())
}

struct ServerFingerprint {
    config: Vec<u8>,
    api_key: String,
    working_dir: PathBuf,
}

impl Drop for ServerFingerprint {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

impl ServerFingerprint {
    fn matches(&self, config: &[u8], api_key: &str, working_dir: &Path) -> bool {
        self.config == config && self.api_key == api_key && self.working_dir == working_dir
    }
}

struct ServerProcess {
    child: Child,
    _tree: ProcessTree,
    url: String,
    password: String,
    fingerprint: ServerFingerprint,
    stdout_task: JoinHandle<()>,
    stderr_task: JoinHandle<()>,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stdout_task.abort();
        self.stderr_task.abort();
        let _ = self.child.start_kill();
        self.password.zeroize();
    }
}

#[derive(Clone)]
struct ServerHandle {
    url: String,
    password: String,
}

pub struct Engine {
    executable: PathBuf,
    working_dir: PathBuf,
    limits: EngineLimits,
    active: Mutex<HashMap<String, ActiveRequest>>,
    server: Mutex<Option<ServerProcess>>,
    http: reqwest::Client,
    server_ready: AtomicBool,
}

impl Engine {
    pub fn new(executable: PathBuf, working_dir: PathBuf) -> Self {
        Self {
            executable,
            working_dir,
            limits: EngineLimits::default(),
            active: Mutex::new(HashMap::new()),
            server: Mutex::new(None),
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            server_ready: AtomicBool::new(false),
        }
    }

    pub fn workspace(&self) -> &Path {
        &self.working_dir
    }

    pub async fn health(&self) -> EngineHealth {
        EngineHealth {
            executable_available: self.server_ready.load(Ordering::Acquire)
                || tokio::fs::metadata(&self.executable).await.is_ok(),
            active_requests: self.active.lock().await.len(),
        }
    }

    pub async fn shutdown(&self) {
        self.cancel_all().await;
        let mut slot = self.server.lock().await;
        self.server_ready.store(false, Ordering::Release);
        if let Some(server) = slot.take() {
            stop_server(server).await;
        }
    }

    pub async fn ensure_server(&self, environment: &RunEnvironment) -> Result<(), String> {
        if self.active.lock().await.len() > 0 {
            return Ok(());
        }
        self.ensure_server_inner(environment, &CancellationToken::new(), None)
            .await
            .map(|_| ())
            .map_err(payload_message)
    }

    async fn ensure_server_inner(
        &self,
        environment: &RunEnvironment,
        cancellation: &CancellationToken,
        sender: Option<&mpsc::Sender<StreamPayload>>,
    ) -> Result<bool, StreamPayload> {
        if !tokio::fs::try_exists(&self.executable)
            .await
            .unwrap_or(false)
        {
            return Err(StreamPayload::failed(
                "sidecar_missing",
                "The bundled OpenCode executable was not found",
            ));
        }
        let config = tokio::fs::read(&environment.config_path)
            .await
            .map_err(|_| {
                StreamPayload::failed(
                    "spawn_failed",
                    "OpenCode configuration is missing. Save your model settings and retry.",
                )
            })?;
        let mut slot = self.server.lock().await;
        if let Some(server) = slot.as_ref() {
            let healthy = self.probe_health(&server.url, &server.password).await;
            if healthy
                && server
                    .fingerprint
                    .matches(&config, &environment.api_key, &self.working_dir)
            {
                self.server_ready.store(true, Ordering::Release);
                return Ok(false);
            }
            // Keep a busy process so a config change does not abort other sessions.
            if healthy && self.active.lock().await.len() > 1 {
                self.server_ready.store(true, Ordering::Release);
                return Ok(false);
            }
        }
        if let Some(server) = slot.take() {
            self.server_ready.store(false, Ordering::Release);
            stop_server(server).await;
        }
        if cancellation.is_cancelled() {
            return Err(StreamPayload::Cancelled);
        }
        if let Some(sender) = sender {
            let _ = sender
                .send(StreamPayload::Status {
                    message: "Starting OpenCode".into(),
                })
                .await;
        }
        let server = self.start_server(environment, config, cancellation).await?;
        self.server_ready.store(true, Ordering::Release);
        *slot = Some(server);
        Ok(true)
    }

    async fn handle(&self) -> Option<ServerHandle> {
        self.server
            .lock()
            .await
            .as_ref()
            .map(|server| ServerHandle {
                url: server.url.clone(),
                password: server.password.clone(),
            })
    }

    async fn probe_health(&self, url: &str, password: &str) -> bool {
        let Ok(response) = self
            .http
            .get(format!("{url}/global/health"))
            .basic_auth("opencode", Some(password))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        response
            .json::<Value>()
            .await
            .ok()
            .is_some_and(|body| body.get("healthy").and_then(Value::as_bool) == Some(true))
    }

    pub async fn start(
        self: &Arc<Self>,
        request: RunRequest,
        environment: RunEnvironment,
        channel: Channel<StreamEvent>,
    ) -> Result<(), String> {
        uuid::Uuid::parse_str(&request.request_id).map_err(|_| "Invalid request ID")?;
        if request.prompt.trim().is_empty() || request.prompt.len() > MAX_PROMPT_BYTES {
            return Err("Prompt must contain 1 to 65536 UTF-8 bytes".into());
        }
        if request.session_id.as_ref().is_some_and(|id| {
            id.len() > 128
                || !id.starts_with("ses_")
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }) {
            return Err("Invalid OpenCode session ID".into());
        }
        let control = Arc::new(FlowControl::new());
        {
            let mut active = self.active.lock().await;
            admit_request(
                active.len(),
                active.values().map(|item| item.session_id.as_deref()),
                request.session_id.as_deref(),
            )?;
            active.insert(
                request.request_id.clone(),
                ActiveRequest {
                    control: Arc::clone(&control),
                    session_id: request.session_id.clone(),
                },
            );
        }
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            engine
                .supervise(request, environment, channel, control)
                .await;
        });
        Ok(())
    }

    pub async fn acknowledge(&self, request_id: &str, sequence: u64) -> Result<(), String> {
        if let Some(active) = self.active.lock().await.get(request_id) {
            active.control.acknowledge(sequence)?;
        }
        Ok(())
    }

    pub async fn cancel(&self, request_id: &str) -> Result<(), String> {
        let control = self
            .active
            .lock()
            .await
            .get(request_id)
            .map(|item| Arc::clone(&item.control));
        if let Some(control) = control {
            control.cancel.cancel();
            let wait = async {
                loop {
                    let notified = control.completion.notified();
                    if control.finished.load(Ordering::Acquire) {
                        break;
                    }
                    notified.await;
                }
            };
            tokio::time::timeout(self.limits.cleanup_timeout + Duration::from_secs(2), wait)
                .await
                .map_err(|_| "Process cleanup timed out")?;
        }
        Ok(())
    }

    pub async fn cancel_all(&self) {
        let requests: Vec<String> = self.active.lock().await.keys().cloned().collect();
        for request in requests {
            let _ = self.cancel(&request).await;
        }
    }

    async fn supervise(
        self: Arc<Self>,
        request: RunRequest,
        environment: RunEnvironment,
        channel: Channel<StreamEvent>,
        control: Arc<FlowControl>,
    ) {
        let request_id = request.request_id.clone();
        let (sender, mut receiver) = mpsc::channel(BUFFER_CAPACITY);
        let process_engine = Arc::clone(&self);
        let process_control = Arc::clone(&control);
        let mut process = tokio::spawn(async move {
            process_engine
                .run_process(request, environment, sender, process_control)
                .await
        });
        let mut writer = StreamWriter::new(request_id.clone(), channel);
        let mut heartbeat = tokio::time::interval(self.limits.heartbeat_interval);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let forced_terminal = loop {
            let next = tokio::select! {
                biased;
                _ = control.cancel.cancelled() => break Some(StreamPayload::Cancelled),
                next = receiver.recv() => match next { Some(next) => next, None => break None },
                _ = heartbeat.tick() => StreamPayload::Heartbeat,
            };
            if let Err(error) = writer
                .send(next, &control, self.limits.acknowledgement_timeout)
                .await
            {
                control.cancel.cancel();
                break Some(match error {
                    DeliveryError::Cancelled => StreamPayload::Cancelled,
                    DeliveryError::Disconnected => StreamPayload::failed(
                        "channel_disconnected",
                        "The application channel disconnected",
                    ),
                    DeliveryError::Unresponsive => StreamPayload::failed(
                        "ack_timeout",
                        "The interface stopped acknowledging stream data",
                    ),
                });
            }
        };
        // Closing the receiver releases a stdout reader waiting on bounded-channel capacity.
        drop(receiver);
        let outcome = match tokio::time::timeout(self.limits.cleanup_timeout, &mut process).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                StreamPayload::failed("process_task", "The agent supervisor stopped unexpectedly")
            }
            Err(_) => {
                control.cancel.cancel();
                process.abort();
                let _ = process.await;
                StreamPayload::failed(
                    "cleanup_timeout",
                    "The agent process exceeded its cleanup deadline",
                )
            }
        };
        let terminal = forced_terminal.unwrap_or(outcome);
        self.active.lock().await.remove(&request_id);
        control.finished.store(true, Ordering::Release);
        control.completion.notify_waiters();
        writer.terminal(terminal, &control);
    }

    async fn run_process(
        &self,
        request: RunRequest,
        environment: RunEnvironment,
        sender: mpsc::Sender<StreamPayload>,
        control: Arc<FlowControl>,
    ) -> StreamPayload {
        if !self.server_ready.load(Ordering::Acquire) {
            let _ = sender
                .send(StreamPayload::Status {
                    message: "Checking OpenCode".into(),
                })
                .await;
        }
        let spawned = match self
            .ensure_server_inner(&environment, &control.cancel, Some(&sender))
            .await
        {
            Ok(spawned) => spawned,
            Err(failure) => return failure,
        };
        if spawned {
            let _ = sender
                .send(StreamPayload::Status {
                    message: "OpenCode ready".into(),
                })
                .await;
        }
        let Some(handle) = self.handle().await else {
            return StreamPayload::failed("spawn_failed", "OpenCode is not running");
        };
        let session_id = match self
            .resolve_session(&handle, request.session_id.as_deref(), &control.cancel)
            .await
        {
            Ok(id) => id,
            Err(failure) => return failure,
        };
        if let Some(active) = self.active.lock().await.get_mut(&request.request_id) {
            active.session_id = Some(session_id.clone());
        }
        if control.cancel.is_cancelled() {
            return StreamPayload::Cancelled;
        }
        let mut events = match self.open_events(&handle).await {
            Ok(events) => events,
            Err(failure) => return failure,
        };
        if let Err(failure) = self
            .prompt_async(
                &handle,
                &session_id,
                &request.prompt,
                environment.model.as_deref(),
                environment.reasoning_effort.as_deref(),
            )
            .await
        {
            return failure;
        }
        tracing::info!(request_id = %request.request_id, session_id = %session_id, "OpenCode prompt accepted");
        let outcome = self
            .read_events(&mut events, &handle, &session_id, &sender, &control)
            .await;
        if matches!(outcome, StreamPayload::Cancelled) {
            self.abort_session(&handle, &session_id).await;
        }
        outcome
    }

    async fn start_server(
        &self,
        environment: &RunEnvironment,
        config: Vec<u8>,
        cancellation: &CancellationToken,
    ) -> Result<ServerProcess, StreamPayload> {
        let mut last_error = StreamPayload::failed(
            "spawn_failed",
            "Unable to start the bundled OpenCode executable",
        );
        for attempt in 0..2 {
            if cancellation.is_cancelled() {
                return Err(StreamPayload::Cancelled);
            }
            let password = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            let mut command = Command::new(&self.executable);
            command
                .args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
                .current_dir(&self.working_dir)
                .env("OPENCODE_CONFIG", &environment.config_path)
                .env("SUB2API_API_KEY", &environment.api_key)
                .env("OPENCODE_SERVER_PASSWORD", &password)
                .env(
                    "MOYU_HARNESS_INSTRUCTIONS_FILE",
                    environment
                        .config_path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join("harness-instructions.md"),
                )
                .env("OPENCODE_DISABLE_AUTOUPDATE", "true")
                .env("OPENCODE_DISABLE_TERMINAL_TITLE", "true")
                .env("OPENCODE_DISABLE_DEFAULT_PLUGINS", "true")
                .env("NO_COLOR", "1")
                .env("FORCE_COLOR", "0")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            #[cfg(windows)]
            command.creation_flags(0x08000000);
            #[cfg(unix)]
            command.process_group(0);
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error)
                    if attempt == 0
                        && matches!(
                            error.kind(),
                            io::ErrorKind::Interrupted
                                | io::ErrorKind::WouldBlock
                                | io::ErrorKind::TimedOut
                        ) =>
                {
                    tokio::select! {
                        _ = cancellation.cancelled() => return Err(StreamPayload::Cancelled),
                        _ = tokio::time::sleep(Duration::from_millis(300)) => {}
                    }
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(StreamPayload::failed(
                        "sidecar_missing",
                        "The bundled OpenCode executable was not found",
                    ))
                }
                Err(_) => {
                    return Err(StreamPayload::failed(
                        "spawn_failed",
                        "Unable to start the bundled OpenCode executable",
                    ))
                }
            };
            let tree = match ProcessTree::attach(&child) {
                Ok(tree) => tree,
                Err(_) => {
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
                    return Err(StreamPayload::failed(
                        "process_isolation",
                        "Unable to isolate the agent process for safe cleanup",
                    ));
                }
            };
            let stdout = child.stdout.take().ok_or_else(|| {
                StreamPayload::failed("spawn_failed", "OpenCode did not provide server output")
            })?;
            let stderr = child.stderr.take().ok_or_else(|| {
                StreamPayload::failed("spawn_failed", "OpenCode did not provide server output")
            })?;
            let stderr_tail = Arc::new(Mutex::new(Vec::<u8>::new()));
            let stderr_capture = stderr_tail.clone();
            let stderr_task = tokio::spawn(async move {
                drain_pipe(stderr, Some(stderr_capture)).await;
            });
            let mut stdout = BufReader::new(stdout);
            let listen = self
                .wait_for_listen(&mut child, &mut stdout, cancellation)
                .await;
            match listen {
                Ok(url) => {
                    if !self.wait_until_healthy(&url, &password, cancellation).await {
                        stderr_task.abort();
                        let _ = stderr_task.await;
                        drop(tree);
                        let _ = child.start_kill();
                        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                        last_error = StreamPayload::failed(
                            "spawn_failed",
                            "OpenCode started but did not become healthy",
                        );
                        continue;
                    }
                    tracing::info!(%url, "OpenCode server listening");
                    let stdout_task = tokio::spawn(async move {
                        drain_pipe(stdout, None).await;
                    });
                    return Ok(ServerProcess {
                        child,
                        _tree: tree,
                        url,
                        password,
                        fingerprint: ServerFingerprint {
                            config,
                            api_key: environment.api_key.clone(),
                            working_dir: self.working_dir.clone(),
                        },
                        stdout_task,
                        stderr_task,
                    });
                }
                Err(failure) => {
                    stderr_task.abort();
                    let _ = stderr_task.await;
                    drop(tree);
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                    let note = summarize_stderr(&stderr_tail.lock().await);
                    last_error = attach_stderr(failure, &note);
                    if matches!(last_error, StreamPayload::Cancelled) {
                        return Err(last_error);
                    }
                    if attempt == 0 {
                        tokio::select! {
                            _ = cancellation.cancelled() => return Err(StreamPayload::Cancelled),
                            _ = tokio::time::sleep(Duration::from_millis(300)) => {}
                        }
                    }
                }
            }
        }
        Err(last_error)
    }

    async fn wait_for_listen(
        &self,
        child: &mut Child,
        stdout: &mut BufReader<impl tokio::io::AsyncRead + Unpin>,
        cancellation: &CancellationToken,
    ) -> Result<String, StreamPayload> {
        let mut line = String::new();
        let deadline = tokio::time::sleep(Duration::from_secs(15));
        tokio::pin!(deadline);
        loop {
            line.clear();
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(StreamPayload::Cancelled),
                _ = &mut deadline => {
                    return Err(StreamPayload::failed(
                        "spawn_failed",
                        "OpenCode did not report a local listening address",
                    ));
                }
                status = child.wait() => {
                    let _ = status;
                    return Err(StreamPayload::failed(
                        "spawn_failed",
                        "OpenCode exited before the local server was ready",
                    ));
                }
                result = stdout.read_line(&mut line) => {
                    match result {
                        Ok(0) => {
                            return Err(StreamPayload::failed(
                                "spawn_failed",
                                "OpenCode closed output before the local server was ready",
                            ));
                        }
                        Ok(_) => {
                            if let Some(url) = parse_listen_url(&line) {
                                return Ok(url);
                            }
                        }
                        Err(_) => {
                            return Err(StreamPayload::failed(
                                "spawn_failed",
                                "Unable to read the OpenCode server address",
                            ));
                        }
                    }
                }
            }
        }
    }

    async fn wait_until_healthy(
        &self,
        url: &str,
        password: &str,
        cancellation: &CancellationToken,
    ) -> bool {
        for _ in 0..20 {
            if cancellation.is_cancelled() {
                return false;
            }
            if self.probe_health(url, password).await {
                return true;
            }
            tokio::select! {
                _ = cancellation.cancelled() => return false,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
        }
        false
    }

    fn authed(
        &self,
        handle: &ServerHandle,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", handle.url))
            .basic_auth("opencode", Some(&handle.password))
    }

    fn authed_dir(
        &self,
        handle: &ServerHandle,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        self.authed(handle, method, path)
            .query(&[("directory", self.working_dir.to_string_lossy().as_ref())])
    }

    async fn resolve_session(
        &self,
        handle: &ServerHandle,
        requested: Option<&str>,
        cancellation: &CancellationToken,
    ) -> Result<String, StreamPayload> {
        if cancellation.is_cancelled() {
            return Err(StreamPayload::Cancelled);
        }
        if let Some(session_id) = requested {
            match self
                .authed_dir(
                    handle,
                    reqwest::Method::GET,
                    &format!("/session/{session_id}"),
                )
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(session_id.to_owned()),
                Ok(response) if response.status() == reqwest::StatusCode::NOT_FOUND => {}
                Ok(_) => {
                    return Err(StreamPayload::failed(
                        "opencode_http",
                        "OpenCode rejected the previous session; start a new conversation",
                    ))
                }
                Err(_) => {
                    return Err(StreamPayload::failed(
                        "opencode_http",
                        "Unable to reach the local OpenCode server",
                    ))
                }
            }
        }
        self.create_session(handle).await
    }

    async fn create_session(&self, handle: &ServerHandle) -> Result<String, StreamPayload> {
        let response = self
            .authed_dir(handle, reqwest::Method::POST, "/session")
            .timeout(Duration::from_secs(10))
            .json(&json!({}))
            .send()
            .await
            .map_err(|error| {
                StreamPayload::failed(
                    "opencode_http",
                    &format!("Unable to create an OpenCode session: {error}"),
                )
            })?;
        if !response.status().is_success() {
            return Err(prefix_http_error("Unable to create an OpenCode session", response).await);
        }
        let body = response.json::<Value>().await.map_err(|_| {
            StreamPayload::failed("invalid_output", "OpenCode returned an invalid session")
        })?;
        body.get("id")
            .and_then(Value::as_str)
            .filter(|id| id.starts_with("ses_") && id.len() <= 128)
            .map(str::to_owned)
            .ok_or_else(|| {
                StreamPayload::failed("invalid_output", "OpenCode returned an invalid session")
            })
    }

    async fn open_events(&self, handle: &ServerHandle) -> Result<reqwest::Response, StreamPayload> {
        let response = self
            .authed_dir(handle, reqwest::Method::GET, "/event")
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|error| {
                StreamPayload::failed(
                    "opencode_http",
                    &format!("Unable to subscribe to OpenCode events: {error}"),
                )
            })?;
        if !response.status().is_success() {
            return Err(http_error(response).await);
        }
        Ok(response)
    }

    async fn prompt_async(
        &self,
        handle: &ServerHandle,
        session_id: &str,
        prompt: &str,
        model: Option<&str>,
        variant: Option<&str>,
    ) -> Result<(), StreamPayload> {
        let mut body = json!({
            "parts": [{ "type": "text", "text": prompt }]
        });
        let variant = variant.filter(|value| !value.is_empty());
        if let Some(model) = model.filter(|value| !value.is_empty()) {
            let (provider, model_id) = split_model(model);
            let mut model_body = json!({ "providerID": provider, "modelID": model_id });
            if let Some(variant) = variant {
                model_body["variant"] = json!(variant);
                body["variant"] = json!(variant);
            }
            body["model"] = model_body;
        } else if let Some(variant) = variant {
            body["variant"] = json!(variant);
        }
        let response = self
            .authed_dir(
                handle,
                reqwest::Method::POST,
                &format!("/session/{session_id}/prompt_async"),
            )
            .timeout(Duration::from_secs(15))
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                StreamPayload::failed(
                    "opencode_http",
                    &format!("Unable to send the prompt to OpenCode: {error}"),
                )
            })?;
        if response.status().as_u16() == 204 || response.status().is_success() {
            return Ok(());
        }
        Err(http_error(response).await)
    }

    async fn abort_session(&self, handle: &ServerHandle, session_id: &str) {
        let _ = self
            .authed_dir(
                handle,
                reqwest::Method::POST,
                &format!("/session/{session_id}/abort"),
            )
            .timeout(Duration::from_secs(5))
            .send()
            .await;
    }

    async fn read_events(
        &self,
        events: &mut reqwest::Response,
        handle: &ServerHandle,
        session_id: &str,
        sender: &mpsc::Sender<StreamPayload>,
        control: &FlowControl,
    ) -> StreamPayload {
        let mut interpreter = SseInterpreter::new(session_id.to_owned());
        let mut buffer = Vec::new();
        let mut total = 0usize;
        let started = Instant::now();
        let mut last_activity = Instant::now();
        let mut watchdog = tokio::time::interval(Duration::from_secs(1));
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = control.cancel.cancelled() => {
                    self.abort_session(handle, session_id).await;
                    return StreamPayload::Cancelled;
                }
                chunk = events.chunk() => {
                    match chunk {
                        Ok(Some(bytes)) => {
                            total = match total.checked_add(bytes.len()) {
                                Some(value) if value <= MAX_OUTPUT_BYTES => value,
                                _ => return oversized_output(),
                            };
                            buffer.extend_from_slice(&bytes);
                            last_activity = Instant::now();
                            let parsed = match take_sse_events(&mut buffer) {
                                Ok(parsed) => parsed,
                                Err(failure) => return failure,
                            };
                            for event in parsed {
                                match interpreter.apply(&event) {
                                    EventOutcome::Ignore => {}
                                    EventOutcome::Status(message) => {
                                        if sender
                                            .send(StreamPayload::Status { message })
                                            .await
                                            .is_err()
                                        {
                                            return disconnected();
                                        }
                                    }
                                    EventOutcome::Chunk(text) => {
                                        if let Err(failure) =
                                            send_chunks(sender, text, Some(session_id.to_owned())).await
                                        {
                                            return failure;
                                        }
                                    }
                                    EventOutcome::Completed => {
                                        if let Err(failure) = flush_pending(
                                            sender,
                                            session_id,
                                            interpreter.take_pending(),
                                        )
                                        .await
                                        {
                                            return failure;
                                        }
                                        return StreamPayload::Completed {
                                            session_id: Some(session_id.to_owned()),
                                        };
                                    }
                                    EventOutcome::Failed(failure) => {
                                        if control.cancel.is_cancelled() {
                                            return StreamPayload::Cancelled;
                                        }
                                        return failure;
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            return StreamPayload::failed(
                                "opencode_http",
                                "OpenCode closed the event stream before the response finished",
                            );
                        }
                        Err(_) => {
                            if control.cancel.is_cancelled() {
                                return StreamPayload::Cancelled;
                            }
                            return StreamPayload::failed(
                                "opencode_http",
                                "The OpenCode event stream was interrupted",
                            );
                        }
                    }
                }
                _ = watchdog.tick() => {
                    if started.elapsed() >= self.limits.max_duration {
                        self.abort_session(handle, session_id).await;
                        return StreamPayload::failed(
                            "duration_timeout",
                            "The agent exceeded the maximum run duration",
                        );
                    }
                    if last_activity.elapsed() >= self.limits.idle_timeout {
                        self.abort_session(handle, session_id).await;
                        return StreamPayload::failed(
                            "idle_timeout",
                            "OpenCode stopped producing output and was terminated",
                        );
                    }
                }
            }
        }
    }
}

async fn stop_server(mut server: ServerProcess) {
    let _ = server.child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), server.child.wait()).await;
}

async fn drain_pipe<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    tail: Option<Arc<Mutex<Vec<u8>>>>,
) {
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                if let Some(tail) = &tail {
                    let mut log = tail.lock().await;
                    log.extend_from_slice(&buffer[..count]);
                    const KEEP: usize = 4096;
                    if log.len() > KEEP * 2 {
                        let extra = log.len() - KEEP;
                        log.drain(..extra);
                    }
                }
            }
        }
    }
}

fn payload_message(payload: StreamPayload) -> String {
    match payload {
        StreamPayload::Failed { message, .. } => message,
        StreamPayload::Cancelled => "OpenCode startup was cancelled".into(),
        _ => "Unable to start OpenCode".into(),
    }
}

fn parse_listen_url(line: &str) -> Option<String> {
    let line = line.trim();
    let marker = "listening on ";
    let index = line.find(marker)?;
    let rest = line[index + marker.len()..].trim();
    let url = rest.split_whitespace().next()?.trim_end_matches('/');
    let port = url
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| url.strip_prefix("http://localhost:"))?;
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("http://127.0.0.1:{port}"))
}

fn split_model(model: &str) -> (String, String) {
    match model.split_once('/') {
        Some((provider, id)) if !provider.is_empty() && !id.is_empty() => {
            (provider.to_owned(), id.to_owned())
        }
        _ => ("sub2api".into(), model.to_owned()),
    }
}

fn take_sse_events(buffer: &mut Vec<u8>) -> Result<Vec<Value>, StreamPayload> {
    let mut events = Vec::new();
    loop {
        let Some((end, skip)) = find_sse_break(buffer) else {
            if buffer.len() > MAX_LINE_BYTES {
                return Err(oversized_output());
            }
            break;
        };
        let raw: Vec<u8> = buffer.drain(..end).collect();
        let skip = skip.min(buffer.len());
        buffer.drain(..skip);
        if let Some(value) = parse_sse_block(&raw) {
            events.push(value);
        }
    }
    Ok(events)
}

fn find_sse_break(buffer: &[u8]) -> Option<(usize, usize)> {
    let mut index = 0;
    while index < buffer.len() {
        if buffer[index] == b'\r'
            && index + 3 < buffer.len()
            && buffer[index + 1] == b'\n'
            && buffer[index + 2] == b'\r'
            && buffer[index + 3] == b'\n'
        {
            return Some((index, 4));
        }
        if buffer[index] == b'\n' && index + 1 < buffer.len() && buffer[index + 1] == b'\n' {
            return Some((index, 2));
        }
        index += 1;
    }
    None
}

fn parse_sse_block(raw: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(raw).ok()?;
    let mut data = String::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(payload) = line.strip_prefix("data:") {
            let payload = payload.strip_prefix(' ').unwrap_or(payload);
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(payload);
        }
    }
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    serde_json::from_str(&data).ok()
}

#[derive(Debug)]
enum EventOutcome {
    Ignore,
    Chunk(String),
    Status(String),
    Completed,
    Failed(StreamPayload),
}

struct SseInterpreter {
    session_id: String,
    text_parts: HashSet<String>,
    streamed_parts: HashSet<String>,
    pending_text: HashMap<String, String>,
    part_messages: HashMap<String, String>,
    user_messages: HashSet<String>,
    assistant_messages: HashSet<String>,
    ignored_parts: HashSet<String>,
    saw_working: bool,
}

impl SseInterpreter {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            text_parts: HashSet::new(),
            streamed_parts: HashSet::new(),
            pending_text: HashMap::new(),
            part_messages: HashMap::new(),
            user_messages: HashSet::new(),
            assistant_messages: HashSet::new(),
            ignored_parts: HashSet::new(),
            saw_working: false,
        }
    }

    fn remember_message(&mut self, props: &Value) {
        let Some(id) = event_message_id(props) else {
            return;
        };
        let Some(role) = event_message_role(props) else {
            return;
        };
        match role {
            "assistant" => {
                self.user_messages.remove(id);
                self.assistant_messages.insert(id.to_owned());
            }
            "user" | "system" => {
                self.assistant_messages.remove(id);
                self.user_messages.insert(id.to_owned());
            }
            _ => {}
        }
    }

    fn is_user_part(&self, props: &Value) -> bool {
        event_message_role(props).is_some_and(|role| role == "user" || role == "system")
            || event_message_id(props).is_some_and(|id| self.user_messages.contains(id))
    }

    fn ignore_part(&mut self, id: &str) {
        self.ignored_parts.insert(id.to_owned());
        self.text_parts.remove(id);
        self.pending_text.remove(id);
    }

    fn take_pending(&mut self) -> Vec<String> {
        let mut texts = Vec::new();
        for (id, text) in self.pending_text.drain() {
            if self.streamed_parts.contains(&id)
                || self.ignored_parts.contains(&id)
                || !self.text_parts.contains(&id)
                || text.is_empty()
            {
                continue;
            }
            if self
                .part_messages
                .get(&id)
                .is_some_and(|message_id| self.user_messages.contains(message_id))
            {
                continue;
            }
            texts.push(text);
        }
        texts
    }

    fn apply(&mut self, event: &Value) -> EventOutcome {
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            return EventOutcome::Ignore;
        };
        let props = event.get("properties").unwrap_or(event);
        if let Some(session_id) = event_session_id(props) {
            if session_id != self.session_id {
                return EventOutcome::Ignore;
            }
        } else if kind != "session.error" {
            return EventOutcome::Ignore;
        }
        self.remember_message(props);
        match kind {
            "message.updated" | "message.created" => EventOutcome::Ignore,
            "message.part.delta" => {
                if props.get("field").and_then(Value::as_str) != Some("text") {
                    return EventOutcome::Ignore;
                }
                let Some(id) = props.get("partID").and_then(Value::as_str) else {
                    return EventOutcome::Ignore;
                };
                if id.len() > 128 {
                    return EventOutcome::Failed(oversized_output());
                }
                if self.is_user_part(props) || self.ignored_parts.contains(id) {
                    return EventOutcome::Ignore;
                }
                if let Some(message_id) = event_message_id(props) {
                    self.part_messages
                        .insert(id.to_owned(), message_id.to_owned());
                }
                // Reasoning parts also stream field=text; only emit confirmed reply text.
                if !self.text_parts.contains(id) {
                    return EventOutcome::Ignore;
                }
                let Some(delta) = props.get("delta").and_then(Value::as_str) else {
                    return EventOutcome::Ignore;
                };
                if delta.is_empty() {
                    return EventOutcome::Ignore;
                }
                if self.streamed_parts.len() + self.text_parts.len() >= MAX_PARTS {
                    return EventOutcome::Failed(oversized_output());
                }
                self.streamed_parts.insert(id.to_owned());
                EventOutcome::Chunk(delta.to_owned())
            }
            "message.part.updated" => {
                let Some(part) = props.get("part") else {
                    return EventOutcome::Ignore;
                };
                let Some(id) = part.get("id").and_then(Value::as_str) else {
                    return EventOutcome::Ignore;
                };
                if id.len() > 128 {
                    return EventOutcome::Failed(oversized_output());
                }
                if let Some(message_id) = event_message_id(props) {
                    self.part_messages
                        .insert(id.to_owned(), message_id.to_owned());
                }
                if self.is_user_part(props) || !is_visible_text_part(part) {
                    self.ignore_part(id);
                    return EventOutcome::Ignore;
                }
                self.ignored_parts.remove(id);
                self.text_parts.insert(id.to_owned());
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !self.streamed_parts.contains(id) {
                        self.pending_text.insert(id.to_owned(), text.to_owned());
                    }
                }
                EventOutcome::Ignore
            }
            "session.idle" => EventOutcome::Completed,
            "session.error" => {
                let error = props.get("error").cloned().unwrap_or(Value::Null);
                EventOutcome::Failed(StreamPayload::failed(
                    "opencode_error",
                    &opencode_error_message(&json!({ "error": error })),
                ))
            }
            "session.status" => {
                let status = props.get("status").unwrap_or(&Value::Null);
                let kind = status
                    .get("type")
                    .and_then(Value::as_str)
                    .or_else(|| status.as_str());
                if kind == Some("idle") || self.saw_working {
                    return EventOutcome::Ignore;
                }
                self.saw_working = true;
                EventOutcome::Status("Agent is working".into())
            }
            _ => EventOutcome::Ignore,
        }
    }
}

fn event_session_id(properties: &Value) -> Option<&str> {
    properties
        .get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| {
            properties
                .pointer("/part/sessionID")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            properties
                .pointer("/info/sessionID")
                .and_then(Value::as_str)
        })
}

fn event_message_id(properties: &Value) -> Option<&str> {
    properties
        .get("messageID")
        .and_then(Value::as_str)
        .or_else(|| {
            properties
                .pointer("/part/messageID")
                .and_then(Value::as_str)
        })
        .or_else(|| properties.pointer("/info/id").and_then(Value::as_str))
        .or_else(|| properties.pointer("/message/id").and_then(Value::as_str))
}

fn event_message_role(properties: &Value) -> Option<&str> {
    properties
        .pointer("/info/role")
        .and_then(Value::as_str)
        .or_else(|| properties.pointer("/message/role").and_then(Value::as_str))
        .or_else(|| properties.pointer("/part/role").and_then(Value::as_str))
        .or_else(|| properties.get("role").and_then(Value::as_str))
}

fn is_visible_text_part(part: &Value) -> bool {
    part.get("type").and_then(Value::as_str) == Some("text")
        && part.get("ignored").and_then(Value::as_bool) != Some(true)
}

async fn send_chunks(
    sender: &mpsc::Sender<StreamPayload>,
    text: String,
    session_id: Option<String>,
) -> Result<(), StreamPayload> {
    let mut remaining = text.as_str();
    while !remaining.is_empty() {
        let mut end = remaining.len().min(MAX_CHUNK_BYTES);
        while end > 0 && !remaining.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            break;
        }
        sender
            .send(StreamPayload::Chunk {
                text: remaining[..end].to_owned(),
                session_id: session_id.clone(),
            })
            .await
            .map_err(|_| disconnected())?;
        remaining = &remaining[end..];
    }
    Ok(())
}

async fn flush_pending(
    sender: &mpsc::Sender<StreamPayload>,
    session_id: &str,
    pending: Vec<String>,
) -> Result<(), StreamPayload> {
    for text in pending {
        send_chunks(sender, text, Some(session_id.to_owned())).await?;
    }
    Ok(())
}

async fn http_error(response: reqwest::Response) -> StreamPayload {
    prefix_http_error("OpenCode request failed", response).await
}

async fn prefix_http_error(prefix: &str, response: reqwest::Response) -> StreamPayload {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let value = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
    let message = opencode_error_message(&value);
    let detail = if message == "OpenCode reported an error" {
        let snippet: String = body.chars().filter(|c| *c != '\n').take(200).collect();
        if snippet.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status}: {snippet}")
        }
    } else {
        message
    };
    StreamPayload::failed("opencode_http", &format!("{prefix}: {detail}"))
}

fn summarize_stderr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.contains("NO_COLOR")
                && !line.contains("FORCE_COLOR")
                && !line.contains("warnOnDeactivatedColors")
        })
        .last()
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

fn attach_stderr(outcome: StreamPayload, stderr: &str) -> StreamPayload {
    if stderr.is_empty() {
        return outcome;
    }
    match outcome {
        StreamPayload::Failed {
            code,
            message,
            retryable,
        } if matches!(
            code.as_str(),
            "process_exit" | "invalid_output" | "spawn_failed"
        ) && !message.contains(stderr) =>
        {
            StreamPayload::Failed {
                code,
                message: format!("{message}: {stderr}"),
                retryable,
            }
        }
        other => other,
    }
}

fn opencode_error_message(value: &Value) -> String {
    let error = value.get("error").unwrap_or(value);
    let text = error
        .pointer("/data/message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            error
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .or_else(|| {
            error
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .unwrap_or("OpenCode reported an error");
    let mut text: String = text.chars().take(400).collect();
    if text.contains('\n') {
        text = text.replace('\n', " ");
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("certificate verification")
        || lower.contains("unable to verify")
        || lower.contains("self signed certificate")
        || lower.contains("tls handshake")
    {
        return format!(
            "The selected API line failed TLS verification ({text}). Switch to the backup line in Settings and retry."
        );
    }
    text
}

fn oversized_output() -> StreamPayload {
    StreamPayload::failed(
        "invalid_output",
        "OpenCode returned invalid or oversized stream output",
    )
}

#[cfg(test)]
async fn read_bounded_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    line: &mut Vec<u8>,
) -> io::Result<bool> {
    line.clear();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return Ok(!line.is_empty());
        }
        let count = buffer
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(buffer.len());
        if line.len() + count > MAX_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "JSONL line limit exceeded",
            ));
        }
        let newline = buffer[count - 1] == b'\n';
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if newline {
            return Ok(true);
        }
    }
}

#[cfg(test)]
#[derive(Default)]
struct OutputParser {
    session_id: Option<String>,
    bridge_ready: bool,
    streamed_parts: HashSet<String>,
    completed_parts: HashSet<String>,
}

#[cfg(test)]
impl OutputParser {
    fn parse(&mut self, line: &[u8]) -> Result<Option<StreamPayload>, ()> {
        if line.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(None);
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return Ok(None);
        };
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            return Ok(None);
        };
        if let Some(id) = value.get("sessionID").and_then(Value::as_str) {
            if id.len() > 128 {
                return Err(());
            }
            if self
                .session_id
                .as_deref()
                .is_some_and(|current| current != id)
            {
                return Ok(None);
            }
            self.session_id = Some(id.to_owned());
        }
        if self.streamed_parts.len() + self.completed_parts.len() >= MAX_PARTS {
            return Err(());
        }
        match kind {
            "text_delta" => {
                let Some(id) = value.get("partID").and_then(Value::as_str) else {
                    return Ok(None);
                };
                if id.len() > 128 {
                    return Err(());
                }
                let Some(text) = value.get("text").and_then(Value::as_str) else {
                    return Ok(None);
                };
                if self.completed_parts.contains(id) {
                    return Ok(None);
                }
                self.streamed_parts.insert(id.to_owned());
                Ok(Some(StreamPayload::Chunk {
                    text: text.to_owned(),
                    session_id: self.session_id.clone(),
                }))
            }
            "text" => {
                let Some(part) = value.get("part") else {
                    return Ok(None);
                };
                let Some(id) = part.get("id").and_then(Value::as_str) else {
                    return Ok(None);
                };
                if id.len() > 128 {
                    return Err(());
                }
                let Some(text) = part.get("text").and_then(Value::as_str) else {
                    return Ok(None);
                };
                if !self.completed_parts.insert(id.to_owned()) || self.streamed_parts.contains(id) {
                    return Ok(None);
                }
                Ok(Some(StreamPayload::Chunk {
                    text: text.to_owned(),
                    session_id: self.session_id.clone(),
                }))
            }
            "step_start" => Ok(Some(StreamPayload::Status {
                message: "Agent is working".into(),
            })),
            "step_finish" => Ok(None),
            "tool_use" => Ok(Some(StreamPayload::Status {
                message: "Agent tool finished".into(),
            })),
            "error" => Ok(Some(StreamPayload::failed(
                "opencode_error",
                &opencode_error_message(&value),
            ))),
            "bridge_ready" => {
                self.bridge_ready = true;
                Ok(Some(StreamPayload::Status {
                    message: "Incremental stream connected".into(),
                }))
            }
            _ => Ok(None),
        }
    }
}

fn disconnected() -> StreamPayload {
    StreamPayload::failed(
        "channel_disconnected",
        "The application channel disconnected",
    )
}

#[cfg(test)]
async fn read_stdout<R: tokio::io::AsyncBufRead + Unpin>(
    mut reader: R,
    sender: mpsc::Sender<StreamPayload>,
    activity: tokio::sync::watch::Sender<Instant>,
) -> Result<Option<String>, StreamPayload> {
    let mut line = Vec::with_capacity(8192);
    let mut total = 0usize;
    let mut parser = OutputParser::default();
    while read_bounded_line(&mut reader, &mut line)
        .await
        .map_err(|_| oversized_output())?
    {
        total = total.checked_add(line.len()).ok_or_else(oversized_output)?;
        if total > MAX_OUTPUT_BYTES {
            return Err(oversized_output());
        }
        tracing::debug!(bytes = line.len(), "Received OpenCode JSONL output");
        let _ = activity.send(Instant::now());
        let Some(payload) = parser.parse(&line).map_err(|_| oversized_output())? else {
            continue;
        };
        if matches!(payload, StreamPayload::Failed { .. }) {
            return Err(payload);
        }
        match payload {
            StreamPayload::Chunk { text, session_id } => {
                let mut remaining = text.as_str();
                while !remaining.is_empty() {
                    let mut end = remaining.len().min(MAX_CHUNK_BYTES);
                    while !remaining.is_char_boundary(end) {
                        end -= 1;
                    }
                    sender
                        .send(StreamPayload::Chunk {
                            text: remaining[..end].to_owned(),
                            session_id: session_id.clone(),
                        })
                        .await
                        .map_err(|_| disconnected())?;
                    remaining = &remaining[end..];
                }
            }
            payload => sender.send(payload).await.map_err(|_| disconnected())?,
        }
    }
    if parser.session_id.is_none() {
        return Err(StreamPayload::failed(
            "invalid_output",
            "OpenCode produced no usable stream output",
        ));
    }
    Ok(parser.session_id)
}

#[cfg(unix)]
struct ProcessTree(u32);

#[cfg(unix)]
impl ProcessTree {
    fn attach(child: &Child) -> io::Result<Self> {
        child
            .id()
            .map(Self)
            .ok_or_else(|| io::Error::other("Missing process ID"))
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // Each CLI is spawned in its own process group, including its tool subprocesses.
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
struct ProcessTree(isize);

#[cfg(windows)]
impl ProcessTree {
    fn attach(child: &Child) -> io::Result<Self> {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                },
                Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
            },
        };
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("Missing process ID"))?;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let tree = Self(job as isize);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            let assigned = AssignProcessToJobObject(job, process);
            let error = io::Error::last_os_error();
            CloseHandle(process);
            if assigned == 0 {
                return Err(error);
            }
            Ok(tree)
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::watch;

    #[test]
    fn admit_request_allows_parallel_sessions_and_rejects_the_same_one() {
        assert!(admit_request(0, std::iter::empty(), None).is_ok());
        assert!(admit_request(1, [None].into_iter(), None).is_ok());
        assert!(admit_request(1, ["ses_a"].into_iter().map(Some), Some("ses_b")).is_ok());
        assert_eq!(
            admit_request(1, ["ses_a"].into_iter().map(Some), Some("ses_a")).unwrap_err(),
            "This conversation is already running"
        );
        assert_eq!(
            admit_request(MAX_PARALLEL_REQUESTS, std::iter::empty(), None).unwrap_err(),
            "Too many agent requests are already running"
        );
    }

    #[test]
    fn parses_delta_and_does_not_duplicate_completed_part() {
        let mut parser = OutputParser::default();
        assert!(matches!(
            parser.parse(br#"{"type":"bridge_ready"}"#),
            Ok(Some(StreamPayload::Status { .. }))
        ));
        assert!(parser.bridge_ready);
        assert!(matches!(
            parser.parse(
                br#"{"type":"text_delta","sessionID":"ses_1","partID":"prt_1","text":"hello"}"#
            ),
            Ok(Some(StreamPayload::Chunk { .. }))
        ));
        assert!(parser
            .parse(br#"{"type":"text","sessionID":"ses_1","part":{"id":"prt_1","text":"hello"}}"#)
            .unwrap()
            .is_none());
        assert!(parser.parse(br#"{"type":"text_delta","sessionID":"ses_other","partID":"prt_2","text":"hidden"}"#).unwrap().is_none());
    }

    #[test]
    fn stock_completed_text_is_supported_and_errors_never_look_completed() {
        let mut parser = OutputParser::default();
        assert!(matches!(
            parser.parse(
                br#"{"type":"text","sessionID":"ses_1","part":{"id":"prt_1","text":"hello"}}"#
            ),
            Ok(Some(StreamPayload::Chunk { .. }))
        ));
        match parser
            .parse(br#"{"type":"error","error":{"message":"secret"}}"#)
            .unwrap()
            .unwrap()
        {
            StreamPayload::Failed { code, message, .. } => {
                assert_eq!(code, "opencode_error");
                assert!(message.contains("secret"));
            }
            other => panic!("{other:?}"),
        }
        assert!(parser.parse(b"not json").unwrap().is_none());
        assert!(parser.parse(br#"{"foo":1}"#).unwrap().is_none());
    }

    #[test]
    fn opencode_error_events_surface_the_provider_message() {
        let mut parser = OutputParser::default();
        match parser
            .parse(
                br#"{"type":"error","sessionID":"ses_1","error":{"name":"ProviderError","data":{"message":"model not found"}}}"#,
            )
            .unwrap()
            .unwrap()
        {
            StreamPayload::Failed { code, message, .. } => {
                assert_eq!(code, "opencode_error");
                assert!(message.contains("model not found"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn certificate_errors_tell_the_user_to_switch_lines() {
        let mut parser = OutputParser::default();
        match parser
            .parse(
                br#"{"type":"error","error":{"data":{"message":"unknown certificate verification error"}}}"#,
            )
            .unwrap()
            .unwrap()
        {
            StreamPayload::Failed { code, message, .. } => {
                assert_eq!(code, "opencode_error");
                assert!(message.contains("TLS verification"));
                assert!(message.contains("backup line"));
                assert!(message.contains("unknown certificate verification error"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn unterminated_oversized_line_is_rejected_before_allocation_grows() {
        let bytes = vec![b'x'; MAX_LINE_BYTES + 100];
        let mut reader = BufReader::new(bytes.as_slice());
        let mut line = Vec::new();
        assert_eq!(
            read_bounded_line(&mut reader, &mut line)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(line.len() <= MAX_LINE_BYTES);
    }

    #[tokio::test]
    async fn eof_and_crlf_lines_are_supported() {
        let mut reader = BufReader::new(&b"one\r\ntwo"[..]);
        let mut line = Vec::new();
        assert!(read_bounded_line(&mut reader, &mut line).await.unwrap());
        assert_eq!(line, b"one\r\n");
        assert!(read_bounded_line(&mut reader, &mut line).await.unwrap());
        assert_eq!(line, b"two");
        assert!(!read_bounded_line(&mut reader, &mut line).await.unwrap());
    }

    #[tokio::test]
    async fn utf8_chunks_are_bounded_and_lossless() {
        let text = "\u{4e2d}".repeat(MAX_CHUNK_BYTES);
        let mut input = b"{\"type\":\"bridge_ready\"}\n".to_vec();
        input.extend(serde_json::to_vec(&serde_json::json!({"type":"text", "sessionID":"ses_1", "part":{"id":"prt_1", "text":text}})).unwrap());
        let (sender, mut receiver) = mpsc::channel(BUFFER_CAPACITY);
        let (activity, _) = watch::channel(Instant::now());
        read_stdout(BufReader::new(input.as_slice()), sender, activity)
            .await
            .unwrap();
        let mut reconstructed = String::new();
        while let Some(payload) = receiver.recv().await {
            if let StreamPayload::Chunk { text, .. } = payload {
                assert!(text.len() <= MAX_CHUNK_BYTES);
                reconstructed.push_str(&text);
            }
        }
        assert_eq!(reconstructed, text);
    }

    #[tokio::test]
    async fn a_slow_consumer_backpressures_the_stdout_reader() {
        let input = String::from("{\"type\":\"bridge_ready\"}\n")
            + &(0..200)
                .map(|part| {
                    serde_json::json!({
            "type":"text_delta", "sessionID":"ses_1", "partID":format!("prt_{part}"), "text":"hello"
        }).to_string() + "\n"
                })
                .collect::<String>();
        let (sender, mut receiver) = mpsc::channel(BUFFER_CAPACITY);
        let (activity, _) = watch::channel(Instant::now());
        let mut task = tokio::spawn(async move {
            read_stdout(BufReader::new(input.as_bytes()), sender, activity).await
        });
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut task)
            .await
            .is_err());
        assert_eq!(receiver.len(), BUFFER_CAPACITY);
        let mut count = 0;
        while receiver.recv().await.is_some() {
            count += 1;
        }
        assert_eq!(count, 201);
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn eof_without_a_session_is_a_failure() {
        for input in ["", "{\"type\":\"bridge_ready\"}\n"] {
            let (sender, _receiver) = mpsc::channel(BUFFER_CAPACITY);
            let (activity, _) = watch::channel(Instant::now());
            assert!(
                read_stdout(BufReader::new(input.as_bytes()), sender, activity)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn native_json_without_bridge_still_completes() {
        let input = concat!(
            "{\"type\":\"step_start\",\"sessionID\":\"ses_1\"}\n",
            "{\"type\":\"text\",\"sessionID\":\"ses_1\",\"part\":{\"id\":\"prt_1\",\"text\":\"hi\"}}\n",
        );
        let (sender, mut receiver) = mpsc::channel(BUFFER_CAPACITY);
        let (activity, _) = watch::channel(Instant::now());
        let session = read_stdout(BufReader::new(input.as_bytes()), sender, activity)
            .await
            .unwrap();
        assert_eq!(session.as_deref(), Some("ses_1"));
        let mut texts = Vec::new();
        while let Some(payload) = receiver.recv().await {
            if let StreamPayload::Chunk { text, .. } = payload {
                texts.push(text);
            }
        }
        assert_eq!(texts, ["hi"]);
    }

    #[tokio::test]
    async fn opencode_error_line_stops_the_reader() {
        let input = concat!(
            "{\"type\":\"bridge_ready\"}\n",
            "{\"type\":\"error\",\"sessionID\":\"ses_1\",\"error\":{\"data\":{\"message\":\"quota exceeded\"}}}\n",
        );
        let (sender, _receiver) = mpsc::channel(BUFFER_CAPACITY);
        let (activity, _) = watch::channel(Instant::now());
        match read_stdout(BufReader::new(input.as_bytes()), sender, activity)
            .await
            .unwrap_err()
        {
            StreamPayload::Failed { code, message, .. } => {
                assert_eq!(code, "opencode_error");
                assert!(message.contains("quota exceeded"));
            }
            other => panic!("{other:?}"),
        }
    }

    fn test_environment(directory: &std::path::Path) -> RunEnvironment {
        RunEnvironment {
            config_path: directory.join("opencode.json"),
            api_key: "fixture-only".into(),
            model: None,
            reasoning_effort: None,
        }
    }

    fn recording_channel() -> (Channel<StreamEvent>, mpsc::UnboundedReceiver<Value>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let channel = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                let _ = sender.send(serde_json::from_str::<Value>(&json).unwrap());
            }
            Ok(())
        });
        (channel, receiver)
    }

    #[tokio::test]
    async fn failed_preflight_emits_one_terminal_and_releases_the_active_request() {
        let directory = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(
            directory.path().join("missing-opencode.exe"),
            directory.path().into(),
        ));
        let (channel, mut events) = recording_channel();
        engine
            .start(
                RunRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    prompt: "test".into(),
                    session_id: None,
                },
                test_environment(directory.path()),
                channel,
            )
            .await
            .unwrap();
        let collected = tokio::time::timeout(Duration::from_secs(5), async {
            let mut collected = Vec::new();
            while let Some(event) = events.recv().await {
                collected.push(event);
            }
            collected
        })
        .await
        .unwrap();
        assert_eq!(
            collected
                .iter()
                .filter(|event| event["kind"] == "failed")
                .count(),
            1
        );
        assert_eq!(collected.last().unwrap()["code"], "sidecar_missing");
        assert!(engine.active.lock().await.is_empty());
    }

    #[test]
    fn listen_url_must_be_loopback() {
        assert_eq!(
            parse_listen_url("opencode server listening on http://127.0.0.1:18780"),
            Some("http://127.0.0.1:18780".into())
        );
        assert_eq!(
            parse_listen_url("listening on http://localhost:9"),
            Some("http://127.0.0.1:9".into())
        );
        assert!(parse_listen_url("listening on http://0.0.0.0:18780").is_none());
        assert!(parse_listen_url("listening on http://127.0.0.1:abc").is_none());
    }

    #[test]
    fn sse_parser_extracts_json_data_and_ignores_comments() {
        let mut buffer =
            b": ping\n\ndata: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_1\"}}\n\npartial"
                .to_vec();
        let events = take_sse_events(&mut buffer).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "session.idle");
        assert_eq!(buffer, b"partial");
    }

    #[test]
    fn sse_interpreter_streams_text_and_completes_on_idle() {
        let mut interpreter = SseInterpreter::new("ses_1".into());
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "message.part.updated",
                "properties": {
                    "sessionID": "ses_1",
                    "part": { "id": "prt_1", "type": "text", "text": "" },
                    "time": 1
                }
            })),
            EventOutcome::Ignore
        ));
        match interpreter.apply(&json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_1",
                "messageID": "msg_1",
                "partID": "prt_1",
                "field": "text",
                "delta": "hello"
            }
        })) {
            EventOutcome::Chunk(text) => assert_eq!(text, "hello"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "message.part.delta",
                "properties": {
                    "sessionID": "ses_other",
                    "messageID": "msg_1",
                    "partID": "prt_2",
                    "field": "text",
                    "delta": "hidden"
                }
            })),
            EventOutcome::Ignore
        ));
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            })),
            EventOutcome::Completed
        ));
        assert!(interpreter.take_pending().is_empty());
    }

    #[test]
    fn sse_interpreter_emits_completed_text_when_deltas_are_missing() {
        let mut interpreter = SseInterpreter::new("ses_1".into());
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": { "id": "prt_1", "type": "text", "text": "hello" },
                "time": 1
            }
        }));
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            })),
            EventOutcome::Completed
        ));
        assert_eq!(interpreter.take_pending(), ["hello"]);
    }

    #[test]
    fn sse_interpreter_does_not_echo_the_user_prompt() {
        let mut interpreter = SseInterpreter::new("ses_1".into());
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "message.updated",
                "properties": {
                    "info": { "id": "msg_user", "sessionID": "ses_1", "role": "user" }
                }
            })),
            EventOutcome::Ignore
        ));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_user",
                    "type": "text",
                    "text": "hi",
                    "messageID": "msg_user",
                    "sessionID": "ses_1"
                }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.updated",
            "properties": {
                "info": { "id": "msg_asst", "sessionID": "ses_1", "role": "assistant" }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_asst",
                    "type": "text",
                    "text": "",
                    "messageID": "msg_asst",
                    "sessionID": "ses_1"
                }
            }
        }));
        match interpreter.apply(&json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_1",
                "messageID": "msg_asst",
                "partID": "prt_asst",
                "field": "text",
                "delta": "Hello!"
            }
        })) {
            EventOutcome::Chunk(text) => assert_eq!(text, "Hello!"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "message.part.delta",
                "properties": {
                    "sessionID": "ses_1",
                    "messageID": "msg_user",
                    "partID": "prt_user",
                    "field": "text",
                    "delta": "hi"
                }
            })),
            EventOutcome::Ignore
        ));
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            })),
            EventOutcome::Completed
        ));
        assert!(interpreter.take_pending().is_empty());
    }

    #[test]
    fn sse_interpreter_flushes_assistant_text_without_the_user_prompt() {
        let mut interpreter = SseInterpreter::new("ses_1".into());
        interpreter.apply(&json!({
            "type": "message.updated",
            "properties": {
                "info": { "id": "msg_user", "sessionID": "ses_1", "role": "user" }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_user",
                    "type": "text",
                    "text": "hi",
                    "messageID": "msg_user",
                    "sessionID": "ses_1"
                }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.updated",
            "properties": {
                "info": { "id": "msg_asst", "sessionID": "ses_1", "role": "assistant" }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_asst",
                    "type": "text",
                    "text": "Hello!",
                    "messageID": "msg_asst",
                    "sessionID": "ses_1"
                }
            }
        }));
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            })),
            EventOutcome::Completed
        ));
        assert_eq!(interpreter.take_pending(), ["Hello!"]);
    }

    #[test]
    fn sse_interpreter_does_not_stream_reasoning_as_reply() {
        let mut interpreter = SseInterpreter::new("ses_1".into());
        interpreter.apply(&json!({
            "type": "message.updated",
            "properties": {
                "info": { "id": "msg_asst", "sessionID": "ses_1", "role": "assistant" }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_think",
                    "type": "reasoning",
                    "text": "",
                    "messageID": "msg_asst",
                    "sessionID": "ses_1"
                }
            }
        }));
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "message.part.delta",
                "properties": {
                    "sessionID": "ses_1",
                    "messageID": "msg_asst",
                    "partID": "prt_think",
                    "field": "text",
                    "delta": "Let me think..."
                }
            })),
            EventOutcome::Ignore
        ));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_asst",
                    "type": "text",
                    "text": "",
                    "messageID": "msg_asst",
                    "sessionID": "ses_1"
                }
            }
        }));
        match interpreter.apply(&json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_1",
                "messageID": "msg_asst",
                "partID": "prt_asst",
                "field": "text",
                "delta": "Hello!"
            }
        })) {
            EventOutcome::Chunk(text) => assert_eq!(text, "Hello!"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            })),
            EventOutcome::Completed
        ));
        assert!(interpreter.take_pending().is_empty());
    }

    #[test]
    fn sse_interpreter_does_not_flush_reasoning_or_ignored_text() {
        let mut interpreter = SseInterpreter::new("ses_1".into());
        interpreter.apply(&json!({
            "type": "message.updated",
            "properties": {
                "info": { "id": "msg_asst", "sessionID": "ses_1", "role": "assistant" }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_think",
                    "type": "reasoning",
                    "text": "Let me think...",
                    "messageID": "msg_asst",
                    "sessionID": "ses_1"
                }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_hidden",
                    "type": "text",
                    "text": "internal",
                    "ignored": true,
                    "messageID": "msg_asst",
                    "sessionID": "ses_1"
                }
            }
        }));
        interpreter.apply(&json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_asst",
                    "type": "text",
                    "text": "Hello!",
                    "messageID": "msg_asst",
                    "sessionID": "ses_1"
                }
            }
        }));
        assert!(matches!(
            interpreter.apply(&json!({
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            })),
            EventOutcome::Completed
        ));
        assert_eq!(interpreter.take_pending(), ["Hello!"]);
    }

    #[tokio::test]
    async fn cancellation_before_prompt_acceptance_has_exactly_one_terminal() {
        let directory = tempfile::tempdir().unwrap();
        let engine = Arc::new(Engine::new(
            directory.path().join("missing-opencode.exe"),
            directory.path().into(),
        ));
        let request_id = uuid::Uuid::new_v4().to_string();
        let (channel, mut events) = recording_channel();
        engine
            .start(
                RunRequest {
                    request_id: request_id.clone(),
                    prompt: "test".into(),
                    session_id: None,
                },
                test_environment(directory.path()),
                channel,
            )
            .await
            .unwrap();
        engine.cancel(&request_id).await.unwrap();
        let mut collected = Vec::new();
        while let Some(event) = events.recv().await {
            collected.push(event);
        }
        let terminals = collected
            .iter()
            .filter(|event| event["kind"] == "cancelled" || event["kind"] == "failed")
            .count();
        assert_eq!(terminals, 1);
        assert!(
            collected.last().unwrap()["kind"] == "cancelled"
                || collected.last().unwrap()["kind"] == "failed"
        );
        assert!(engine.active.lock().await.is_empty());
    }
}
