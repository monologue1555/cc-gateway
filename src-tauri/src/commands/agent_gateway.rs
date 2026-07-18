//! Thin Tauri adapters for Agent Gateway configuration.

use crate::agent_gateway::{
    self, AgentGatewayConfigInput, AgentGatewayState, AgentGatewayTestInput,
    AgentGatewayTestResult, AgentGatewayTokenResult,
};
use crate::store::AppState;
use serde_json::json;
use std::time::{Duration, Instant};
use tauri::State;

#[tauri::command]
pub fn get_claude_connection_profile(
    state: State<'_, AppState>,
) -> Result<crate::agent_gateway::connection_profile::ClaudeConnectionProfileState, String> {
    crate::agent_gateway::connection_profile_commands::get_claude_connection_profile(state)
}

#[tauri::command]
pub fn update_claude_connection_profile(
    state: State<'_, AppState>,
    input: crate::agent_gateway::connection_profile::ClaudeConnectionProfileInput,
) -> Result<crate::agent_gateway::connection_profile::ClaudeConnectionProfileState, String> {
    crate::agent_gateway::connection_profile_commands::update_claude_connection_profile(
        state, input,
    )
}

#[tauri::command]
pub async fn test_claude_connection_model(
    state: State<'_, AppState>,
    input: crate::agent_gateway::connection_profile_commands::ClaudeConnectionTestInput,
) -> Result<crate::agent_gateway::connection_profile::ClaudeModelTestResult, String> {
    crate::agent_gateway::connection_profile_commands::test_claude_connection_model(state, input)
        .await
}

#[tauri::command]
pub async fn test_all_claude_connection_models(
    state: State<'_, AppState>,
) -> Result<crate::agent_gateway::connection_profile::ClaudeConnectionTestSummary, String> {
    crate::agent_gateway::connection_profile_commands::test_all_claude_connection_models(state)
        .await
}

async fn resolved_listen_port(state: &AppState) -> Result<u16, String> {
    let status = state.proxy_service.get_status().await?;
    if status.running && status.port != 0 {
        return Ok(status.port);
    }

    let config = state.proxy_service.get_config().await?;
    Ok(config.listen_port)
}

async fn ensure_loopback_listener(state: &AppState) -> Result<(), String> {
    let status = state.proxy_service.get_status().await?;
    let address = if status.running && !status.address.trim().is_empty() {
        status.address
    } else {
        state.proxy_service.get_config().await?.listen_address
    };
    if agent_gateway::listen_address_is_safe(&address) {
        Ok(())
    } else {
        Err(format!(
            "Agent Gateway 仅允许 localhost；当前共享代理监听 {address}。请先在 CC Gateway 中把监听地址改为 127.0.0.1。"
        ))
    }
}

#[tauri::command]
pub async fn get_agent_gateway_state(
    state: State<'_, AppState>,
) -> Result<AgentGatewayState, String> {
    let listen_port = resolved_listen_port(&state).await?;
    agent_gateway::build_state(state.db.as_ref(), listen_port).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn update_agent_gateway_config(
    state: State<'_, AppState>,
    input: AgentGatewayConfigInput,
) -> Result<AgentGatewayState, String> {
    let previous = agent_gateway::load_agent_gateway_config(state.db.as_ref())
        .map_err(|error| error.to_string())?;
    if input.enabled {
        ensure_loopback_listener(&state).await?;
        if !previous.enabled {
            state
                .proxy_service
                .acquire_claude_consumer(crate::claude_runtime::RuntimeConsumer::Backend)
                .await?;
        }
        if let Err(error) = agent_gateway::update_config(state.db.as_ref(), input) {
            if !previous.enabled {
                let _ = state
                    .proxy_service
                    .release_claude_consumer(crate::claude_runtime::RuntimeConsumer::Backend)
                    .await;
            }
            return Err(error.to_string());
        }
    } else {
        agent_gateway::update_config(state.db.as_ref(), input)
            .map_err(|error| error.to_string())?;
        if previous.enabled {
            state
                .proxy_service
                .release_claude_consumer(crate::claude_runtime::RuntimeConsumer::Backend)
                .await?;
        }
    }
    let listen_port = resolved_listen_port(&state).await?;
    agent_gateway::build_state(state.db.as_ref(), listen_port).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_claude_runtime_status(
    state: State<'_, AppState>,
) -> Result<crate::claude_runtime::RuntimeStatus, String> {
    Ok(state.proxy_service.claude_runtime_status().await)
}

#[tauri::command]
pub fn regenerate_agent_gateway_token(
    state: State<'_, AppState>,
) -> Result<AgentGatewayTokenResult, String> {
    agent_gateway::regenerate_token(state.db.as_ref()).map_err(|error| error.to_string())
}

fn test_request(protocol: &str, model: &str) -> Option<(&'static str, serde_json::Value)> {
    match protocol {
        "anthropic" => Some((
            "/agent/v1/messages",
            json!({
                "model": model,
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "Reply with OK."}]
            }),
        )),
        "responses" => Some((
            "/agent/v1/responses",
            json!({
                "model": model,
                "max_output_tokens": 16,
                "input": "Reply with OK."
            }),
        )),
        "chat" => Some((
            "/agent/v1/chat/completions",
            json!({
                "model": model,
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "Reply with OK."}]
            }),
        )),
        _ => None,
    }
}

fn bounded_message(message: &str) -> String {
    const MAX_CHARS: usize = 500;
    let mut chars = message.chars();
    let bounded = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

#[tauri::command]
pub async fn test_agent_gateway_protocol(
    state: State<'_, AppState>,
    input: AgentGatewayTestInput,
) -> Result<AgentGatewayTestResult, String> {
    let protocol = input.protocol.trim().to_ascii_lowercase();
    let model = input.model.trim().to_string();
    let Some((path, body)) = test_request(&protocol, &model) else {
        return Err("protocol 必须是 anthropic、responses 或 chat".to_string());
    };

    let config = agent_gateway::load_agent_gateway_config(state.db.as_ref())
        .map_err(|error| error.to_string())?;
    if !config.enabled {
        return Ok(AgentGatewayTestResult {
            success: false,
            protocol,
            latency_ms: 0,
            model,
            message: Some("Agent Gateway 尚未启用".to_string()),
        });
    }

    let server = state.proxy_service.start().await?;
    let url = format!("http://127.0.0.1:{}{path}", server.port);
    let started = Instant::now();
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|error| error.to_string())?
        .post(url)
        .bearer_auth(&config.token)
        .json(&body)
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;

    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Ok(AgentGatewayTestResult {
                success: status.is_success(),
                protocol,
                latency_ms,
                model,
                message: Some(if status.is_success() {
                    "连接与最小模型请求成功".to_string()
                } else {
                    bounded_message(&format!("HTTP {}: {}", status.as_u16(), body))
                }),
            })
        }
        Err(error) => Ok(AgentGatewayTestResult {
            success: false,
            protocol,
            latency_ms,
            model,
            message: Some(bounded_message(&error.to_string())),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payloads_use_the_public_model_unchanged() {
        for protocol in ["anthropic", "responses", "chat"] {
            let (_, body) =
                test_request(protocol, "claude-fable-5[1M]").expect("supported test protocol");
            assert_eq!(body["model"], "claude-fable-5[1M]");
        }
    }

    #[test]
    fn bounded_error_message_does_not_split_unicode() {
        let long = "错".repeat(600);
        let bounded = bounded_message(&long);
        assert!(bounded.ends_with('…'));
        assert_eq!(bounded.chars().count(), 501);
    }
}
