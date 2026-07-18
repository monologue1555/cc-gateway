//! Agent Gateway's local authentication and presentation settings.
//!
//! Provider selection, failover, and model mapping deliberately remain owned by
//! the existing Claude Desktop proxy route. The gateway is only a thin facade
//! over that route and therefore persists no provider or routing state.

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, Provider};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

pub const AGENT_GATEWAY_CONFIG_KEY: &str = "agent_gateway_config";
pub const AGENT_GATEWAY_SOURCE_SCOPE: &str = "claude-desktop";
pub const AGENT_GATEWAY_LISTEN_ADDRESS: &str = "127.0.0.1";
pub const AGENT_GATEWAY_PATH_PREFIX: &str = "/agent/v1";

const MISSING_PROVIDER_MESSAGE: &str =
    "Claude Desktop 尚未选择供应商。请先启用一个 Anthropic Messages（原生）代理供应商。";
const INCOMPATIBLE_PROVIDER_MESSAGE: &str = "Agent Gateway 当前仅支持 Claude Desktop 的 Anthropic Messages（原生）代理供应商。请切换供应商的 API 格式后重试。";

pub(crate) fn listen_address_is_safe(address: &str) -> bool {
    address
        .trim()
        .parse::<Ipv4Addr>()
        .is_ok_and(|ip| ip == Ipv4Addr::LOCALHOST)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatewayEndpoints {
    pub anthropic: String,
    pub responses: String,
    pub chat: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatewayModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "supports1m")]
    pub supports_1m: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatewayState {
    pub enabled: bool,
    pub listen_address: String,
    pub listen_port: u16,
    pub endpoints: AgentGatewayEndpoints,
    pub masked_token: String,
    pub current_provider_id: Option<String>,
    pub current_provider_name: Option<String>,
    pub compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility_message: Option<String>,
    pub auto_failover_enabled: bool,
    pub emulate_claude_code: bool,
    pub models: Vec<AgentGatewayModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatewayConfigInput {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub emulate_claude_code: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatewayTokenResult {
    pub token: String,
    pub masked_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatewayTestInput {
    pub protocol: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGatewayTestResult {
    pub success: bool,
    pub protocol: String,
    pub latency_ms: u64,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The only persisted Agent Gateway state. Claude Desktop remains the single
/// routing control plane and owns its current provider, failover queue, and
/// model mappings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub emulate_claude_code: bool,
}

impl Default for AgentGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: String::new(),
            emulate_claude_code: false,
        }
    }
}

fn generate_token() -> String {
    format!("ccs-agent-{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn mask_token(token: &str) -> String {
    let token = token.trim();
    if token.len() <= 8 || !token.is_ascii() {
        return "***".to_string();
    }

    if let Some(secret) = token.strip_prefix("ccs-agent-") {
        let suffix_start = secret.len().saturating_sub(4);
        return format!("ccs-agent-••••••••{}", &secret[suffix_start..]);
    }

    format!("{}...{}", &token[..4], &token[token.len() - 4..])
}

fn parse_config(raw: &str) -> Result<AgentGatewayConfig, AppError> {
    serde_json::from_str(raw)
        .map_err(|error| AppError::Config(format!("Agent Gateway 配置 JSON 无效: {error}")))
}

fn save_config(db: &Database, config: &AgentGatewayConfig) -> Result<(), AppError> {
    let raw = serde_json::to_string(config)
        .map_err(|error| AppError::Config(format!("Agent Gateway 配置序列化失败: {error}")))?;
    db.set_setting(AGENT_GATEWAY_CONFIG_KEY, &raw)
}

/// Load the gateway settings and lazily create its independent local key.
pub(crate) fn load_agent_gateway_config(db: &Database) -> Result<AgentGatewayConfig, AppError> {
    let stored = db.get_setting(AGENT_GATEWAY_CONFIG_KEY)?;
    let (mut config, mut needs_save) = match stored.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            let config = parse_config(raw)?;
            let stored_value = serde_json::from_str::<serde_json::Value>(raw).ok();
            let canonical_value = serde_json::to_value(&config).ok();
            let needs_save = stored_value != canonical_value;
            (config, needs_save)
        }
        _ => (AgentGatewayConfig::default(), true),
    };

    if config.token.trim().is_empty() {
        config.token = generate_token();
        needs_save = true;
    }
    if needs_save {
        save_config(db, &config)?;
    }

    Ok(config)
}

