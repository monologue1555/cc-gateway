//! Sanitized, in-memory diagnostics for CC Gateway request paths.

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Instant,
};

const MAX_RECENT_DIAGNOSTICS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayProtocol {
    Anthropic,
    Responses,
    Chat,
}

impl GatewayProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Responses => "responses",
            Self::Chat => "chat",
        }
    }
}

/// Incremental SSE completion detector. It only retains the current line, so
/// model output is never accumulated in the diagnostic history.
pub struct StreamCompletionDetector {
    protocol: GatewayProtocol,
    line: Vec<u8>,
    complete: bool,
}

impl StreamCompletionDetector {
    pub fn new(protocol: GatewayProtocol) -> Self {
        Self {
            protocol,
            line: Vec::new(),
            complete: false,
        }
    }

    pub fn observe(&mut self, chunk: &[u8]) {
        for byte in chunk {
            if *byte == b'\n' {
                self.inspect_current_line();
                self.line.clear();
            } else {
                self.line.push(*byte);
            }
        }
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    fn inspect_current_line(&mut self) {
        let line = String::from_utf8_lossy(&self.line);
        let line = line.trim_end_matches('\r').trim();
        let event = line
            .strip_prefix("event:")
            .map(str::trim)
            .filter(|event| !event.is_empty());
        let data = line.strip_prefix("data:").map(str::trim);
        let data_json = data.and_then(|data| serde_json::from_str::<serde_json::Value>(data).ok());
        let data_type = data_json
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(serde_json::Value::as_str);

        self.complete |= match self.protocol {
            GatewayProtocol::Anthropic => {
                event == Some("message_stop") || data_type == Some("message_stop")
            }
            GatewayProtocol::Responses => {
                matches!(event, Some("response.completed") | Some("response.done"))
                    || matches!(
                        data_type,
                        Some("response.completed") | Some("response.done")
                    )
            }
            GatewayProtocol::Chat => {
                data == Some("[DONE]")
                    || data_json
                        .as_ref()
                        .and_then(|value| value.get("choices"))
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|choices| {
                            choices.iter().any(|choice| {
                                choice
                                    .get("finish_reason")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some_and(|reason| !reason.is_empty())
                            })
                        })
            }
        }
    }
}

/// A deliberately small diagnostic record. Request and response content, HTTP
/// headers, and credentials have no representation in this type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayDiagnosticEntry {
    pub request_id: String,
    pub protocol: String,
    pub requested_model: String,
    pub upstream_model: String,
    pub provider: String,
    pub status: String,
    pub first_byte_ms: Option<u64>,
    pub end_reason: String,
    pub timestamp: String,
}

impl GatewayDiagnosticEntry {
    #[cfg(test)]
    fn test_entry(request_id: String) -> Self {
        Self {
            request_id,
            protocol: "anthropic".to_string(),
            requested_model: "client-model".to_string(),
            upstream_model: "upstream-model".to_string(),
            provider: "AnyRouter".to_string(),
            status: "success".to_string(),
            first_byte_ms: Some(1),
            end_reason: "completed".to_string(),
            timestamp: "2026-07-18T00:00:00Z".to_string(),
        }
    }
}

#[derive(Default)]
pub struct GatewayDiagnostics {
    entries: Mutex<VecDeque<GatewayDiagnosticEntry>>,
}

