//! Tauri command adapters for the canonical Claude connection profile.

use super::connection_profile::{
    self, ClaudeConnectionProfileInput, ClaudeConnectionProfileState, ClaudeConnectionTestSummary,
    ClaudeModelRole, ClaudeModelTestResult,
};
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeConnectionTestInput {
    pub role: ClaudeModelRole,
}

pub fn get_claude_connection_profile(
    state: State<'_, AppState>,
) -> Result<ClaudeConnectionProfileState, String> {
    connection_profile::get_profile_state(state.db.as_ref()).map_err(String::from)
}

pub fn update_claude_connection_profile(
    state: State<'_, AppState>,
    input: ClaudeConnectionProfileInput,
) -> Result<ClaudeConnectionProfileState, String> {
    connection_profile::update_profile(state.db.as_ref(), input).map_err(String::from)
}

pub async fn test_claude_connection_model(
    state: State<'_, AppState>,
    input: ClaudeConnectionTestInput,
) -> Result<ClaudeModelTestResult, String> {
    connection_profile::test_profile_model(state.db.as_ref(), input.role)
        .await
        .map_err(String::from)
}

pub async fn test_all_claude_connection_models(
    state: State<'_, AppState>,
) -> Result<ClaudeConnectionTestSummary, String> {
    connection_profile::test_all_profile_models(state.db.as_ref())
        .await
        .map_err(String::from)
}
