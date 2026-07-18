//! Shared lifecycle state for the Claude Code, Claude Desktop, and local
//! backend consumers of CC Gateway's single loopback listener.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;

pub const DEFAULT_LISTEN_ADDRESS: &str = "127.0.0.1";
pub const DEFAULT_LISTEN_PORT: u16 = 15722;
const HEALTH_SERVICE_ID: &str = "cc-gateway";
const CLAUDE_CODE_GATEWAY_TOKEN_SETTING_KEY: &str = "claude_code_gateway_token";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GatewayPhase {
    Disabled,
    Starting,
    Ready,
    Degraded,
    Stopping,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeConsumer {
    ClaudeCode,
    ClaudeDesktop,
    Backend,
}

#[derive(Debug, Clone)]
pub struct RuntimeRegistry {
    phase: GatewayPhase,
    consumers: BTreeSet<RuntimeConsumer>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeCheckpoint {
    phase: GatewayPhase,
    consumers: BTreeSet<RuntimeConsumer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub phase: GatewayPhase,
    pub consumers: Vec<RuntimeConsumer>,
    pub last_error: Option<String>,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self {
            phase: GatewayPhase::Disabled,
            consumers: BTreeSet::new(),
            last_error: None,
        }
    }
}

impl RuntimeRegistry {
    pub fn phase(&self) -> GatewayPhase {
        self.phase
    }

    pub fn consumers(&self) -> Vec<RuntimeConsumer> {
        self.consumers.iter().copied().collect()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            phase: self.phase,
            consumers: self.consumers(),
            last_error: self.last_error.clone(),
        }
    }

    pub fn begin_enable(&mut self, consumer: RuntimeConsumer) -> RuntimeCheckpoint {
        let checkpoint = RuntimeCheckpoint {
            phase: self.phase,
            consumers: self.consumers.clone(),
        };
        let listener_was_unused = self.consumers.is_empty();
        self.consumers.insert(consumer);
        if listener_was_unused
            || matches!(self.phase, GatewayPhase::Disabled | GatewayPhase::Degraded)
        {
            self.phase = GatewayPhase::Starting;
        }
        checkpoint
    }

    pub fn mark_ready(&mut self) {
        if !self.consumers.is_empty() {
            self.phase = GatewayPhase::Ready;
            self.last_error = None;
        }
    }

    pub fn rollback_enable(&mut self, checkpoint: RuntimeCheckpoint, error: impl Into<String>) {
        self.phase = checkpoint.phase;
        self.consumers = checkpoint.consumers;
        self.last_error = Some(error.into());
        if self.consumers.is_empty() {
            self.phase = GatewayPhase::Disabled;
        } else if matches!(self.phase, GatewayPhase::Starting | GatewayPhase::Stopping) {
            self.phase = GatewayPhase::Ready;
        }
    }

    pub fn mark_degraded(&mut self, error: impl Into<String>) {
        // A stop failure can leave the listener alive after the final logical
        // consumer was removed. Report that honestly instead of presenting a
        // false Disabled state while the port is still bound.
        self.phase = GatewayPhase::Degraded;
        self.last_error = Some(error.into());
    }

    /// Returns true when the caller must stop the shared listener.
    pub fn begin_disable(&mut self, consumer: RuntimeConsumer) -> bool {
        if !self.consumers.remove(&consumer) {
            return false;
        }
        if self.consumers.is_empty() {
            self.phase = GatewayPhase::Stopping;
            true
        } else {
            self.phase = GatewayPhase::Ready;
            false
        }
    }

    pub fn mark_stopped(&mut self) {
        if self.consumers.is_empty() {
            self.phase = GatewayPhase::Disabled;
        }
    }
}