impl GatewayDiagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, entry: GatewayDiagnosticEntry) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.len() == MAX_RECENT_DIAGNOSTICS {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub fn snapshot(&self) -> Vec<GatewayDiagnosticEntry> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

pub(crate) fn gateway_diagnostics_store() -> Arc<GatewayDiagnostics> {
    static DIAGNOSTICS: OnceLock<Arc<GatewayDiagnostics>> = OnceLock::new();
    DIAGNOSTICS
        .get_or_init(|| Arc::new(GatewayDiagnostics::new()))
        .clone()
}

pub fn recent_gateway_diagnostics() -> Vec<GatewayDiagnosticEntry> {
    gateway_diagnostics_store().snapshot()
}

#[derive(Clone)]
pub struct GatewayRequestDiagnostic {
    store: Arc<GatewayDiagnostics>,
    request_id: String,
    protocol: GatewayProtocol,
    requested_model: String,
    upstream_model: String,
    provider: String,
    started_at: Instant,
    recorded: Arc<AtomicBool>,
}

impl GatewayRequestDiagnostic {
    pub fn new(
        store: Arc<GatewayDiagnostics>,
        protocol: GatewayProtocol,
        requested_model: impl Into<String>,
        upstream_model: impl Into<String>,
        provider: impl Into<String>,
        started_at: Instant,
    ) -> Self {
        Self {
            store,
            request_id: uuid::Uuid::new_v4().to_string(),
            protocol,
            requested_model: requested_model.into(),
            upstream_model: upstream_model.into(),
            provider: provider.into(),
            started_at,
            recorded: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn protocol(&self) -> GatewayProtocol {
        self.protocol
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    pub fn record_terminal(
        &self,
        success: bool,
        first_byte_ms: Option<u64>,
        end_reason: impl Into<String>,
    ) {
        if self
            .recorded
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        self.store.record(GatewayDiagnosticEntry {
            request_id: self.request_id.clone(),
            protocol: self.protocol.as_str().to_string(),
            requested_model: self.requested_model.clone(),
            upstream_model: self.upstream_model.clone(),
            provider: self.provider.clone(),
            status: if success { "success" } else { "error" }.to_string(),
            first_byte_ms,
            end_reason: end_reason.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }
}

struct StreamDiagnosticGuard {
    diagnostic: GatewayRequestDiagnostic,
    http_success: bool,
    first_byte_ms: Option<u64>,
    finished: bool,
}

impl StreamDiagnosticGuard {
    fn new(diagnostic: GatewayRequestDiagnostic, http_success: bool) -> Self {
        Self {
            diagnostic,
            http_success,
            first_byte_ms: None,
            finished: false,
        }
    }

    fn observe_first_byte(&mut self) {
        if self.first_byte_ms.is_none() {
            self.first_byte_ms = Some(self.diagnostic.elapsed_ms());
        }
    }

    fn finish(&mut self, end_reason: &'static str) {
        if self.finished {
            return;
        }
        self.diagnostic.record_terminal(
            self.http_success && end_reason == "completed",
            self.first_byte_ms,
            end_reason,
        );
        self.finished = true;
    }
}

impl Drop for StreamDiagnosticGuard {
    fn drop(&mut self) {
        if !self.finished {
            let reason = if self.http_success {
                "stream_incomplete"
            } else {
                "http_error"
            };
            self.finish(reason);
        }
    }
}

/// Observe the client-facing SSE stream without retaining its contents. A
/// success is recorded only after the selected protocol's terminal marker.
pub fn observe_gateway_stream<S>(
    stream: S,
    diagnostic: GatewayRequestDiagnostic,
    http_success: bool,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    async_stream::stream! {
        let mut detector = StreamCompletionDetector::new(diagnostic.protocol());
        let mut guard = StreamDiagnosticGuard::new(diagnostic, http_success);
        tokio::pin!(stream);

        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    guard.observe_first_byte();
                    detector.observe(&bytes);
                    if detector.is_complete() {
                        guard.finish(if http_success { "completed" } else { "http_error" });
                    }
                    yield Ok(bytes);
                }
                Err(error) => {
                    guard.finish("stream_error");
                    yield Err(error);
                    break;
                }
            }
        }

        if !detector.is_complete() {
            guard.finish(if http_success { "stream_incomplete" } else { "http_error" });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_diagnostics_keep_only_the_latest_fifty_entries() {
        let store = GatewayDiagnostics::new();

        for index in 0..51 {
            store.record(GatewayDiagnosticEntry::test_entry(format!(
                "request-{index}"
            )));
        }

        let entries = store.snapshot();
        assert_eq!(entries.len(), 50);
        assert_eq!(entries.first().unwrap().request_id, "request-1");
        assert_eq!(entries.last().unwrap().request_id, "request-50");
    }

    #[test]
    fn anthropic_completion_marker_is_detected_across_chunk_boundaries() {
        let mut detector = StreamCompletionDetector::new(GatewayProtocol::Anthropic);

        detector.observe(b"event: message_st");
        assert!(!detector.is_complete());
        detector.observe(b"op\ndata: {\"type\":\"message_stop\"}\n\n");

        assert!(detector.is_complete());
    }

    #[test]
    fn responses_accepts_completed_and_done_markers() {
        for marker in ["response.completed", "response.done"] {
            let mut detector = StreamCompletionDetector::new(GatewayProtocol::Responses);
            let payload = format!("event: {marker}\ndata: {{\"type\":\"{marker}\"}}\n\n");

            detector.observe(&payload.as_bytes()[..11]);
            detector.observe(&payload.as_bytes()[11..]);

            assert!(detector.is_complete(), "marker {marker} was not detected");
        }
    }

    #[test]
    fn chat_accepts_done_or_non_empty_finish_reason() {
        let mut done = StreamCompletionDetector::new(GatewayProtocol::Chat);
        done.observe(b"data: [DO");
        done.observe(b"NE]\n\n");
        assert!(done.is_complete());

        let mut finish_reason = StreamCompletionDetector::new(GatewayProtocol::Chat);
        finish_reason
            .observe(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n");
        assert!(finish_reason.is_complete());
    }

    #[tokio::test]
    async fn eof_without_protocol_completion_marker_is_stream_incomplete() {
        use bytes::Bytes;
        use futures::StreamExt;
        use std::{sync::Arc, time::Instant};

        let store = Arc::new(GatewayDiagnostics::new());
        let diagnostic = GatewayRequestDiagnostic::new(
            store.clone(),
            GatewayProtocol::Anthropic,
            "client-model",
            "upstream-model",
            "AnyRouter",
            Instant::now(),
        );
        let upstream = futures::stream::iter([Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n",
        ))]);

        let received = observe_gateway_stream(upstream, diagnostic, true)
            .collect::<Vec<_>>()
            .await;

        assert_eq!(received.len(), 1);
        let entries = store.snapshot();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "error");
        assert_eq!(entries[0].end_reason, "stream_incomplete");
        assert!(entries[0].first_byte_ms.is_some());
    }

    #[test]
    fn serialized_diagnostic_has_only_the_sanitized_schema() {
        let mut entry = GatewayDiagnosticEntry::test_entry("request-1".to_string());
        entry.provider = "provider-without-credentials".to_string();
        let value = serde_json::to_value(entry).unwrap();
        let object = value.as_object().unwrap();
        let keys = object
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            keys,
            [
                "endReason",
                "firstByteMs",
                "protocol",
                "provider",
                "requestId",
                "requestedModel",
                "status",
                "timestamp",
                "upstreamModel",
            ]
            .into_iter()
            .collect()
        );
        let encoded = value.to_string();
        for forbidden in [
            "prompt",
            "body",
            "header",
            "authorization",
            "apiKey",
            "sk-secret",
        ] {
            assert!(
                !encoded.contains(forbidden),
                "serialized forbidden field/value: {forbidden}"
            );
        }
    }
}
