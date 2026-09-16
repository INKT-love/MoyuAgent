use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::ipc::Channel;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub const BUFFER_CAPACITY: usize = 100;
pub const ACK_WINDOW: u64 = 128;
pub const MAX_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEvent {
    pub request_id: String,
    pub sequence: u64,
    #[serde(flatten)]
    pub payload: StreamPayload,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum StreamPayload {
    Chunk {
        text: String,
        session_id: Option<String>,
    },
    Status {
        message: String,
    },
    Heartbeat,
    Completed {
        session_id: Option<String>,
    },
    Failed {
        code: String,
        message: String,
        retryable: bool,
    },
    Cancelled,
}

impl StreamPayload {
    pub fn failed(code: &str, message: &str) -> Self {
        Self::Failed {
            code: code.into(),
            message: message.into(),
            retryable: matches!(
                code,
                "sidecar_missing" | "spawn_failed" | "preflight_failed"
            ),
        }
    }
}

pub(super) struct FlowControl {
    pub cancel: CancellationToken,
    sent: AtomicU64,
    acknowledged: AtomicU64,
    acknowledgement: Notify,
    pub finished: AtomicBool,
    pub completion: Notify,
}

impl FlowControl {
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
            sent: AtomicU64::new(0),
            acknowledged: AtomicU64::new(0),
            acknowledgement: Notify::new(),
            finished: AtomicBool::new(false),
            completion: Notify::new(),
        }
    }

    pub fn acknowledge(&self, sequence: u64) -> Result<(), String> {
        if sequence > self.sent.load(Ordering::Acquire) {
            return Err("Acknowledgement exceeds the last delivered sequence".into());
        }
        self.acknowledged.fetch_max(sequence, Ordering::AcqRel);
        self.acknowledgement.notify_one();
        Ok(())
    }

    async fn wait_for_capacity(&self, timeout: Duration) -> Result<(), DeliveryError> {
        tokio::time::timeout(timeout, async {
            loop {
                let notified = self.acknowledgement.notified();
                if self
                    .sent
                    .load(Ordering::Acquire)
                    .saturating_sub(self.acknowledged.load(Ordering::Acquire))
                    < ACK_WINDOW
                {
                    return Ok(());
                }
                tokio::select! {
                    _ = self.cancel.cancelled() => return Err(DeliveryError::Cancelled),
                    _ = notified => {}
                }
            }
        })
        .await
        .map_err(|_| DeliveryError::Unresponsive)?
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum DeliveryError {
    Cancelled,
    Disconnected,
    Unresponsive,
}

pub(super) struct StreamWriter {
    request_id: String,
    channel: Channel<StreamEvent>,
    sequence: u64,
}

impl StreamWriter {
    pub fn new(request_id: String, channel: Channel<StreamEvent>) -> Self {
        Self {
            request_id,
            channel,
            sequence: 0,
        }
    }

    pub async fn send(
        &mut self,
        payload: StreamPayload,
        control: &FlowControl,
        timeout: Duration,
    ) -> Result<(), DeliveryError> {
        control.wait_for_capacity(timeout).await?;
        if control.cancel.is_cancelled() {
            return Err(DeliveryError::Cancelled);
        }
        self.deliver(payload, control)
    }

    // Terminal delivery has its own reserved slot so cancellation cannot wait on a frozen UI.
    pub fn terminal(&mut self, payload: StreamPayload, control: &FlowControl) {
        let _ = self.deliver(payload, control);
    }

    fn deliver(
        &mut self,
        payload: StreamPayload,
        control: &FlowControl,
    ) -> Result<(), DeliveryError> {
        self.sequence += 1;
        control.sent.store(self.sequence, Ordering::Release);
        self.channel
            .send(StreamEvent {
                request_id: self.request_id.clone(),
                sequence: self.sequence,
                payload,
            })
            .map_err(|_| DeliveryError::Disconnected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capacity_requires_ack_and_rejects_future_ack() {
        let flow = FlowControl::new();
        flow.sent.store(ACK_WINDOW, Ordering::Release);
        assert!(flow.acknowledge(ACK_WINDOW + 1).is_err());
        assert_eq!(
            flow.wait_for_capacity(Duration::from_millis(10)).await,
            Err(DeliveryError::Unresponsive)
        );
        flow.acknowledge(64).unwrap();
        flow.wait_for_capacity(Duration::from_millis(10))
            .await
            .unwrap();
        flow.acknowledge(2).unwrap();
        assert_eq!(flow.acknowledged.load(Ordering::Acquire), 64);
    }

    #[tokio::test]
    async fn cancelling_unblocks_a_backpressured_stream() {
        let flow = FlowControl::new();
        flow.sent.store(ACK_WINDOW, Ordering::Release);
        flow.cancel.cancel();
        assert_eq!(
            flow.wait_for_capacity(Duration::from_secs(60)).await,
            Err(DeliveryError::Cancelled)
        );
    }

    #[test]
    fn wire_schema_is_camel_case_with_lowercase_kind() {
        let event = StreamEvent {
            request_id: "request".into(),
            sequence: 1,
            payload: StreamPayload::Chunk {
                text: "hello".into(),
                session_id: Some("ses_1".into()),
            },
        };
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["requestId"], "request");
        assert_eq!(json["kind"], "chunk");
        assert_eq!(json["sessionId"], "ses_1");
    }

    #[tokio::test]
    async fn terminal_has_a_reserved_slot_and_delivery_stays_ordered() {
        use std::sync::{Arc, Mutex};
        use tauri::ipc::InvokeResponseBody;
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                captured
                    .lock()
                    .unwrap()
                    .push(serde_json::from_str::<serde_json::Value>(&json).unwrap());
            }
            Ok(())
        });
        let mut writer = StreamWriter::new("request".into(), channel);
        let flow = FlowControl::new();
        for _ in 0..ACK_WINDOW {
            writer
                .send(StreamPayload::Heartbeat, &flow, Duration::from_millis(10))
                .await
                .unwrap();
        }
        assert_eq!(
            writer
                .send(StreamPayload::Heartbeat, &flow, Duration::from_millis(10))
                .await,
            Err(DeliveryError::Unresponsive)
        );
        writer.terminal(StreamPayload::Cancelled, &flow);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), ACK_WINDOW as usize + 1);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event["sequence"], (index + 1) as u64);
        }
        assert_eq!(events.last().unwrap()["kind"], "cancelled");
    }

    #[tokio::test]
    async fn channel_failure_is_detected_without_waiting_for_ack_timeout() {
        let channel = Channel::new(|_| {
            Err(tauri::Error::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            )))
        });
        let mut writer = StreamWriter::new("request".into(), channel);
        assert_eq!(
            writer
                .send(
                    StreamPayload::Heartbeat,
                    &FlowControl::new(),
                    Duration::from_secs(30)
                )
                .await,
            Err(DeliveryError::Disconnected)
        );
    }
}
