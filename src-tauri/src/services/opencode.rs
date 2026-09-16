use super::stream::{
    DeliveryError, FlowControl, StreamEvent, StreamPayload, StreamWriter, BUFFER_CAPACITY,
    MAX_CHUNK_BYTES,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::ipc::Channel;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, watch, Mutex},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

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

pub struct Engine {
    executable: PathBuf,
    working_dir: PathBuf,
    limits: EngineLimits,
    active: Mutex<HashMap<String, Arc<FlowControl>>>,
    version_healthy: AtomicBool,
}

impl Engine {
    pub fn new(executable: PathBuf, working_dir: PathBuf) -> Self {
        Self {
            executable,
            working_dir,
            limits: EngineLimits::default(),
            active: Mutex::new(HashMap::new()),
            version_healthy: AtomicBool::new(false),
        }
    }

    pub async fn health(&self) -> EngineHealth {
        let active_requests = self.active.lock().await.len();
        let executable_available = if active_requests == 0 {
            self.preflight(&CancellationToken::new()).await.is_ok()
        } else {
            self.version_healthy.load(Ordering::Acquire)
        };
        EngineHealth {
            executable_available,
            active_requests,
        }
    }

    async fn preflight(&self, cancellation: &CancellationToken) -> Result<(), StreamPayload> {
        self.version_healthy.store(false, Ordering::Release);
        for attempt in 0..2 {
            if cancellation.is_cancelled() {
                return Err(StreamPayload::Cancelled);
            }
            if self.probe_version(cancellation).await.is_ok() {
                self.version_healthy.store(true, Ordering::Release);
                return Ok(());
            }
            if cancellation.is_cancelled() {
                return Err(StreamPayload::Cancelled);
            }
            if attempt == 0 {
                tracing::warn!("Retrying the OpenCode startup health check");
                tokio::select! {
                    _ = cancellation.cancelled() => return Err(StreamPayload::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(300)) => {}
                }
            }
        }
        Err(StreamPayload::failed("preflight_failed", "OpenCode failed its startup health check after one retry. Reinstall the bundled executable."))
    }