/// Read the effective Claude Desktop provider selected by CC Switch. This is a
/// view of the existing route, not a second selection mechanism.
pub(crate) fn desktop_current_provider(db: &Database) -> Result<Option<Provider>, AppError> {
    let Some(provider_id) =
        crate::settings::get_effective_current_provider(db, &AppType::ClaudeDesktop)?
    else {
        return Ok(None);
    };
    db.get_provider_by_id(&provider_id, AGENT_GATEWAY_SOURCE_SCOPE)
}

/// Whether a Claude Desktop provider can back all three Agent Gateway
/// protocols without a second response-normalization layer.
pub(crate) fn provider_compatible(provider: &Provider) -> bool {
    crate::claude_desktop_config::provider_mode(provider) == ClaudeDesktopMode::Proxy
        && crate::proxy::providers::get_claude_api_format(provider) == "anthropic"
}

fn compatibility_status(providers: &[Provider]) -> (bool, Option<String>) {
    if providers.is_empty() {
        return (false, Some(MISSING_PROVIDER_MESSAGE.to_string()));
    }
    match ensure_provider_chain_compatible(providers) {
        Ok(()) => (true, None),
        Err(message) => (false, Some(message)),
    }
}

fn desktop_route_providers(
    db: &Database,
    auto_failover_enabled: bool,
) -> Result<Vec<Provider>, AppError> {
    if !auto_failover_enabled {
        return Ok(desktop_current_provider(db)?.into_iter().collect());
    }

    db.get_failover_queue(AGENT_GATEWAY_SOURCE_SCOPE)?
        .into_iter()
        .map(|item| db.get_provider_by_id(&item.provider_id, AGENT_GATEWAY_SOURCE_SCOPE))
        .filter_map(|result| match result {
            Ok(Some(provider)) => Some(Ok(provider)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

pub(crate) fn ensure_provider_chain_compatible(providers: &[Provider]) -> Result<(), String> {
    let incompatible = providers
        .iter()
        .filter(|provider| !provider_compatible(provider))
        .map(|provider| {
            format!(
                "{} ({})",
                provider.name,
                crate::proxy::providers::get_claude_api_format(provider)
            )
        })
        .collect::<Vec<_>>();
    if incompatible.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{INCOMPATIBLE_PROVIDER_MESSAGE} 不兼容的故障转移候选：{}",
            incompatible.join("、")
        ))
    }
}

fn ensure_desktop_route_compatible(db: &Database) -> Result<Vec<Provider>, AppError> {
    let (_, auto_failover_enabled) = db.get_proxy_flags_sync(AGENT_GATEWAY_SOURCE_SCOPE);
    let providers = desktop_route_providers(db, auto_failover_enabled)?;
    if providers.is_empty() {
        return Err(AppError::Config(MISSING_PROVIDER_MESSAGE.to_string()));
    }
    ensure_provider_chain_compatible(&providers).map_err(AppError::Config)?;
    Ok(providers)
}

/// Present the exact model catalog produced by Claude Desktop's proxy mapping
/// implementation. Invalid/direct provider configurations intentionally expose
/// an empty catalog; the shared HTTP route returns the authoritative error.
pub(crate) fn model_catalog(provider: &Provider) -> Vec<AgentGatewayModel> {
    if !provider_compatible(provider) {
        return Vec::new();
    }

    crate::claude_desktop_config::proxy_model_routes(provider)
        .unwrap_or_default()
        .into_iter()
        .map(|route| AgentGatewayModel {
            id: route.route_id,
            label: route.label_override,
            supports_1m: route.supports_1m,
            upstream_model: Some(route.upstream_model),
        })
        .collect()
}

pub(crate) fn update_config(
    db: &Database,
    input: AgentGatewayConfigInput,
) -> Result<AgentGatewayConfig, AppError> {
    if input.enabled {
        ensure_desktop_route_compatible(db)?;
    }
    let existing = load_agent_gateway_config(db)?;
    let config = AgentGatewayConfig {
        enabled: input.enabled,
        token: existing.token,
        emulate_claude_code: input.emulate_claude_code,
    };
    save_config(db, &config)?;
    Ok(config)
}

pub(crate) fn regenerate_token(db: &Database) -> Result<AgentGatewayTokenResult, AppError> {
    let mut config = load_agent_gateway_config(db)?;
    config.token = generate_token();
    save_config(db, &config)?;
    Ok(AgentGatewayTokenResult {
        masked_token: mask_token(&config.token),
        token: config.token,
    })
}

pub(crate) fn build_state(db: &Database, listen_port: u16) -> Result<AgentGatewayState, AppError> {
    let config = load_agent_gateway_config(db)?;
    let origin = format!("http://{}:{}", AGENT_GATEWAY_LISTEN_ADDRESS, listen_port);
    let (_, desktop_auto_failover_enabled) = db.get_proxy_flags_sync(AGENT_GATEWAY_SOURCE_SCOPE);
    let route_providers = desktop_route_providers(db, desktop_auto_failover_enabled)?;
    let route_provider = route_providers.first();
    let (compatible, compatibility_message) = compatibility_status(&route_providers);

    Ok(AgentGatewayState {
        enabled: config.enabled,
        listen_address: AGENT_GATEWAY_LISTEN_ADDRESS.to_string(),
        listen_port,
        endpoints: AgentGatewayEndpoints {
            anthropic: format!("{origin}{AGENT_GATEWAY_PATH_PREFIX}/messages"),
            responses: format!("{origin}{AGENT_GATEWAY_PATH_PREFIX}/responses"),
            chat: format!("{origin}{AGENT_GATEWAY_PATH_PREFIX}/chat/completions"),
        },
        masked_token: mask_token(&config.token),
        current_provider_id: route_provider.map(|provider| provider.id.clone()),
        current_provider_name: route_provider.map(|provider| provider.name.clone()),
        compatible,
        compatibility_message,
        auto_failover_enabled: desktop_auto_failover_enabled,
        emulate_claude_code: config.emulate_claude_code,
        models: if compatible {
            route_provider.map(model_catalog).unwrap_or_default()
        } else {
            Vec::new()
        },
    })
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        difference |= (left_byte ^ right_byte) as usize;
    }
    difference == 0
}

