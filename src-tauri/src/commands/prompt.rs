use indexmap::IndexMap;
use tauri::State;

use crate::product_scope::parse_public_app_type;
use crate::prompt::Prompt;
use crate::services::PromptService;
use crate::store::AppState;

#[tauri::command]
pub async fn get_prompts(
    app: String,
    state: State<'_, AppState>,
) -> Result<IndexMap<String, Prompt>, String> {
    let app_type = parse_public_app_type(&app)?;
    PromptService::get_prompts(&state, app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_prompt(
    app: String,
    id: String,
    prompt: Prompt,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app_type = parse_public_app_type(&app)?;
    PromptService::upsert_prompt(&state, app_type, &id, prompt).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_prompt(
    app: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app_type = parse_public_app_type(&app)?;
    PromptService::delete_prompt(&state, app_type, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn enable_prompt(
    app: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let app_type = parse_public_app_type(&app)?;
    PromptService::enable_prompt(&state, app_type, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_prompt_from_file(
    app: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let app_type = parse_public_app_type(&app)?;
    PromptService::import_from_file(&state, app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_prompt_file_content(app: String) -> Result<Option<String>, String> {
    let app_type = parse_public_app_type(&app)?;
    PromptService::get_current_file_content(app_type).map_err(|e| e.to_string())
}