    async fn probe_version(&self, cancellation: &CancellationToken) -> io::Result<()> {
        let mut command = Command::new(&self.executable);
        command
            .arg("--version")
            .current_dir(&self.working_dir)
            .env("OPENCODE_DISABLE_AUTOUPDATE", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn()?;
        let tree = ProcessTree::attach(&child)?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Missing version output"))?
            .take(129);
        let result = tokio::select! {
            _ = cancellation.cancelled() => Err(io::Error::other("Cancelled")),
            result = tokio::time::timeout(Duration::from_secs(3), async {
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).await?;
                let status = child.wait().await?;
                let valid_version = std::str::from_utf8(&bytes).ok().is_some_and(|text| {
                    let version = text.trim();
                    version.starts_with(|c: char| c.is_ascii_digit()) && version.contains('.')
                        && version.bytes().all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
                });
                if status.success() && bytes.len() <= 128 && valid_version { Ok(()) }
                else { Err(io::Error::other("Invalid version response")) }
            }) => result.unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "Version check timeout"))),
        };
        drop(tree);
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
        result
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
            // One CLI at a time also prevents concurrent writes to a resumed session.
            if !active.is_empty() {
                return Err("An agent request is already running".into());
            }
            active.insert(request.request_id.clone(), Arc::clone(&control));
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
        if let Some(control) = self.active.lock().await.get(request_id).cloned() {
            control.acknowledge(sequence)?;
        }
        Ok(())
    }

    pub async fn cancel(&self, request_id: &str) -> Result<(), String> {
        let control = self.active.lock().await.get(request_id).cloned();
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
        let _ = sender
            .send(StreamPayload::Status {
                message: "Checking OpenCode".into(),
            })
            .await;
        if let Err(failure) = self.preflight(&control.cancel).await {
            return failure;
        }
        let _ = sender
            .send(StreamPayload::Status {
                message: "Starting OpenCode".into(),
            })
            .await;
        let mut child = match self.spawn(&request, &environment, &sender, &control).await {
            Ok(child) => child,
            Err(failure) => return failure,
        };
        tracing::info!(request_id = %request.request_id, "OpenCode process started");
        let process_tree = match ProcessTree::attach(&child) {
            Ok(tree) => tree,
            Err(_) => {
                let _ = child.kill().await;
                return StreamPayload::failed(
                    "process_isolation",
                    "Unable to isolate the agent process for safe cleanup",
                );
            }
        };
        let stdout = child
            .stdout
            .take()
            .expect("stdout was configured as a pipe");
        let stderr = child
            .stderr
            .take()
            .expect("stderr was configured as a pipe");
        let mut stdin = child.stdin.take().expect("stdin was configured as a pipe");
        let (activity, activity_receiver) = watch::channel(Instant::now());
        let stdout_sender = sender.clone();
        let stdout_activity = activity.clone();
        let mut stdout_task = tokio::spawn(async move {
            read_stdout(BufReader::new(stdout), stdout_sender, stdout_activity).await
        });
        let mut stderr_task = tokio::spawn(async move {
            let mut stderr = stderr;
            let mut buffer = [0u8; 8192];
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) => return,
                    Ok(_) => {
                        let _ = activity.send(Instant::now());
                    }
                    Err(_) => return,
                }
            }
        });
        let mut stdin_task = tokio::spawn(async move {
            let result = stdin.write_all(request.prompt.as_bytes()).await;
            let _ = stdin.shutdown().await;
            result
        });
        let started = Instant::now();
        let mut watchdog = tokio::time::interval(Duration::from_secs(1));
        let mut stdout_result = None;
        let mut stdout_done = false;
        let mut stdin_done = false;
        let outcome = loop {
            tokio::select! {
                biased;
                _ = control.cancel.cancelled() => break StreamPayload::Cancelled,
                result = &mut stdout_task, if !stdout_done => {
                    stdout_done = true;
                    match result {
                        Ok(Ok(session_id)) => stdout_result = Some(session_id),
                        _ => break StreamPayload::failed("invalid_output", "OpenCode returned invalid or oversized stream output"),
                    }
                }
                result = &mut stdin_task, if !stdin_done => {
                    stdin_done = true;
                    if !matches!(result, Ok(Ok(()))) {
                        break StreamPayload::failed("stdin_closed", "OpenCode closed its input pipe before accepting the prompt");
                    }
                }
                result = child.wait() => {
                    match result {
                        Ok(status) if status.success() => {
                            if !stdout_done {
                                let read_result = tokio::select! {
                                    _ = control.cancel.cancelled() => break StreamPayload::Cancelled,
                                    result = tokio::time::timeout(
                                        self.limits.idle_timeout.min(self.limits.max_duration.saturating_sub(started.elapsed())),
                                        &mut stdout_task,
                                    ) => result,
                                };
                                stdout_done = read_result.is_ok();
                                match read_result {
                                    Ok(Ok(Ok(session_id))) => stdout_result = Some(session_id),
                                    _ => break StreamPayload::failed("stdout_timeout", "OpenCode output did not close after process exit"),
                                }
                            }
                            break StreamPayload::Completed { session_id: stdout_result.flatten() };
                        }
                        Ok(_) => break StreamPayload::failed("process_exit", "OpenCode exited unsuccessfully; review the API configuration before retrying"),
                        Err(_) => break StreamPayload::failed("process_wait", "Unable to read the OpenCode process status"),
                    }
                }
                _ = watchdog.tick() => {
                    if started.elapsed() >= self.limits.max_duration {
                        break StreamPayload::failed("duration_timeout", "The agent exceeded the maximum run duration");
                    }
                    if activity_receiver.borrow().elapsed() >= self.limits.idle_timeout {
                        break StreamPayload::failed("idle_timeout", "OpenCode stopped producing output and was terminated");
                    }
                }
            }
        };
        // Kill the process group/job before joining pipes: descendants can otherwise hold them open.
        drop(process_tree);
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        abort_task(&mut stdout_task, stdout_done).await;
        abort_task(&mut stderr_task, false).await;
        abort_task(&mut stdin_task, stdin_done).await;
        outcome
    }

    async fn spawn(
        &self,
        request: &RunRequest,
        environment: &RunEnvironment,
        sender: &mpsc::Sender<StreamPayload>,
        control: &FlowControl,
    ) -> Result<Child, StreamPayload> {
        for attempt in 0..2 {
            if control.cancel.is_cancelled() {
                return Err(StreamPayload::Cancelled);
            }
            let mut command = Command::new(&self.executable);
            command
                .arg("run")
                .args(["--format", "json"])
                .current_dir(&self.working_dir)
                .env("OPENCODE_CONFIG", &environment.config_path)
                .env("SUB2API_API_KEY", &environment.api_key)
                .env("OPENCODE_DISABLE_AUTOUPDATE", "true")
                .env("OPENCODE_DISABLE_TERMINAL_TITLE", "true")
                .env("NO_COLOR", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            if let Some(session_id) = &request.session_id {
                command.args(["--session", session_id]);
            }
            if let Some(model) = &environment.model {
                command.args(["--model", model]);
            }
            #[cfg(windows)]
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            #[cfg(unix)]
            command.process_group(0);
            match command.spawn() {
                Ok(child) => return Ok(child),
                Err(error)
                    if attempt == 0
                        && matches!(
                            error.kind(),
                            io::ErrorKind::Interrupted
                                | io::ErrorKind::WouldBlock
                                | io::ErrorKind::TimedOut
                        ) =>
                {
                    let _ = sender
                        .send(StreamPayload::Status {
                            message: "Retrying process startup".into(),
                        })
                        .await;
                    tokio::select! {
                        _ = control.cancel.cancelled() => return Err(StreamPayload::Cancelled),
                        _ = tokio::time::sleep(Duration::from_millis(300)) => {}
                    }
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
            }
        }
        Err(StreamPayload::failed(
            "spawn_failed",
            "Unable to start the bundled OpenCode executable",
        ))
    }
}

async fn abort_task<T>(task: &mut JoinHandle<T>, already_joined: bool) {
    if !already_joined {
        task.abort();
        let _ = task.await;
    }
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
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

#[derive(Default)]
struct OutputParser {
    session_id: Option<String>,
    bridge_ready: bool,
    streamed_parts: HashSet<String>,
    completed_parts: HashSet<String>,
}

impl OutputParser {
    fn parse(&mut self, line: &[u8]) -> Result<Option<StreamPayload>, ()> {
        if line.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(None);
        }
        let value: Value = serde_json::from_slice(line).map_err(|_| ())?;
        let kind = value.get("type").and_then(Value::as_str).ok_or(())?;
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
                let id = value.get("partID").and_then(Value::as_str).ok_or(())?;
                if id.len() > 128 {
                    return Err(());
                }
                let text = value.get("text").and_then(Value::as_str).ok_or(())?;
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
                let part = value.get("part").ok_or(())?;
                let id = part.get("id").and_then(Value::as_str).ok_or(())?;
                if id.len() > 128 {
                    return Err(());
                }
                let text = part.get("text").and_then(Value::as_str).ok_or(())?;
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
            "error" => Err(()),
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

async fn read_stdout<R: AsyncBufRead + Unpin>(
    mut reader: R,
    sender: mpsc::Sender<StreamPayload>,
    activity: watch::Sender<Instant>,
) -> Result<Option<String>, ()> {
    let mut line = Vec::with_capacity(8192);
    let mut total = 0usize;
    let mut parser = OutputParser::default();
    while read_bounded_line(&mut reader, &mut line)
        .await
        .map_err(|_| ())?
    {
        total = total.checked_add(line.len()).ok_or(())?;
        if total > MAX_OUTPUT_BYTES {
            return Err(());
        }
        tracing::debug!(bytes = line.len(), "Received OpenCode JSONL output");
        let _ = activity.send(Instant::now());
        if let Some(payload) = parser.parse(&line)? {
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
                            .map_err(|_| ())?;
                        remaining = &remaining[end..];
                    }
                }
                payload => sender.send(payload).await.map_err(|_| ())?,
            }
        }
    }
    if !parser.bridge_ready || parser.session_id.is_none() {
        return Err(());
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

    #[test]
    fn parses_delta_and_does_not_duplicate_completed_part() {
        let mut parser = OutputParser::default();
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
        assert!(parser
            .parse(br#"{"type":"error","error":{"message":"secret"}}"#)
            .is_err());
        assert!(parser.parse(b"not json").is_err());
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
    async fn eof_without_a_bridge_and_session_is_a_failure() {
        for input in [
            "",
            "{\"type\":\"bridge_ready\"}\n",
            "{\"type\":\"step_start\",\"sessionID\":\"ses_1\"}\n",
        ] {
            let (sender, _receiver) = mpsc::channel(BUFFER_CAPACITY);
            let (activity, _) = watch::channel(Instant::now());
            assert!(
                read_stdout(BufReader::new(input.as_bytes()), sender, activity)
                    .await
                    .is_err()
            );
        }
    }

    fn test_environment(directory: &std::path::Path) -> RunEnvironment {
        RunEnvironment {
            config_path: directory.join("opencode.json"),
            api_key: "fixture-only".into(),
            model: None,
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
        assert_eq!(collected.last().unwrap()["code"], "preflight_failed");
        assert!(engine.active.lock().await.is_empty());
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
        assert_eq!(
            collected
                .iter()
                .filter(|event| event["kind"] == "cancelled")
                .count(),
            1
        );
        assert!(engine.active.lock().await.is_empty());
    }
}