pub(crate) fn token_matches(config: &AgentGatewayConfig, candidate: &str) -> bool {
    let expected = config.token.trim().as_bytes();
    let candidate = candidate.trim().as_bytes();
    !expected.is_empty() && constant_time_eq(expected, candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeDesktopModelRoute, ProviderMeta};
    use serde_json::json;
    use std::collections::HashMap;

    fn provider_with_routes() -> Provider {
        Provider {
            id: "p1".to_string(),
            name: "AnyRouter".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "upstream-secret",
                    "ANTHROPIC_BASE_URL": "https://example.invalid"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: Some("test".to_string()),
            meta: Some(ProviderMeta {
                claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
                claude_desktop_model_routes: HashMap::from([
                    (
                        "claude-fable-5".to_string(),
                        ClaudeDesktopModelRoute {
                            model: "fable-upstream".to_string(),
                            label_override: Some("Fable 5".to_string()),
                            supports_1m: Some(true),
                        },
                    ),
                    (
                        "claude-opus-4-8".to_string(),
                        ClaudeDesktopModelRoute {
                            model: "opus-upstream".to_string(),
                            label_override: None,
                            supports_1m: Some(false),
                        },
                    ),
                ]),
                api_format: Some("anthropic".to_string()),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn masks_local_gateway_token_without_exposing_the_secret() {
        let token = "ccs-agent-1234567890abcdef";
        let masked = mask_token(token);

        assert_eq!(masked, "ccs-agent-••••••••cdef");
        assert!(!masked.contains("1234567890ab"));
    }

    #[test]
    fn listener_requires_the_exact_advertised_loopback_address() {
        assert!(listen_address_is_safe("127.0.0.1"));
        for address in [
            "0.0.0.0",
            "127.0.0.2",
            "::1",
            "::",
            "localhost",
            "192.168.1.10",
        ] {
            assert!(!listen_address_is_safe(address), "{address}");
        }
    }

    #[test]
    fn legacy_routing_fields_are_ignored() {
        let parsed = parse_config(
            r#"{"enabled":true,"providerIds":["p1"],"currentProviderId":"p1","autoFailoverEnabled":true}"#,
        )
        .expect("legacy config should remain readable");

        assert!(parsed.enabled);
        assert!(!parsed.emulate_claude_code);
        assert!(parsed.token.is_empty());
    }

    #[test]
    fn catalog_reuses_claude_desktop_route_normalization() {
        let models = model_catalog(&provider_with_routes());

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-fable-5", "claude-opus-4-8"]
        );
        assert_eq!(models[0].upstream_model.as_deref(), Some("fable-upstream"));
        assert!(models[0].supports_1m);
    }

    #[test]
    fn agent_gateway_accepts_only_native_anthropic_desktop_proxy_providers() {
        let native_proxy = provider_with_routes();
        assert!(provider_compatible(&native_proxy));

        let mut transformed_proxy = native_proxy.clone();
        transformed_proxy
            .meta
            .as_mut()
            .expect("provider meta")
            .api_format = Some("openai_chat".to_string());
        assert!(!provider_compatible(&transformed_proxy));

        let mut direct_provider = native_proxy;
        direct_provider
            .meta
            .as_mut()
            .expect("provider meta")
            .claude_desktop_mode = Some(ClaudeDesktopMode::Direct);
        assert!(!provider_compatible(&direct_provider));
    }

    #[test]
    fn incompatible_provider_exposes_no_agent_models() {
        let mut provider = provider_with_routes();
        provider.meta.as_mut().expect("provider meta").api_format =
            Some("openai_responses".to_string());

        assert!(model_catalog(&provider).is_empty());
    }

    #[test]
    fn state_explains_when_current_desktop_provider_is_incompatible() {
        let db = Database::memory().expect("memory database");
        let mut provider = provider_with_routes();
        provider.meta.as_mut().expect("provider meta").api_format =
            Some("gemini_native".to_string());
        db.save_provider(AGENT_GATEWAY_SOURCE_SCOPE, &provider)
            .expect("save provider");
        db.set_current_provider(AGENT_GATEWAY_SOURCE_SCOPE, &provider.id)
            .expect("select provider");

        let state = build_state(&db, 15721).expect("build state");

        assert!(!state.compatible);
        assert!(state
            .compatibility_message
            .as_deref()
            .is_some_and(|message| message.contains("Anthropic Messages")));
        assert!(state.models.is_empty());
    }

    #[test]
    fn mixed_provider_chain_is_incompatible() {
        let mut native = provider_with_routes();
        native.id = "native".to_string();
        let mut transformed = provider_with_routes();
        transformed.id = "transformed".to_string();
        transformed.name = "OpenAI-compatible route".to_string();
        transformed.meta.as_mut().expect("provider meta").api_format =
            Some("openai_chat".to_string());

        let error = ensure_provider_chain_compatible(&[native, transformed])
            .expect_err("mixed route must be rejected");

        assert!(error.contains("Anthropic Messages"));
        assert!(error.contains("OpenAI-compatible route"));
    }

    #[test]
    fn update_persists_only_local_gateway_settings() {
        let db = Database::memory().expect("memory database");
        let provider = provider_with_routes();
        db.save_provider(AGENT_GATEWAY_SOURCE_SCOPE, &provider)
            .expect("save provider");
        db.set_current_provider(AGENT_GATEWAY_SOURCE_SCOPE, &provider.id)
            .expect("select provider");
        db.set_setting(
            AGENT_GATEWAY_CONFIG_KEY,
            r#"{"enabled":false,"token":"ccs-agent-existing","providerIds":["p1"],"currentProviderId":"p1","autoFailoverEnabled":true}"#,
        )
        .expect("seed legacy config");

        let config = update_config(
            &db,
            AgentGatewayConfigInput {
                enabled: true,
                emulate_claude_code: true,
            },
        )
        .expect("save gateway config");

        assert_eq!(config.token, "ccs-agent-existing");
        let raw = db
            .get_setting(AGENT_GATEWAY_CONFIG_KEY)
            .expect("read config")
            .expect("stored config");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&raw).expect("valid json"),
            json!({
                "enabled": true,
                "token": "ccs-agent-existing",
                "emulateClaudeCode": true
            })
        );
    }

    #[test]
    fn enabling_gateway_requires_a_current_desktop_provider() {
        let db = Database::memory().expect("memory database");

        let error = update_config(
            &db,
            AgentGatewayConfigInput {
                enabled: true,
                emulate_claude_code: false,
            },
        )
        .expect_err("gateway must reject a missing provider");

        assert!(error.to_string().contains("尚未选择供应商"));
    }

    #[test]
    fn enabling_gateway_rejects_a_transformed_desktop_provider() {
        let db = Database::memory().expect("memory database");
        let mut provider = provider_with_routes();
        provider.meta.as_mut().expect("provider meta").api_format = Some("openai_chat".to_string());
        db.save_provider(AGENT_GATEWAY_SOURCE_SCOPE, &provider)
            .expect("save provider");
        db.set_current_provider(AGENT_GATEWAY_SOURCE_SCOPE, &provider.id)
            .expect("select provider");

        let error = update_config(
            &db,
            AgentGatewayConfigInput {
                enabled: true,
                emulate_claude_code: false,
            },
        )
        .expect_err("gateway must reject a transformed provider");

        assert!(error.to_string().contains("Anthropic Messages"));
    }

    #[test]
    fn token_match_is_exact() {
        let config = AgentGatewayConfig {
            token: "ccs-agent-secret".to_string(),
            ..AgentGatewayConfig::default()
        };

        assert!(token_matches(&config, "ccs-agent-secret"));
        assert!(!token_matches(&config, "ccs-agent-secrex"));
        assert!(!token_matches(&config, "ccs-agent-secret-extra"));
    }
}
