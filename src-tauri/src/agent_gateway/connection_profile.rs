//! Canonical Claude connection profile shared by Claude Code, Claude Desktop,
//! and the local backend gateway.

use crate::error::AppError;
use crate::provider::{
    ClaudeDesktopMode, ClaudeDesktopModelRoute, LocalProxyRequestOverrides, Provider, ProviderMeta,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

pub const CLAUDE_CONNECTION_PROFILE_SETTING_KEY: &str = "claude_connection_profile";
pub const DEFAULT_ANYROUTER_BASE_URL: &str = "https://anyrouter.top";
pub const CLAUDE_CODE_PROFILE_PROVIDER_ID: &str = "cc-gateway-anyrouter-claude";
pub const CLAUDE_DESKTOP_PROFILE_PROVIDER_ID: &str = "cc-gateway-anyrouter-desktop";
const DEFAULT_MODEL_TEST_TIMEOUT: Duration = Duration::from_secs(45);
const ANYROUTER_CLAUDE_CODE_USER_AGENT: &str = "claude-cli/2.1.209 (external, local-agent)";
const ANYROUTER_ANTHROPIC_BETA: &str = "claude-code-20250219,context-1m-2025-08-07,interleaved-thinking-2025-05-14,mid-conversation-system-2026-04-07,effort-2025-11-24";
const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeAuthMode {
    Bearer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeApiFormat {
    Anthropic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeModelRole {
    Opus,
    Fable,
    Sonnet,
    Haiku,
    Subagent,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeProfileConsumer {
    ClaudeCode,
    ClaudeDesktop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelRoute {
    pub role: ClaudeModelRole,
    pub display_name: String,
    pub client_model_id: String,
    pub upstream_model_id: String,
    #[serde(rename = "supports1m")]
    pub supports_1m: bool,
}

impl ClaudeModelRoute {
    pub fn client_model_with_capability(&self) -> String {
        if self.supports_1m {
            format!("{}[1M]", self.client_model_id)
        } else {
            self.client_model_id.clone()
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeConnectionProfile {
    #[serde(default)]
    pub enabled: bool,
    pub base_url: String,
    pub auth_mode: ClaudeAuthMode,
    pub api_format: ClaudeApiFormat,
    #[serde(default)]
    pub upstream_api_key: String,
    pub models: Vec<ClaudeModelRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_test: Option<ClaudeConnectionTestSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeConnectionProfileInput {
    pub enabled: bool,
    pub base_url: String,
    /// `None` preserves the canonical stored key; `Some` replaces it.
    pub api_key: Option<String>,
    pub models: Vec<ClaudeModelRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeConnectionProfileState {
    pub enabled: bool,
    pub base_url: String,
    pub auth_mode: ClaudeAuthMode,
    pub api_format: ClaudeApiFormat,
    pub has_api_key: bool,
    pub masked_api_key: String,
    pub models: Vec<ClaudeModelRoute>,
    pub last_test: Option<ClaudeConnectionTestSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeConnectionTestCategory {
    Success,
    Configuration,
    AuthenticationFailed,
    ModelUnavailable,
    ProtocolError,
    ProxyInterference,
    Timeout,
    NetworkError,
    UpstreamUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelTestResult {
    pub success: bool,
    pub role: ClaudeModelRole,
    pub requested_model: String,
    pub upstream_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    pub latency_ms: u64,
    pub category: ClaudeConnectionTestCategory,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeConnectionTestSummary {
    pub tested_at_ms: u64,
    pub success: bool,
    pub results: Vec<ClaudeModelTestResult>,
}

impl std::fmt::Debug for ClaudeConnectionProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClaudeConnectionProfile")
            .field("enabled", &self.enabled)
            .field("base_url", &self.base_url)
            .field("auth_mode", &self.auth_mode)
            .field("api_format", &self.api_format)
            .field("upstream_api_key", &"[REDACTED]")
            .field("models", &self.models)
            .field("last_test", &self.last_test)
            .finish()
    }
}

impl Default for ClaudeConnectionProfile {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: DEFAULT_ANYROUTER_BASE_URL.to_string(),
            auth_mode: ClaudeAuthMode::Bearer,
            api_format: ClaudeApiFormat::Anthropic,
            upstream_api_key: String::new(),
            models: vec![
                default_route(ClaudeModelRole::Opus, "Claude Opus 4.8", "claude-opus-4-8"),
                default_route(ClaudeModelRole::Fable, "Claude Fable 5", "claude-fable-5"),
                default_route(ClaudeModelRole::Sonnet, "Claude Fable 5", "claude-fable-5"),
                default_route(ClaudeModelRole::Haiku, "Claude Opus 4.6", "claude-opus-4-6"),
                default_route(
                    ClaudeModelRole::Subagent,
                    "Claude Opus 4.6",
                    "claude-opus-4-6",
                ),
                default_route(
                    ClaudeModelRole::Fallback,
                    "Claude Opus 4.6",
                    "claude-opus-4-6",
                ),
            ],
            last_test: None,
        }
    }
}

fn default_route(role: ClaudeModelRole, display_name: &str, model_id: &str) -> ClaudeModelRoute {
    ClaudeModelRoute {
        role,
        display_name: display_name.to_string(),
        client_model_id: model_id.to_string(),
        upstream_model_id: model_id.to_string(),
        supports_1m: true,
    }
}

impl ClaudeConnectionProfile {
    pub fn model_for_role(&self, role: ClaudeModelRole) -> Option<&ClaudeModelRoute> {
        self.models.iter().find(|model| model.role == role)
    }

    pub fn normalize_for_storage(mut self) -> Result<Self, AppError> {
        self.base_url = normalize_base_url(&self.base_url)?;

        let mut roles = HashSet::new();
        let mut client_routes = HashMap::<String, (String, bool)>::new();
        for model in &mut self.models {
            if !roles.insert(model.role) {
                return Err(AppError::InvalidInput(format!(
                    "模型角色重复: {:?}",
                    model.role
                )));
            }

            model.display_name = model.display_name.trim().to_string();
            let client_had_one_m = has_one_m_suffix(&model.client_model_id);
            model.client_model_id = strip_one_m_suffix(&model.client_model_id);
            model.upstream_model_id = strip_one_m_suffix(&model.upstream_model_id);
            model.supports_1m |= client_had_one_m;

            if model.display_name.is_empty()
                || model.client_model_id.is_empty()
                || model.upstream_model_id.is_empty()
            {
                return Err(AppError::InvalidInput(format!(
                    "模型角色 {:?} 的名称或模型 ID 不能为空",
                    model.role
                )));
            }

            let route_signature = (model.upstream_model_id.clone(), model.supports_1m);
            if let Some(existing) =
                client_routes.insert(model.client_model_id.clone(), route_signature.clone())
            {
                if existing != route_signature {
                    return Err(AppError::InvalidInput(format!(
                        "客户端模型 {} 被映射到多个上游模型",
                        model.client_model_id
                    )));
                }
            }
        }

        let required = [
            ClaudeModelRole::Opus,
            ClaudeModelRole::Fable,
            ClaudeModelRole::Sonnet,
            ClaudeModelRole::Haiku,
            ClaudeModelRole::Subagent,
            ClaudeModelRole::Fallback,
        ];
        let missing = required
            .into_iter()
            .filter(|role| !roles.contains(role))
            .map(|role| format!("{role:?}"))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "模型目录缺少角色: {}",
                missing.join("、")
            )));
        }

        Ok(self)
    }
}

fn normalize_base_url(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim().trim_end_matches('/');
    let parsed = Url::parse(trimmed)
        .map_err(|error| AppError::InvalidInput(format!("基础 URL 无效: {error}")))?;
    if parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(AppError::InvalidInput(
            "基础 URL 不能包含用户信息、查询参数或片段".to_string(),
        ));
    }
    let is_loopback = parsed
        .host_str()
        .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && is_loopback) {
        return Err(AppError::InvalidInput(
            "远程基础 URL 必须使用 HTTPS；HTTP 仅允许 localhost".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn has_one_m_suffix(model: &str) -> bool {
    let trimmed = model.trim_end();
    trimmed
        .as_bytes()
        .get(trimmed.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(b"[1m]"))
}

pub fn strip_one_m_suffix(model: &str) -> String {
    let trimmed = model.trim();
    if has_one_m_suffix(trimmed) {
        trimmed[..trimmed.len() - 4].trim_end().to_string()
    } else {
        trimmed.to_string()
    }
}

fn save_profile(
    db: &crate::database::Database,
    profile: &ClaudeConnectionProfile,
) -> Result<(), AppError> {
    let raw = serde_json::to_string(profile)
        .map_err(|error| AppError::Config(format!("Claude 连接配置序列化失败: {error}")))?;
    db.set_setting(CLAUDE_CONNECTION_PROFILE_SETTING_KEY, &raw)
}

pub fn load_profile(db: &crate::database::Database) -> Result<ClaudeConnectionProfile, AppError> {
    let Some(raw) = db.get_setting(CLAUDE_CONNECTION_PROFILE_SETTING_KEY)? else {
        let profile = ClaudeConnectionProfile::default();
        save_profile(db, &profile)?;
        return Ok(profile);
    };
    let profile: ClaudeConnectionProfile = serde_json::from_str(&raw)
        .map_err(|error| AppError::Config(format!("Claude 连接配置 JSON 无效: {error}")))?;
    let normalized = profile.clone().normalize_for_storage()?;
    if normalized != profile {
        save_profile(db, &normalized)?;
    }
    Ok(normalized)
}

pub fn build_state(profile: &ClaudeConnectionProfile) -> ClaudeConnectionProfileState {
    let api_key = profile.upstream_api_key.trim();
    ClaudeConnectionProfileState {
        enabled: profile.enabled,
        base_url: profile.base_url.clone(),
        auth_mode: profile.auth_mode,
        api_format: profile.api_format,
        has_api_key: !api_key.is_empty(),
        masked_api_key: mask_secret(api_key),
        models: profile.models.clone(),
        last_test: profile.last_test.clone(),
    }
}

pub fn get_profile_state(
    db: &crate::database::Database,
) -> Result<ClaudeConnectionProfileState, AppError> {
    load_profile(db).map(|profile| build_state(&profile))
}

pub fn update_profile(
    db: &crate::database::Database,
    input: ClaudeConnectionProfileInput,
) -> Result<ClaudeConnectionProfileState, AppError> {
    let existing = load_profile(db)?;
    let upstream_api_key = input
        .api_key
        .map(|key| key.trim().to_string())
        .unwrap_or(existing.upstream_api_key);
    if input.enabled && upstream_api_key.is_empty() {
        return Err(AppError::InvalidInput(
            "启用 Claude 连接前必须填写 AnyRouter API Key".to_string(),
        ));
    }

    let profile = ClaudeConnectionProfile {
        enabled: input.enabled,
        base_url: input.base_url,
        auth_mode: ClaudeAuthMode::Bearer,
        api_format: ClaudeApiFormat::Anthropic,
        upstream_api_key,
        models: input.models,
        last_test: None,
    }
    .normalize_for_storage()?;
    save_profile(db, &profile)?;
    Ok(build_state(&profile))
}

fn mask_secret(secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return String::new();
    }
    if !secret.is_ascii() || secret.len() <= 8 {
        return "***".to_string();
    }
    format!("{}...{}", &secret[..4], &secret[secret.len() - 4..])
}

fn projected_provider(
    profile: &ClaudeConnectionProfile,
    provider_id: &str,
) -> Result<Provider, AppError> {
    let profile = profile.clone().normalize_for_storage()?;
    if profile.upstream_api_key.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "AnyRouter API Key 尚未配置".to_string(),
        ));
    }

    let mut env = Map::<String, Value>::new();
    env.insert("ANTHROPIC_BASE_URL".to_string(), json!(profile.base_url));
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        json!(profile.upstream_api_key),
    );
    env.insert("AUTH_MODE".to_string(), json!("bearer_only"));

    for model in &profile.models {
        let upstream_for_client = if model.supports_1m {
            format!("{}[1M]", model.upstream_model_id)
        } else {
            model.upstream_model_id.clone()
        };
        match model.role {
            ClaudeModelRole::Opus => insert_role_env(
                &mut env,
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
                &upstream_for_client,
                &model.display_name,
            ),
            ClaudeModelRole::Fable => insert_role_env(
                &mut env,
                "ANTHROPIC_DEFAULT_FABLE_MODEL",
                "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
                &upstream_for_client,
                &model.display_name,
            ),
            ClaudeModelRole::Sonnet => insert_role_env(
                &mut env,
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
                &upstream_for_client,
                &model.display_name,
            ),
            ClaudeModelRole::Haiku => insert_role_env(
                &mut env,
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
                &upstream_for_client,
                &model.display_name,
            ),
            ClaudeModelRole::Subagent => {
                env.insert(
                    "CLAUDE_CODE_SUBAGENT_MODEL".to_string(),
                    json!(upstream_for_client),
                );
            }
            ClaudeModelRole::Fallback => {
                env.insert("ANTHROPIC_MODEL".to_string(), json!(upstream_for_client));
            }
        }
    }

    let desktop_routes = profile
        .models
        .iter()
        .map(|model| {
            (
                model.client_model_id.clone(),
                ClaudeDesktopModelRoute {
                    model: model.upstream_model_id.clone(),
                    label_override: Some(model.display_name.clone()),
                    supports_1m: Some(model.supports_1m),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    Ok(Provider {
        id: provider_id.to_string(),
        name: "AnyRouter".to_string(),
        settings_config: json!({
            "env": env,
            "auth_mode": "bearer_only"
        }),
        website_url: Some(DEFAULT_ANYROUTER_BASE_URL.to_string()),
        category: Some("api".to_string()),
        created_at: None,
        sort_index: None,
        notes: Some("Managed by the canonical CC Gateway Claude profile".to_string()),
        meta: Some(ProviderMeta {
            claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
            claude_desktop_model_routes: desktop_routes,
            api_format: Some("anthropic".to_string()),
            api_key_field: Some("ANTHROPIC_AUTH_TOKEN".to_string()),
            impersonate_claude_code: Some(true),
            custom_user_agent: Some(ANYROUTER_CLAUDE_CODE_USER_AGENT.to_string()),
            local_proxy_request_overrides: Some(LocalProxyRequestOverrides {
                headers: HashMap::from([
                    ("x-app".to_string(), "cli".to_string()),
                    (
                        "anthropic-beta".to_string(),
                        ANYROUTER_ANTHROPIC_BETA.to_string(),
                    ),
                    (
                        "anthropic-client-platform".to_string(),
                        "desktop_app".to_string(),
                    ),
                    (
                        "anthropic-client-version".to_string(),
                        "1.22209.0".to_string(),
                    ),
                ]),
                body: Some(json!({"thinking": {"type": "adaptive"}})),
            }),
            ..ProviderMeta::default()
        }),
        icon: Some("anthropic".to_string()),
        icon_color: None,
        in_failover_queue: false,
    })
}

fn insert_role_env(
    env: &mut Map<String, Value>,
    model_key: &str,
    name_key: &str,
    model: &str,
    display_name: &str,
) {
    env.insert(model_key.to_string(), json!(model));
    env.insert(name_key.to_string(), json!(display_name));
}

pub fn project_claude_code_provider(
    profile: &ClaudeConnectionProfile,
) -> Result<Provider, AppError> {
    projected_provider(profile, CLAUDE_CODE_PROFILE_PROVIDER_ID)
}

pub fn project_claude_desktop_provider(
    profile: &ClaudeConnectionProfile,
) -> Result<Provider, AppError> {
    projected_provider(profile, CLAUDE_DESKTOP_PROFILE_PROVIDER_ID)
}

/// Build an ephemeral provider for a runtime consumer. The API key remains
/// canonical in the profile setting; callers must not persist this projection.
pub fn active_provider_projection(
    db: &crate::database::Database,
    consumer: ClaudeProfileConsumer,
) -> Result<Option<Provider>, AppError> {
    let profile = load_profile(db)?;
    if !profile.enabled {
        return Ok(None);
    }
    match consumer {
        ClaudeProfileConsumer::ClaudeCode => project_claude_code_provider(&profile).map(Some),
        ClaudeProfileConsumer::ClaudeDesktop => project_claude_desktop_provider(&profile).map(Some),
    }
}

fn messages_url(base_url: &str) -> Result<Url, AppError> {
    let mut url = Url::parse(base_url)
        .map_err(|error| AppError::InvalidInput(format!("基础 URL 无效: {error}")))?;
    let path = url.path().trim_end_matches('/');
    let messages_path = if path.ends_with("/v1/messages") {
        path.to_string()
    } else if path.ends_with("/v1") {
        format!("{path}/messages")
    } else if path.is_empty() || path == "/" {
        "/v1/messages".to_string()
    } else {
        format!("{path}/v1/messages")
    };
    url.set_path(&messages_path);
    Ok(url)
}

fn bounded_message(message: &str, secret: &str) -> String {
    let redacted = if secret.is_empty() {
        message.to_string()
    } else {
        message.replace(secret, "[REDACTED]")
    };
    let mut chars = redacted.chars();
    let bounded = chars.by_ref().take(500).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn base_test_result(
    role: ClaudeModelRole,
    model: Option<&ClaudeModelRoute>,
    latency_ms: u64,
    category: ClaudeConnectionTestCategory,
    message: String,
) -> ClaudeModelTestResult {
    ClaudeModelTestResult {
        success: category == ClaudeConnectionTestCategory::Success,
        role,
        requested_model: model
            .map(|route| route.client_model_id.clone())
            .unwrap_or_default(),
        upstream_model: model
            .map(|route| strip_one_m_suffix(&route.upstream_model_id))
            .unwrap_or_default(),
        response_model: None,
        status_code: None,
        latency_ms,
        category,
        message,
    }
}

fn classify_http_failure(status: u16, response_body: &str) -> ClaudeConnectionTestCategory {
    if matches!(status, 401 | 403) {
        return ClaudeConnectionTestCategory::AuthenticationFailed;
    }
    if status == 407 {
        return ClaudeConnectionTestCategory::ProxyInterference;
    }
    if matches!(status, 502..=504) {
        return ClaudeConnectionTestCategory::UpstreamUnavailable;
    }

    let lower_body = response_body.to_ascii_lowercase();
    let structured_error = serde_json::from_str::<Value>(response_body).ok();
    let error_type = structured_error
        .as_ref()
        .and_then(|body| body.pointer("/error/type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let model_error = status == 404
        || error_type.contains("model")
        || lower_body.contains("model_not_found")
        || lower_body.contains("unknown model")
        || lower_body.contains("invalid model")
        || (lower_body.contains("model")
            && (lower_body.contains("not found")
                || lower_body.contains("unavailable")
                || lower_body.contains("does not exist")))
        || response_body.contains("模型不存在")
        || response_body.contains("模型不可用");
    if model_error {
        ClaudeConnectionTestCategory::ModelUnavailable
    } else {
        ClaudeConnectionTestCategory::ProtocolError
    }
}

pub async fn run_model_test(
    profile: &ClaudeConnectionProfile,
    role: ClaudeModelRole,
    timeout: Duration,
) -> ClaudeModelTestResult {
    let Some(model) = profile.model_for_role(role) else {
        return base_test_result(
            role,
            None,
            0,
            ClaudeConnectionTestCategory::Configuration,
            "模型角色未配置".to_string(),
        );
    };
    if profile.upstream_api_key.trim().is_empty() {
        return base_test_result(
            role,
            Some(model),
            0,
            ClaudeConnectionTestCategory::Configuration,
            "AnyRouter API Key 尚未配置".to_string(),
        );
    }
    let url = match messages_url(&profile.base_url) {
        Ok(url) => url,
        Err(error) => {
            return base_test_result(
                role,
                Some(model),
                0,
                ClaudeConnectionTestCategory::Configuration,
                error.to_string(),
            );
        }
    };

    let is_loopback = url
        .host_str()
        .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
    let mut client_builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none());
    if is_loopback {
        client_builder = client_builder.no_proxy();
    }
    let client = match client_builder.build() {
        Ok(client) => client,
        Err(error) => {
            return base_test_result(
                role,
                Some(model),
                0,
                ClaudeConnectionTestCategory::NetworkError,
                bounded_message(&error.to_string(), &profile.upstream_api_key),
            );
        }
    };

    let started = Instant::now();
    let response = client
        .post(url)
        .bearer_auth(profile.upstream_api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", ANYROUTER_ANTHROPIC_BETA)
        .header("user-agent", ANYROUTER_CLAUDE_CODE_USER_AGENT)
        .header("x-app", "cli")
        .header("anthropic-client-platform", "desktop_app")
        .header("anthropic-client-version", "1.22209.0")
        .json(&json!({
            "model": strip_one_m_suffix(&model.upstream_model_id),
            "max_tokens": 64,
            "system": [{"type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY}],
            "thinking": {"type": "adaptive"},
            "messages": [{"role": "user", "content": "Reply with OK."}]
        }))
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let category = if error.is_timeout() {
                ClaudeConnectionTestCategory::Timeout
            } else {
                ClaudeConnectionTestCategory::NetworkError
            };
            return base_test_result(
                role,
                Some(model),
                latency_ms,
                category,
                bounded_message(&error.to_string(), &profile.upstream_api_key),
            );
        }
    };

    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let response_body = response.text().await.unwrap_or_default();
    let mut result = base_test_result(
        role,
        Some(model),
        latency_ms,
        ClaudeConnectionTestCategory::ProtocolError,
        String::new(),
    );
    result.status_code = Some(status.as_u16());

    if status.is_success() {
        if content_type.contains("text/html") {
            result.category = ClaudeConnectionTestCategory::ProxyInterference;
            result.message = "收到 HTML 响应，连接可能被代理或登录页拦截".to_string();
            return result;
        }
        match serde_json::from_str::<Value>(&response_body) {
            Ok(body) if body.get("type").and_then(Value::as_str) == Some("message") => {
                result.success = true;
                result.category = ClaudeConnectionTestCategory::Success;
                result.response_model = body
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                result.message = "AnyRouter 最小模型请求成功".to_string();
            }
            _ => {
                result.message = "响应不是有效的 Anthropic Messages 消息".to_string();
            }
        }
        return result;
    }

    result.category = classify_http_failure(status.as_u16(), &response_body);
    result.message = bounded_message(
        &format!("HTTP {}: {}", status.as_u16(), response_body),
        &profile.upstream_api_key,
    );
    result
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn unique_test_roles(profile: &ClaudeConnectionProfile) -> Vec<ClaudeModelRole> {
    let mut seen = HashSet::new();
    profile
        .models
        .iter()
        .filter_map(|model| {
            let upstream = strip_one_m_suffix(&model.upstream_model_id);
            seen.insert(upstream).then_some(model.role)
        })
        .collect()
}

fn persist_test_summary(
    db: &crate::database::Database,
    mut profile: ClaudeConnectionProfile,
    results: Vec<ClaudeModelTestResult>,
) -> Result<ClaudeConnectionTestSummary, AppError> {
    let summary = ClaudeConnectionTestSummary {
        tested_at_ms: now_unix_ms(),
        success: !results.is_empty() && results.iter().all(|result| result.success),
        results,
    };
    profile.last_test = Some(summary.clone());
    save_profile(db, &profile)?;
    Ok(summary)
}

pub async fn test_profile_model(
    db: &crate::database::Database,
    role: ClaudeModelRole,
) -> Result<ClaudeModelTestResult, AppError> {
    let profile = load_profile(db)?;
    let result = run_model_test(&profile, role, DEFAULT_MODEL_TEST_TIMEOUT).await;
    persist_test_summary(db, profile, vec![result.clone()])?;
    Ok(result)
}

pub async fn test_all_profile_models(
    db: &crate::database::Database,
) -> Result<ClaudeConnectionTestSummary, AppError> {
    let profile = load_profile(db)?;
    let mut results = Vec::new();
    for role in unique_test_roles(&profile) {
        results.push(run_model_test(&profile, role, DEFAULT_MODEL_TEST_TIMEOUT).await);
    }
    persist_test_summary(db, profile, results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn default_profile_has_the_anyrouter_role_catalog() {
        let profile = ClaudeConnectionProfile::default();

        assert_eq!(profile.base_url, "https://anyrouter.top");
        assert_eq!(profile.auth_mode, ClaudeAuthMode::Bearer);
        assert_eq!(profile.api_format, ClaudeApiFormat::Anthropic);
        assert_eq!(profile.models.len(), 6);
        assert_eq!(
            profile
                .model_for_role(ClaudeModelRole::Opus)
                .unwrap()
                .upstream_model_id,
            "claude-opus-4-8"
        );
        assert_eq!(
            profile
                .model_for_role(ClaudeModelRole::Sonnet)
                .unwrap()
                .upstream_model_id,
            "claude-fable-5"
        );
        assert_eq!(
            profile
                .model_for_role(ClaudeModelRole::Fable)
                .unwrap()
                .upstream_model_id,
            "claude-fable-5"
        );
        for role in [
            ClaudeModelRole::Haiku,
            ClaudeModelRole::Subagent,
            ClaudeModelRole::Fallback,
        ] {
            assert_eq!(
                profile.model_for_role(role).unwrap().upstream_model_id,
                "claude-opus-4-6"
            );
        }
    }

    #[test]
    fn normalization_keeps_client_identity_but_never_sends_the_one_m_marker_upstream() {
        let mut profile = ClaudeConnectionProfile::default();
        let opus = profile
            .models
            .iter_mut()
            .find(|model| model.role == ClaudeModelRole::Opus)
            .unwrap();
        opus.client_model_id = "client-opus[1M]".to_string();
        opus.upstream_model_id = "vendor/opus-4-8 [1m]".to_string();
        opus.supports_1m = false;

        let normalized = profile.normalize_for_storage().unwrap();
        let opus = normalized.model_for_role(ClaudeModelRole::Opus).unwrap();

        assert_eq!(opus.client_model_id, "client-opus");
        assert_eq!(opus.upstream_model_id, "vendor/opus-4-8");
        assert!(opus.supports_1m);
        assert_eq!(opus.client_model_with_capability(), "client-opus[1M]");
    }

    #[test]
    fn validation_rejects_missing_roles_and_non_https_remote_urls() {
        let mut missing_role = ClaudeConnectionProfile::default();
        missing_role
            .models
            .retain(|model| model.role != ClaudeModelRole::Fallback);
        assert!(missing_role.normalize_for_storage().is_err());

        let mut insecure = ClaudeConnectionProfile::default();
        insecure.base_url = "http://anyrouter.top".to_string();
        assert!(insecure.normalize_for_storage().is_err());

        let mut localhost = ClaudeConnectionProfile::default();
        localhost.base_url = "http://127.0.0.1:32123/".to_string();
        assert_eq!(
            localhost.normalize_for_storage().unwrap().base_url,
            "http://127.0.0.1:32123"
        );
    }

    #[test]
    fn update_is_the_single_secret_store_and_state_is_redacted() {
        let db = crate::database::Database::memory().unwrap();
        let defaults = load_profile(&db).unwrap();
        let updated = update_profile(
            &db,
            ClaudeConnectionProfileInput {
                enabled: true,
                base_url: defaults.base_url,
                api_key: Some("sk-local-test-only".to_string()),
                models: defaults.models,
            },
        )
        .unwrap();

        assert!(updated.enabled);
        assert!(updated.has_api_key);
        assert_eq!(updated.masked_api_key, "sk-l...only");
        let serialized = serde_json::to_string(&updated).unwrap();
        assert!(!serialized.contains("sk-local-test-only"));

        let raw = db
            .get_setting(CLAUDE_CONNECTION_PROFILE_SETTING_KEY)
            .unwrap()
            .unwrap();
        assert_eq!(raw.matches("sk-local-test-only").count(), 1);

        let preserved = update_profile(
            &db,
            ClaudeConnectionProfileInput {
                enabled: true,
                base_url: updated.base_url.clone(),
                api_key: None,
                models: updated.models.clone(),
            },
        )
        .unwrap();
        assert_eq!(preserved.masked_api_key, "sk-l...only");
    }

    #[test]
    fn provider_projections_share_the_profile_without_persisting_provider_copies() {
        let mut profile = ClaudeConnectionProfile::default();
        profile.upstream_api_key = "sk-ephemeral".to_string();
        let opus = profile
            .models
            .iter_mut()
            .find(|model| model.role == ClaudeModelRole::Opus)
            .unwrap();
        opus.client_model_id = "claude-opus-client[1M]".to_string();
        opus.upstream_model_id = "vendor-opus[1M]".to_string();
        let profile = profile.normalize_for_storage().unwrap();

        let code = project_claude_code_provider(&profile).unwrap();
        let desktop = project_claude_desktop_provider(&profile).unwrap();

        assert_eq!(
            code.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            "sk-ephemeral"
        );
        assert_eq!(
            code.settings_config["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "vendor-opus[1M]"
        );
        assert_eq!(
            desktop.meta.as_ref().unwrap().claude_desktop_model_routes["claude-opus-client"].model,
            "vendor-opus"
        );
        assert_eq!(
            desktop.meta.as_ref().unwrap().claude_desktop_mode,
            Some(crate::provider::ClaudeDesktopMode::Proxy)
        );
        assert_eq!(
            desktop.meta.as_ref().unwrap().api_format.as_deref(),
            Some("anthropic")
        );
        let meta = desktop.meta.as_ref().unwrap();
        assert_eq!(meta.impersonate_claude_code, Some(true));
        assert_eq!(
            meta.custom_user_agent.as_deref(),
            Some(ANYROUTER_CLAUDE_CODE_USER_AGENT)
        );
        let overrides = meta.local_proxy_request_overrides.as_ref().unwrap();
        assert_eq!(
            overrides.headers.get("anthropic-beta").map(String::as_str),
            Some(ANYROUTER_ANTHROPIC_BETA)
        );
        assert_eq!(
            overrides.headers.get("x-app").map(String::as_str),
            Some("cli")
        );
        assert_eq!(
            overrides.body.as_ref().unwrap()["thinking"]["type"],
            "adaptive"
        );
    }

    #[test]
    fn active_projection_is_gated_by_the_single_profile_enabled_flag() {
        let db = crate::database::Database::memory().unwrap();
        assert!(
            active_provider_projection(&db, ClaudeProfileConsumer::ClaudeCode)
                .unwrap()
                .is_none()
        );

        let defaults = load_profile(&db).unwrap();
        update_profile(
            &db,
            ClaudeConnectionProfileInput {
                enabled: true,
                base_url: defaults.base_url,
                api_key: Some("sk-active-test".to_string()),
                models: defaults.models,
            },
        )
        .unwrap();
        let projected = active_provider_projection(&db, ClaudeProfileConsumer::ClaudeDesktop)
            .unwrap()
            .unwrap();
        assert_eq!(projected.id, CLAUDE_DESKTOP_PROFILE_PROVIDER_ID);
    }

    #[tokio::test]
    async fn minimal_model_test_uses_bearer_and_reports_the_upstream_response_model() {
        let captured = Arc::new(Mutex::new(None::<(HeaderMap, Value)>));
        let capture = captured.clone();
        let app = Router::new()
            .route(
                "/v1/messages",
                post(
                    |State(captured): State<Arc<Mutex<Option<(HeaderMap, Value)>>>>,
                     headers: HeaderMap,
                     Json(body): Json<Value>| async move {
                        *captured.lock().unwrap() = Some((headers, body));
                        Json(json!({
                            "id": "msg_test",
                            "type": "message",
                            "model": "anyrouter-confirmed-opus",
                            "content": [],
                            "stop_reason": "end_turn",
                            "usage": {"input_tokens": 1, "output_tokens": 1}
                        }))
                    },
                ),
            )
            .with_state(capture);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut profile = ClaudeConnectionProfile::default();
        profile.base_url = format!("http://{address}");
        profile.upstream_api_key = "sk-mock-only".to_string();
        profile
            .models
            .iter_mut()
            .find(|model| model.role == ClaudeModelRole::Opus)
            .unwrap()
            .upstream_model_id = "vendor-opus[1M]".to_string();
        let profile = profile.normalize_for_storage().unwrap();

        let result = run_model_test(&profile, ClaudeModelRole::Opus, Duration::from_secs(2)).await;

        assert!(result.success);
        assert_eq!(result.requested_model, "claude-opus-4-8");
        assert_eq!(result.upstream_model, "vendor-opus");
        assert_eq!(
            result.response_model.as_deref(),
            Some("anyrouter-confirmed-opus")
        );
        let (headers, body) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(headers["authorization"], "Bearer sk-mock-only");
        assert_eq!(headers["user-agent"], ANYROUTER_CLAUDE_CODE_USER_AGENT);
        assert_eq!(headers["anthropic-beta"], ANYROUTER_ANTHROPIC_BETA);
        assert_eq!(headers["x-app"], "cli");
        assert_eq!(body["model"], "vendor-opus");
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["system"][0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
        assert_eq!(body["thinking"]["type"], "adaptive");
    }

    #[test]
    fn test_all_targets_each_distinct_upstream_model_once() {
        let profile = ClaudeConnectionProfile::default();
        assert_eq!(
            unique_test_roles(&profile),
            vec![
                ClaudeModelRole::Opus,
                ClaudeModelRole::Fable,
                ClaudeModelRole::Haiku,
            ]
        );
    }

    #[test]
    fn structured_model_errors_are_not_reported_as_generic_protocol_errors() {
        assert_eq!(
            classify_http_failure(
                400,
                r#"{"error":{"type":"model_not_found","message":"模型不存在"}}"#,
            ),
            ClaudeConnectionTestCategory::ModelUnavailable
        );
        assert_eq!(
            classify_http_failure(401, "invalid key"),
            ClaudeConnectionTestCategory::AuthenticationFailed
        );
        assert_eq!(
            classify_http_failure(407, "proxy auth required"),
            ClaudeConnectionTestCategory::ProxyInterference
        );
    }
}
