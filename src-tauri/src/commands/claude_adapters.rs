//! Public adapters that project one canonical Claude profile into Claude Code
//! and Claude Desktop while sharing the same verified loopback listener.

use crate::claude_runtime::RuntimeStatus;
use crate::store::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAdapterState {
    pub claude_code_enabled: bool,
    pub claude_desktop_enabled: bool,
    pub backend_enabled: bool,
    pub runtime: RuntimeStatus,
}

async fn build_state(state: &AppState) -> Result<ClaudeAdapterState, String> {
    let claude_code_enabled = state
        .db
        .get_proxy_config_for_app("claude")
        .await
        .map_err(|error| error.to_string())?
        .enabled;
    let claude_desktop_enabled = state
        .db
        .get_proxy_config_for_app("claude-desktop")
        .await
        .map_err(|error| error.to_string())?
        .enabled;
    let backend_enabled = crate::agent_gateway::load_agent_gateway_config(state.db.as_ref())
        .map_err(|error| error.to_string())?
        .enabled;
    Ok(ClaudeAdapterState {
        claude_code_enabled,
        claude_desktop_enabled,
        backend_enabled,
        runtime: state.proxy_service.claude_runtime_status().await,
    })
}

#[tauri::command]
pub async fn get_claude_adapter_state(
    state: State<'_, AppState>,
) -> Result<ClaudeAdapterState, String> {
    build_state(&state).await
}

#[tauri::command]
pub async fn set_claude_code_adapter_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<ClaudeAdapterState, String> {
    state
        .proxy_service
        .set_takeover_for_app("claude", enabled)
        .await?;
    build_state(&state).await
}

#[tauri::command]
pub async fn set_claude_desktop_adapter_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<ClaudeAdapterState, String> {
    state
        .proxy_service
        .set_claude_desktop_consumer_enabled(enabled)
        .await?;
    build_state(&state).await
}