pub async fn probe_listener(address: &str, port: u16, timeout: Duration) -> Result<(), String> {
    if address != DEFAULT_LISTEN_ADDRESS || port == 0 {
        return Err(format!(
            "invalid CC Gateway listener address: {address}:{port}"
        ));
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("readiness client error: {error}"))?;
    let response = client
        .get(format!("http://{address}:{port}/health"))
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                format!("readiness timeout: {error}")
            } else {
                format!("readiness connection failed: {error}")
            }
        })?;
    if !response.status().is_success() {
        return Err(format!(
            "readiness returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("readiness protocol error: {error}"))?;
    if body.get("service").and_then(serde_json::Value::as_str) != Some(HEALTH_SERVICE_ID) {
        return Err("readiness identity mismatch: port is owned by another service".to_string());
    }
    if body.get("status").and_then(serde_json::Value::as_str) != Some("healthy") {
        return Err("readiness health status is not healthy".to_string());
    }
    Ok(())
}

pub fn get_or_create_claude_code_token(
    db: &crate::database::Database,
) -> Result<String, crate::error::AppError> {
    if let Some(token) = db.get_setting(CLAUDE_CODE_GATEWAY_TOKEN_SETTING_KEY)? {
        let token = token.trim();
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }
    let token = format!("ccg-code-{}", uuid::Uuid::new_v4().simple());
    db.set_setting(CLAUDE_CODE_GATEWAY_TOKEN_SETTING_KEY, &token)?;
    Ok(token)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..max_len {
        difference |= (left.get(index).copied().unwrap_or(0)
            ^ right.get(index).copied().unwrap_or(0)) as usize;
    }
    difference == 0
}

pub fn claude_code_token_matches(expected: &str, candidate: &str) -> bool {
    let expected = expected.trim().as_bytes();
    let candidate = candidate.trim().as_bytes();
    !expected.is_empty() && constant_time_eq(expected, candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use serde_json::{json, Value};
    use std::time::Duration;

    #[test]
    fn listener_stops_only_after_the_last_consumer_is_released() {
        let mut runtime = RuntimeRegistry::default();

        runtime.begin_enable(RuntimeConsumer::ClaudeCode);
        runtime.mark_ready();
        runtime.begin_enable(RuntimeConsumer::ClaudeDesktop);
        runtime.mark_ready();
        runtime.begin_enable(RuntimeConsumer::Backend);
        runtime.mark_ready();

        assert!(!runtime.begin_disable(RuntimeConsumer::ClaudeCode));
        assert_eq!(runtime.phase(), GatewayPhase::Ready);
        assert!(!runtime.begin_disable(RuntimeConsumer::ClaudeDesktop));
        assert_eq!(runtime.phase(), GatewayPhase::Ready);
        assert!(runtime.begin_disable(RuntimeConsumer::Backend));
        assert_eq!(runtime.phase(), GatewayPhase::Stopping);

        runtime.mark_stopped();
        assert_eq!(runtime.phase(), GatewayPhase::Disabled);
    }

    #[test]
    fn failed_consumer_activation_restores_the_previous_runtime_state() {
        let mut runtime = RuntimeRegistry::default();
        let first = runtime.begin_enable(RuntimeConsumer::ClaudeCode);
        runtime.mark_ready();

        let desktop = runtime.begin_enable(RuntimeConsumer::ClaudeDesktop);
        runtime.rollback_enable(desktop, "desktop config write failed");

        assert_eq!(runtime.phase(), GatewayPhase::Ready);
        assert_eq!(runtime.consumers(), vec![RuntimeConsumer::ClaudeCode]);
        assert_eq!(runtime.last_error(), Some("desktop config write failed"));

        runtime.begin_disable(RuntimeConsumer::ClaudeCode);
        runtime.mark_stopped();
        let backend = runtime.begin_enable(RuntimeConsumer::Backend);
        runtime.rollback_enable(backend, "port already in use");

        assert_eq!(runtime.phase(), GatewayPhase::Disabled);
        assert!(runtime.consumers().is_empty());
        assert_eq!(runtime.last_error(), Some("port already in use"));

        // Keep the first checkpoint live in the test so accidental non-copy
        // checkpoint implementations cannot borrow the registry indefinitely.
        let _ = first;
    }

    #[test]
    fn failed_listener_stop_remains_visible_after_last_consumer_is_removed() {
        let mut runtime = RuntimeRegistry::default();
        runtime.begin_enable(RuntimeConsumer::Backend);
        runtime.mark_ready();

        assert!(runtime.begin_disable(RuntimeConsumer::Backend));
        runtime.mark_degraded("listener stop failed");

        assert_eq!(runtime.phase(), GatewayPhase::Degraded);
        assert!(runtime.consumers().is_empty());
        assert_eq!(runtime.last_error(), Some("listener stop failed"));
    }

    #[tokio::test]
    async fn readiness_probe_rejects_an_unrelated_service_on_the_expected_port() {
        async fn start_health(payload: Value) -> (u16, tokio::task::JoinHandle<()>) {
            let app = Router::new().route(
                "/health",
                get(move || {
                    let payload = payload.clone();
                    async move { Json(payload) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            (port, handle)
        }

        let (wrong_port, wrong_server) = start_health(json!({"status": "healthy"})).await;
        let error = probe_listener("127.0.0.1", wrong_port, Duration::from_secs(1))
            .await
            .expect_err("another service must not satisfy readiness");
        assert!(error.contains("identity"), "{error}");
        wrong_server.abort();

        let (gateway_port, gateway_server) = start_health(json!({
            "status": "healthy",
            "service": "cc-gateway"
        }))
        .await;
        probe_listener("127.0.0.1", gateway_port, Duration::from_secs(1))
            .await
            .expect("CC Gateway health must satisfy readiness");
        gateway_server.abort();
    }
}
