//! Selective, read-only import from a user-chosen legacy CC Switch database.
//!
//! This module deliberately does not call any service that materializes Claude
//! Code or Claude Desktop live configuration. Import is a database-only
//! operation and must remain opt-in.

use crate::agent_gateway::connection_profile::{
    strip_one_m_suffix, ClaudeApiFormat, ClaudeAuthMode, ClaudeConnectionProfile, ClaudeModelRole,
    ClaudeModelRoute,
};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

const ALLOWED_APP_TYPES: [&str; 2] = ["claude", "claude-desktop"];
const LOCAL_GATEWAY_TOKEN_PREFIXES: [&str; 3] = ["ccs-agent-", "ccgateway-", "cc-gateway-"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyProviderCounts {
    pub claude: usize,
    pub claude_desktop: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportCounts {
    pub providers: LegacyProviderCounts,
    pub mcp_servers: usize,
    pub skills: usize,
    pub prompts: usize,
    pub profiles: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyExcludedCounts {
    pub other_agent_providers: usize,
    pub local_gateway_providers: usize,
    pub gateway_settings: usize,
    pub proxy_or_takeover_rows: usize,
    pub sync_settings: usize,
    pub history_or_usage_rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyCanonicalCandidatePreview {
    pub source_app_type: String,
    pub source_provider_id: String,
    pub source_provider_name: String,
    pub base_url: String,
    pub has_api_key: bool,
    pub masked_api_key: String,
    pub models: Vec<ClaudeModelRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportPreview {
    pub providers: LegacyProviderCounts,
    pub mcp_servers: usize,
    pub skills: usize,
    pub prompts: usize,
    pub profiles: usize,
    pub excluded: LegacyExcludedCounts,
    pub canonical_candidate: Option<LegacyCanonicalCandidatePreview>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportOptions {
    /// Explicitly replace a non-default canonical profile. False protects the
    /// user's already configured CC Gateway connection.
    #[serde(default)]
    pub replace_canonical_profile: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyCanonicalImportResult {
    pub imported: bool,
    pub replaced_existing: bool,
    pub reason: String,
    pub candidate: Option<LegacyCanonicalCandidatePreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportResult {
    pub imported: LegacyImportCounts,
    pub excluded: LegacyExcludedCounts,
    pub canonical_profile: LegacyCanonicalImportResult,
    /// Always false. This field makes the no-live-write contract explicit to
    /// the caller and is backed by the database-only execution interface.
    pub live_configuration_touched: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct LegacyProviderRow {
    id: String,
    app_type: String,
    name: String,
    settings_config: String,
    website_url: Option<String>,
    category: Option<String>,
    created_at: Option<i64>,
    sort_index: Option<i64>,
    notes: Option<String>,
    icon: Option<String>,
    icon_color: Option<String>,
    meta: String,
    is_current: bool,
}

#[derive(Debug, Clone)]
struct CanonicalCandidate {
    preview: LegacyCanonicalCandidatePreview,
    profile: ClaudeConnectionProfile,
}

fn open_legacy_read_only(path: &Path) -> Result<Connection, AppError> {
    if !path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "旧 CC Switch 数据库不存在或不是文件: {}",
            path.display()
        )));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags)
        .map_err(|error| AppError::Database(format!("无法只读打开旧 CC Switch 数据库: {error}")))?;
    conn.pragma_update(None, "query_only", true)
        .map_err(|error| AppError::Database(format!("无法启用只读保护: {error}")))?;
    Ok(conn)
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, AppError> {
    if !table_exists(conn, table)? {
        return Ok(HashSet::new());
    }
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = HashSet::new();
    for row in rows {
        columns.insert(row?);
    }
    Ok(columns)
}

fn optional_column(columns: &HashSet<String>, name: &str, fallback: &str) -> String {
    if columns.contains(name) {
        name.to_string()
    } else {
        format!("{fallback} AS {name}")
    }
}

fn load_legacy_providers(conn: &Connection) -> Result<Vec<LegacyProviderRow>, AppError> {
    let columns = table_columns(conn, "providers")?;
    for required in ["id", "app_type", "name", "settings_config"] {
        if !columns.contains(required) {
            return Ok(Vec::new());
        }
    }
    let meta = optional_column(&columns, "meta", "'{}'");
    let is_current = optional_column(&columns, "is_current", "0");
    let website_url = optional_column(&columns, "website_url", "NULL");
    let category = optional_column(&columns, "category", "NULL");
    let created_at = optional_column(&columns, "created_at", "NULL");
    let sort_index = optional_column(&columns, "sort_index", "NULL");
    let notes = optional_column(&columns, "notes", "NULL");
    let icon = optional_column(&columns, "icon", "NULL");
    let icon_color = optional_column(&columns, "icon_color", "NULL");
    let sql = format!(
        "SELECT id, app_type, name, settings_config, {website_url}, {category},
         {created_at}, {sort_index}, {notes}, {icon}, {icon_color}, {meta}, {is_current}
         FROM providers"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(LegacyProviderRow {
            id: row.get(0)?,
            app_type: row.get(1)?,
            name: row.get(2)?,
            settings_config: row.get(3)?,
            website_url: row.get(4)?,
            category: row.get(5)?,
            created_at: row.get(6)?,
            sort_index: row.get(7)?,
            notes: row.get(8)?,
            icon: row.get(9)?,
            icon_color: row.get(10)?,
            meta: row.get(11)?,
            is_current: row.get(12)?,
        })
    })?;
    let mut providers = Vec::new();
    for row in rows {
        providers.push(row?);
    }
    Ok(providers)
}

fn count_where(conn: &Connection, table: &str, predicate: &str) -> Result<usize, AppError> {
    if !table_exists(conn, table)? {
        return Ok(0);
    }
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {predicate}");
    let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
    Ok(count.max(0) as usize)
}

fn count_table(conn: &Connection, table: &str) -> Result<usize, AppError> {
    count_where(conn, table, "1 = 1")
}

fn first_non_empty_string<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a str> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn has_local_gateway_credential(base_url: &str, api_key: &str) -> bool {
    let lower_url = base_url.to_ascii_lowercase();
    let is_loopback = lower_url.contains("localhost")
        || lower_url.contains("127.0.0.1")
        || lower_url.contains("[::1]")
        || lower_url.contains("/agent/v1");
    is_loopback
        || LOCAL_GATEWAY_TOKEN_PREFIXES
            .iter()
            .any(|prefix| api_key.starts_with(prefix))
}

fn provider_is_local_gateway(provider: &LegacyProviderRow) -> bool {
    let Ok(settings) = serde_json::from_str::<Value>(&provider.settings_config) else {
        return false;
    };
    let base_url =
        first_non_empty_string(&settings, &["/env/ANTHROPIC_BASE_URL"]).unwrap_or_default();
    let api_key = first_non_empty_string(
        &settings,
        &[
            "/env/ANTHROPIC_AUTH_TOKEN",
            "/env/ANTHROPIC_API_KEY",
            "/env/OPENROUTER_API_KEY",
        ],
    )
    .unwrap_or_default();
    has_local_gateway_credential(base_url, api_key)
}

fn masked_secret(secret: &str) -> String {
    if secret.is_empty() {
        return String::new();
    }
    if !secret.is_ascii() || secret.len() <= 8 {
        return "***".to_string();
    }
    format!("{}...{}", &secret[..4], &secret[secret.len() - 4..])
}

fn env_model(
    settings: &Value,
    role: ClaudeModelRole,
    model_key: &str,
    name_key: &str,
    fallback: &str,
    fallback_name: &str,
    fallback_supports_1m: bool,
) -> ClaudeModelRoute {
    let configured_model = settings
        .pointer(&format!("/env/{model_key}"))
        .and_then(Value::as_str);
    let raw_model = configured_model.unwrap_or(fallback).trim();
    let supports_1m = raw_model.to_ascii_lowercase().ends_with("[1m]")
        || (configured_model.is_none() && fallback_supports_1m);
    let model = strip_one_m_suffix(raw_model);
    let display_name = settings
        .pointer(&format!("/env/{name_key}"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback_name)
        .to_string();
    ClaudeModelRoute {
        role,
        display_name,
        client_model_id: model.clone(),
        upstream_model_id: model,
        supports_1m,
    }
}

fn apply_desktop_model_routes(meta: &Value, models: &mut [ClaudeModelRoute]) {
    let Some(routes) = meta
        .get("claudeDesktopModelRoutes")
        .or_else(|| meta.get("claude_desktop_model_routes"))
        .and_then(Value::as_object)
    else {
        return;
    };
    for (client_model, route) in routes {
        let Some(upstream_model) = route.get("model").and_then(Value::as_str) else {
            continue;
        };
        let suffix_declares_1m = client_model.to_ascii_lowercase().ends_with("[1m]")
            || upstream_model.to_ascii_lowercase().ends_with("[1m]");
        let client_model = strip_one_m_suffix(client_model);
        let upstream_model = strip_one_m_suffix(upstream_model);
        if client_model.is_empty() || upstream_model.is_empty() {
            continue;
        }
        let identity = format!("{client_model} {upstream_model}").to_ascii_lowercase();
        let roles: &[ClaudeModelRole] = if identity.contains("fable") || identity.contains("sonnet")
        {
            &[ClaudeModelRole::Fable, ClaudeModelRole::Sonnet]
        } else if identity.contains("opus-4-8")
            || identity.contains("opus-4.8")
            || identity.contains("opus_4_8")
        {
            &[ClaudeModelRole::Opus]
        } else if identity.contains("opus-4-6")
            || identity.contains("opus-4.6")
            || identity.contains("opus_4_6")
            || identity.contains("haiku")
        {
            &[
                ClaudeModelRole::Haiku,
                ClaudeModelRole::Subagent,
                ClaudeModelRole::Fallback,
            ]
        } else {
            continue;
        };
        let label = route
            .get("labelOverride")
            .or_else(|| route.get("label_override"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty());
        let supports_1m = route
            .get("supports1m")
            .or_else(|| route.get("supports_1m"))
            .and_then(Value::as_bool)
            .unwrap_or(suffix_declares_1m);
        for model in models
            .iter_mut()
            .filter(|model| roles.contains(&model.role))
        {
            model.client_model_id = client_model.clone();
            model.upstream_model_id = upstream_model.clone();
            model.supports_1m = supports_1m;
            if let Some(label) = label {
                model.display_name = label.to_string();
            }
        }
    }
}

fn candidate_from_provider(provider: &LegacyProviderRow) -> Option<CanonicalCandidate> {
    let settings: Value = serde_json::from_str(&provider.settings_config).ok()?;
    let meta: Value = serde_json::from_str(&provider.meta).unwrap_or(Value::Null);
    let base_url = first_non_empty_string(&settings, &["/env/ANTHROPIC_BASE_URL"])?;
    let api_key = first_non_empty_string(
        &settings,
        &[
            "/env/ANTHROPIC_AUTH_TOKEN",
            "/env/ANTHROPIC_API_KEY",
            "/env/OPENROUTER_API_KEY",
        ],
    )?;
    if has_local_gateway_credential(base_url, api_key) {
        return None;
    }

    let opus = env_model(
        &settings,
        ClaudeModelRole::Opus,
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
        "claude-opus-4-8",
        "Claude Opus 4.8",
        true,
    );
    let sonnet = env_model(
        &settings,
        ClaudeModelRole::Sonnet,
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        "claude-fable-5",
        "Claude Fable 5",
        true,
    );
    let fable = env_model(
        &settings,
        ClaudeModelRole::Fable,
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
        &sonnet.upstream_model_id,
        &sonnet.display_name,
        sonnet.supports_1m,
    );
    let haiku = env_model(
        &settings,
        ClaudeModelRole::Haiku,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        "claude-opus-4-6",
        "Claude Opus 4.6",
        true,
    );
    let subagent = env_model(
        &settings,
        ClaudeModelRole::Subagent,
        "CLAUDE_CODE_SUBAGENT_MODEL",
        "CLAUDE_CODE_SUBAGENT_MODEL_NAME",
        &haiku.upstream_model_id,
        &haiku.display_name,
        haiku.supports_1m,
    );
    let fallback = env_model(
        &settings,
        ClaudeModelRole::Fallback,
        "ANTHROPIC_MODEL",
        "ANTHROPIC_MODEL_NAME",
        &haiku.upstream_model_id,
        &haiku.display_name,
        haiku.supports_1m,
    );
    let mut models = vec![opus, fable, sonnet, haiku, subagent, fallback];
    apply_desktop_model_routes(&meta, &mut models);
    let profile = ClaudeConnectionProfile {
        // Importing data must never activate a listener or materialize live files.
        enabled: false,
        base_url: base_url.to_string(),
        auth_mode: ClaudeAuthMode::Bearer,
        api_format: ClaudeApiFormat::Anthropic,
        upstream_api_key: api_key.to_string(),
        models: models.clone(),
        last_test: None,
    }
    .normalize_for_storage()
    .ok()?;
    Some(CanonicalCandidate {
        preview: LegacyCanonicalCandidatePreview {
            source_app_type: provider.app_type.clone(),
            source_provider_id: provider.id.clone(),
            source_provider_name: provider.name.clone(),
            base_url: profile.base_url.clone(),
            has_api_key: true,
            masked_api_key: masked_secret(api_key),
            models: profile.models.clone(),
        },
        profile,
    })
}

fn choose_canonical_candidate(providers: &[LegacyProviderRow]) -> Option<CanonicalCandidate> {
    providers
        .iter()
        .filter(|provider| ALLOWED_APP_TYPES.contains(&provider.app_type.as_str()))
        .filter(|provider| provider.is_current)
        .chain(
            providers
                .iter()
                .filter(|provider| ALLOWED_APP_TYPES.contains(&provider.app_type.as_str()))
                .filter(|provider| !provider.is_current),
        )
        .find_map(candidate_from_provider)
}

fn excluded_counts(
    conn: &Connection,
    providers: &[LegacyProviderRow],
) -> Result<LegacyExcludedCounts, AppError> {
    let gateway_settings = if table_exists(conn, "settings")? {
        count_where(
            conn,
            "settings",
            "key IN ('agent_gateway_config', 'gateway_key', 'agent_gateway_key')",
        )?
    } else {
        0
    };
    let sync_settings = if table_exists(conn, "settings")? {
        count_where(
            conn,
            "settings",
            "key LIKE '%sync%' OR key LIKE 'webdav%' OR key LIKE 's3_%'",
        )?
    } else {
        0
    };
    let mut proxy_or_takeover_rows = 0;
    for table in ["proxy_config", "proxy_live_backup", "provider_health"] {
        proxy_or_takeover_rows += count_table(conn, table)?;
    }
    let mut history_or_usage_rows = 0;
    for table in [
        "proxy_request_logs",
        "usage_daily_rollups",
        "stream_check_logs",
        "session_log_sync",
    ] {
        history_or_usage_rows += count_table(conn, table)?;
    }
    Ok(LegacyExcludedCounts {
        other_agent_providers: providers
            .iter()
            .filter(|provider| !ALLOWED_APP_TYPES.contains(&provider.app_type.as_str()))
            .count(),
        local_gateway_providers: providers
            .iter()
            .filter(|provider| ALLOWED_APP_TYPES.contains(&provider.app_type.as_str()))
            .filter(|provider| provider_is_local_gateway(provider))
            .count(),
        gateway_settings,
        proxy_or_takeover_rows,
        sync_settings,
        history_or_usage_rows,
    })
}

fn sanitized_provider_meta(raw: &str) -> String {
    let Ok(mut meta) = serde_json::from_str::<Value>(raw) else {
        return "{}".to_string();
    };
    if let Some(object) = meta.as_object_mut() {
        for key in [
            "usage_script",
            "usageScript",
            "costMultiplier",
            "pricingModelSource",
            "limitDailyUsd",
            "limitMonthlyUsd",
            "liveConfigManaged",
        ] {
            object.remove(key);
        }
    }
    serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string())
}

fn strip_local_gateway_secrets(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.retain(|key, value| {
                let key_lower = key.to_ascii_lowercase();
                if key_lower == "agent_gateway_config"
                    || key_lower == "agentgatewayconfig"
                    || key_lower == "gateway_key"
                    || key_lower == "gatewaykey"
                    || key_lower == "gateway_token"
                    || key_lower == "gatewaytoken"
                {
                    return false;
                }
                !value.as_str().is_some_and(|value| {
                    LOCAL_GATEWAY_TOKEN_PREFIXES
                        .iter()
                        .any(|prefix| value.starts_with(prefix))
                })
            });
            for child in object.values_mut() {
                strip_local_gateway_secrets(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_local_gateway_secrets(item);
            }
        }
        _ => {}
    }
}

fn sanitized_provider_settings(raw: &str) -> String {
    let Ok(mut settings) = serde_json::from_str::<Value>(raw) else {
        return "{}".to_string();
    };
    strip_local_gateway_secrets(&mut settings);
    serde_json::to_string(&settings).unwrap_or_else(|_| "{}".to_string())
}

fn import_providers(
    source: &Connection,
    target: &Transaction<'_>,
    providers: &[LegacyProviderRow],
) -> Result<LegacyProviderCounts, AppError> {
    let mut counts = LegacyProviderCounts::default();
    for app_type in ALLOWED_APP_TYPES {
        if providers.iter().any(|provider| {
            provider.app_type == app_type
                && provider.is_current
                && !provider_is_local_gateway(provider)
        }) {
            target.execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
                [app_type],
            )?;
        }
    }
    for provider in providers.iter().filter(|provider| {
        ALLOWED_APP_TYPES.contains(&provider.app_type.as_str())
            && !provider_is_local_gateway(provider)
    }) {
        target.execute(
            "INSERT INTO providers (
                id, app_type, name, settings_config, website_url, category,
                created_at, sort_index, notes, icon, icon_color, meta,
                is_current, in_failover_queue
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0)
             ON CONFLICT(id, app_type) DO UPDATE SET
                name = excluded.name,
                settings_config = excluded.settings_config,
                website_url = excluded.website_url,
                category = excluded.category,
                created_at = excluded.created_at,
                sort_index = excluded.sort_index,
                notes = excluded.notes,
                icon = excluded.icon,
                icon_color = excluded.icon_color,
                meta = excluded.meta,
                is_current = excluded.is_current,
                in_failover_queue = 0",
            params![
                provider.id,
                provider.app_type,
                provider.name,
                sanitized_provider_settings(&provider.settings_config),
                provider.website_url,
                provider.category,
                provider.created_at,
                provider.sort_index,
                provider.notes,
                provider.icon,
                provider.icon_color,
                sanitized_provider_meta(&provider.meta),
                provider.is_current,
            ],
        )?;
        match provider.app_type.as_str() {
            "claude" => counts.claude += 1,
            "claude-desktop" => counts.claude_desktop += 1,
            _ => {}
        }
    }

    if table_exists(source, "provider_endpoints")? {
        for provider in providers.iter().filter(|provider| {
            ALLOWED_APP_TYPES.contains(&provider.app_type.as_str())
                && !provider_is_local_gateway(provider)
        }) {
            target.execute(
                "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
                params![provider.id, provider.app_type],
            )?;
        }
        let mut stmt = source.prepare(
            "SELECT e.provider_id, e.app_type, e.url, e.added_at
             FROM provider_endpoints e
             JOIN providers p ON p.id = e.provider_id AND p.app_type = e.app_type
             WHERE e.app_type IN ('claude', 'claude-desktop')",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        for row in rows {
            let (provider_id, app_type, url, added_at) = row?;
            if providers.iter().any(|provider| {
                provider.id == provider_id
                    && provider.app_type == app_type
                    && !provider_is_local_gateway(provider)
            }) {
                target.execute(
                    "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![provider_id, app_type, url, added_at],
                )?;
            }
        }
    }
    Ok(counts)
}

fn import_mcp_servers(source: &Connection, target: &Transaction<'_>) -> Result<usize, AppError> {
    let columns = table_columns(source, "mcp_servers")?;
    if !["id", "name", "server_config", "enabled_claude"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Ok(0);
    }
    let description = optional_column(&columns, "description", "NULL");
    let homepage = optional_column(&columns, "homepage", "NULL");
    let docs = optional_column(&columns, "docs", "NULL");
    let tags = optional_column(&columns, "tags", "'[]'");
    let sql = format!(
        "SELECT id, name, server_config, {description}, {homepage}, {docs}, {tags}
         FROM mcp_servers WHERE enabled_claude = 1"
    );
    let mut stmt = source.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    let mut count = 0;
    for row in rows {
        let (id, name, config, description, homepage, docs, tags) = row?;
        target.execute(
            "INSERT INTO mcp_servers (
                id, name, server_config, description, homepage, docs, tags,
                enabled_claude, enabled_codex, enabled_gemini, enabled_grokbuild,
                enabled_opencode, enabled_hermes
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 0, 0, 0, 0, 0)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, server_config = excluded.server_config,
                description = excluded.description, homepage = excluded.homepage,
                docs = excluded.docs, tags = excluded.tags, enabled_claude = 1,
                enabled_codex = 0, enabled_gemini = 0, enabled_grokbuild = 0,
                enabled_opencode = 0, enabled_hermes = 0",
            params![id, name, config, description, homepage, docs, tags],
        )?;
        count += 1;
    }
    Ok(count)
}

fn import_skills(source: &Connection, target: &Transaction<'_>) -> Result<usize, AppError> {
    let columns = table_columns(source, "skills")?;
    if !["id", "name", "directory", "enabled_claude"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Ok(0);
    }
    let description = optional_column(&columns, "description", "NULL");
    let repo_owner = optional_column(&columns, "repo_owner", "NULL");
    let repo_name = optional_column(&columns, "repo_name", "NULL");
    let repo_branch = optional_column(&columns, "repo_branch", "'main'");
    let readme_url = optional_column(&columns, "readme_url", "NULL");
    let installed_at = optional_column(&columns, "installed_at", "0");
    let content_hash = optional_column(&columns, "content_hash", "NULL");
    let updated_at = optional_column(&columns, "updated_at", "0");
    let sql = format!(
        "SELECT id, name, {description}, directory, {repo_owner}, {repo_name},
         {repo_branch}, {readme_url}, {installed_at}, {content_hash}, {updated_at}
         FROM skills WHERE enabled_claude = 1"
    );
    let mut stmt = source.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, i64>(10)?,
        ))
    })?;
    let mut count = 0;
    for row in rows {
        let (
            id,
            name,
            description,
            directory,
            owner,
            repo,
            branch,
            readme,
            installed,
            hash,
            updated,
        ) = row?;
        target.execute(
            "INSERT INTO skills (
                id, name, description, directory, repo_owner, repo_name, repo_branch,
                readme_url, enabled_claude, enabled_codex, enabled_gemini,
                enabled_grokbuild, enabled_opencode, enabled_hermes,
                installed_at, content_hash, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 0, 0, 0, 0, 0, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, description = excluded.description,
                directory = excluded.directory, repo_owner = excluded.repo_owner,
                repo_name = excluded.repo_name, repo_branch = excluded.repo_branch,
                readme_url = excluded.readme_url, enabled_claude = 1,
                enabled_codex = 0, enabled_gemini = 0, enabled_grokbuild = 0,
                enabled_opencode = 0, enabled_hermes = 0,
                installed_at = excluded.installed_at, content_hash = excluded.content_hash,
                updated_at = excluded.updated_at",
            params![
                id,
                name,
                description,
                directory,
                owner,
                repo,
                branch,
                readme,
                installed,
                hash,
                updated
            ],
        )?;
        count += 1;
    }
    Ok(count)
}

fn import_prompts(source: &Connection, target: &Transaction<'_>) -> Result<usize, AppError> {
    let columns = table_columns(source, "prompts")?;
    if !["id", "app_type", "name", "content"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Ok(0);
    }
    let description = optional_column(&columns, "description", "NULL");
    let enabled = optional_column(&columns, "enabled", "1");
    let created_at = optional_column(&columns, "created_at", "NULL");
    let updated_at = optional_column(&columns, "updated_at", "NULL");
    let sql = format!(
        "SELECT id, app_type, name, content, {description}, {enabled}, {created_at}, {updated_at}
         FROM prompts WHERE app_type IN ('claude', 'claude-desktop')"
    );
    let mut stmt = source.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, bool>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
        ))
    })?;
    let mut count = 0;
    for row in rows {
        let (id, app_type, name, content, description, enabled, created, updated) = row?;
        target.execute(
            "INSERT INTO prompts
             (id, app_type, name, content, description, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id, app_type) DO UPDATE SET
                name = excluded.name, content = excluded.content,
                description = excluded.description, enabled = excluded.enabled,
                created_at = excluded.created_at, updated_at = excluded.updated_at",
            params![
                id,
                app_type,
                name,
                content,
                description,
                enabled,
                created,
                updated
            ],
        )?;
        count += 1;
    }
    Ok(count)
}

fn sanitized_profile_payload(raw: &str) -> Option<String> {
    let input = serde_json::from_str::<Value>(raw).ok()?;
    let mut output = serde_json::Map::new();
    let mut has_claude_snapshot = false;
    for section in ["providers", "mcp", "skills", "prompts"] {
        let mut scoped = serde_json::Map::new();
        for app_type in ALLOWED_APP_TYPES {
            let value = input
                .get(section)
                .and_then(|value| value.get(app_type))
                .cloned()
                .unwrap_or(Value::Null);
            has_claude_snapshot |= !value.is_null();
            scoped.insert(app_type.to_string(), value);
        }
        output.insert(section.to_string(), Value::Object(scoped));
    }
    has_claude_snapshot.then(|| Value::Object(output).to_string())
}

fn count_importable_profiles(source: &Connection) -> Result<usize, AppError> {
    let columns = table_columns(source, "profiles")?;
    if !columns.contains("payload") {
        return Ok(0);
    }
    let mut stmt = source.prepare("SELECT payload FROM profiles")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut count = 0;
    for row in rows {
        if sanitized_profile_payload(&row?).is_some() {
            count += 1;
        }
    }
    Ok(count)
}

fn import_profiles(
    source: &Connection,
    target: &Transaction<'_>,
) -> Result<(usize, HashSet<String>), AppError> {
    let columns = table_columns(source, "profiles")?;
    if !["id", "name", "payload"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Ok((0, HashSet::new()));
    }
    let sort_order = optional_column(&columns, "sort_order", "NULL");
    let created_at = optional_column(&columns, "created_at", "NULL");
    let updated_at = optional_column(&columns, "updated_at", "NULL");
    let sql =
        format!("SELECT id, name, payload, {sort_order}, {created_at}, {updated_at} FROM profiles");
    let mut stmt = source.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
        ))
    })?;
    let mut ids = HashSet::new();
    for row in rows {
        let (id, name, payload, sort_order, created_at, updated_at) = row?;
        let Some(payload) = sanitized_profile_payload(&payload) else {
            continue;
        };
        target.execute(
            "INSERT INTO profiles (id, name, payload, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, payload = excluded.payload,
                sort_order = excluded.sort_order, created_at = excluded.created_at,
                updated_at = excluded.updated_at",
            params![id, name, payload, sort_order, created_at, updated_at],
        )?;
        ids.insert(id);
    }
    Ok((ids.len(), ids))
}

fn import_current_profile_selections(
    source: &Connection,
    target: &Transaction<'_>,
    imported_profile_ids: &HashSet<String>,
) -> Result<(), AppError> {
    if !table_exists(source, "settings")? {
        return Ok(());
    }
    for key in [
        "current_profile_id_claude",
        "current_profile_id_claude-desktop",
    ] {
        let value = source
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten();
        if let Some(value) = value.filter(|id| imported_profile_ids.contains(id)) {
            target.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
    }
    Ok(())
}

fn is_default_canonical_profile(raw: &str) -> bool {
    let Ok(existing) = serde_json::from_str::<ClaudeConnectionProfile>(raw) else {
        return false;
    };
    let Ok(existing) = existing.normalize_for_storage() else {
        return false;
    };
    let default = ClaudeConnectionProfile::default()
        .normalize_for_storage()
        .expect("built-in canonical profile must be valid");
    existing == default
}

fn import_canonical_candidate(
    target: &Transaction<'_>,
    candidate: Option<&CanonicalCandidate>,
    options: &LegacyImportOptions,
) -> Result<LegacyCanonicalImportResult, AppError> {
    let Some(candidate) = candidate else {
        return Ok(LegacyCanonicalImportResult {
            imported: false,
            replaced_existing: false,
            reason: "旧数据库中没有可安全使用的远程 Claude 凭据".to_string(),
            candidate: None,
        });
    };
    let setting_key =
        crate::agent_gateway::connection_profile::CLAUDE_CONNECTION_PROFILE_SETTING_KEY;
    let existing = target
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [setting_key],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let existing_is_default = existing
        .as_deref()
        .is_some_and(is_default_canonical_profile);
    let may_import = options.replace_canonical_profile || existing.is_none() || existing_is_default;
    if !may_import {
        return Ok(LegacyCanonicalImportResult {
            imported: false,
            replaced_existing: false,
            reason: "已保留目标数据库中现有的非默认 Claude 连接配置".to_string(),
            candidate: Some(candidate.preview.clone()),
        });
    }
    let raw = serde_json::to_string(&candidate.profile)
        .map_err(|error| AppError::Config(format!("旧 Claude 连接配置序列化失败: {error}")))?;
    target.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![setting_key, raw],
    )?;
    Ok(LegacyCanonicalImportResult {
        imported: true,
        replaced_existing: existing.is_some() && !existing_is_default,
        reason: "已导入为禁用状态；用户确认启用前不会写入 Live 配置".to_string(),
        candidate: Some(candidate.preview.clone()),
    })
}

pub fn execute_legacy_database_import(
    path: &Path,
    target_db: &Database,
    options: LegacyImportOptions,
) -> Result<LegacyImportResult, AppError> {
    let source = open_legacy_read_only(path)?;
    let providers = load_legacy_providers(&source)?;
    let candidate = choose_canonical_candidate(&providers);
    let excluded = excluded_counts(&source, &providers)?;
    let mut target_conn = lock_conn!(target_db.conn);
    let tx = target_conn.transaction()?;
    let provider_counts = import_providers(&source, &tx, &providers)?;
    let mcp_servers = import_mcp_servers(&source, &tx)?;
    let skills = import_skills(&source, &tx)?;
    let prompts = import_prompts(&source, &tx)?;
    let (profiles, profile_ids) = import_profiles(&source, &tx)?;
    import_current_profile_selections(&source, &tx, &profile_ids)?;
    let canonical_profile = import_canonical_candidate(&tx, candidate.as_ref(), &options)?;
    tx.commit()?;
    let mut warnings = Vec::new();
    if excluded.local_gateway_providers > 0 {
        warnings.push(format!(
            "已跳过 {} 个指向本机的旧 Gateway 供应商及其本地密钥",
            excluded.local_gateway_providers
        ));
    }
    if !canonical_profile.imported {
        warnings.push(canonical_profile.reason.clone());
    }
    Ok(LegacyImportResult {
        imported: LegacyImportCounts {
            providers: provider_counts,
            mcp_servers,
            skills,
            prompts,
            profiles,
        },
        excluded,
        canonical_profile,
        live_configuration_touched: false,
        warnings,
    })
}

pub fn preview_legacy_database(path: &Path) -> Result<LegacyImportPreview, AppError> {
    let conn = open_legacy_read_only(path)?;
    let providers = load_legacy_providers(&conn)?;
    let provider_counts = LegacyProviderCounts {
        claude: providers
            .iter()
            .filter(|provider| {
                provider.app_type == "claude" && !provider_is_local_gateway(provider)
            })
            .count(),
        claude_desktop: providers
            .iter()
            .filter(|provider| {
                provider.app_type == "claude-desktop" && !provider_is_local_gateway(provider)
            })
            .count(),
    };
    let canonical_candidate = choose_canonical_candidate(&providers);
    let mut warnings = Vec::new();
    if provider_counts.claude + provider_counts.claude_desktop == 0 {
        warnings.push("旧数据库中没有 Claude Code 或 Claude Desktop 供应商".to_string());
    } else if canonical_candidate.is_none() {
        warnings.push(
            "未找到可安全提升为统一连接配置的远程 Claude 凭据；本地 Gateway Key 不会被候选化"
                .to_string(),
        );
    }
    let mcp_servers = if table_columns(&conn, "mcp_servers")?.contains("enabled_claude") {
        count_where(&conn, "mcp_servers", "enabled_claude = 1")?
    } else {
        0
    };
    let skills = if table_columns(&conn, "skills")?.contains("enabled_claude") {
        count_where(&conn, "skills", "enabled_claude = 1")?
    } else {
        0
    };
    let prompts = if table_columns(&conn, "prompts")?.contains("app_type") {
        count_where(&conn, "prompts", "app_type IN ('claude', 'claude-desktop')")?
    } else {
        0
    };
    let profiles = count_importable_profiles(&conn)?;
    let excluded = excluded_counts(&conn, &providers)?;
    Ok(LegacyImportPreview {
        providers: provider_counts,
        mcp_servers,
        skills,
        prompts,
        profiles,
        excluded,
        canonical_candidate: canonical_candidate.map(|candidate| candidate.preview),
        warnings,
    })
}

/// Preview a user-selected legacy database. The source is opened with SQLite
/// read-only + query-only protections and no target state is required.
#[tauri::command]
pub async fn preview_legacy_cc_switch_import(
    #[allow(non_snake_case)] filePath: String,
) -> Result<LegacyImportPreview, String> {
    tauri::async_runtime::spawn_blocking(move || preview_legacy_database(Path::new(&filePath)))
        .await
        .map_err(|error| format!("预览旧 CC Switch 数据失败: {error}"))?
        .map_err(|error| error.to_string())
}

/// Import only the previewed Claude-owned records into CC Gateway's database.
/// This command intentionally does not accept an AppHandle and never invokes
/// provider/MCP/Skill/Prompt services, so it cannot materialize live files.
#[tauri::command]
pub async fn import_legacy_cc_switch_database(
    #[allow(non_snake_case)] filePath: String,
    options: LegacyImportOptions,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<LegacyImportResult, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        execute_legacy_database_import(Path::new(&filePath), &db, options)
    })
    .await
    .map_err(|error| format!("导入旧 CC Switch 数据失败: {error}"))?
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};
    use serde_json::json;
    use tempfile::NamedTempFile;

    fn legacy_database() -> NamedTempFile {
        let file = NamedTempFile::new().expect("legacy db temp file");
        let conn = Connection::open(file.path()).expect("open legacy db");
        conn.execute_batch(
            "CREATE TABLE providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                icon TEXT,
                icon_color TEXT,
                meta TEXT NOT NULL DEFAULT '{}',
                is_current BOOLEAN NOT NULL DEFAULT 0,
                in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
                PRIMARY KEY (id, app_type)
            );
            CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE mcp_servers (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL,
                description TEXT, homepage TEXT, docs TEXT, tags TEXT NOT NULL DEFAULT '[]',
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
                enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
                enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
                enabled_hermes BOOLEAN NOT NULL DEFAULT 0
            );
            CREATE TABLE prompts (
                id TEXT NOT NULL, app_type TEXT NOT NULL, name TEXT NOT NULL,
                content TEXT NOT NULL, description TEXT, enabled BOOLEAN NOT NULL DEFAULT 1,
                created_at INTEGER, updated_at INTEGER, PRIMARY KEY (id, app_type)
            );
            CREATE TABLE skills (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT,
                directory TEXT NOT NULL, repo_owner TEXT, repo_name TEXT,
                repo_branch TEXT DEFAULT 'main', readme_url TEXT,
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
                enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
                enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
                enabled_hermes BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0,
                content_hash TEXT, updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE profiles (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, payload TEXT NOT NULL,
                sort_order INTEGER, created_at INTEGER, updated_at INTEGER
            );
            CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY);
            CREATE TABLE session_log_sync (file_path TEXT PRIMARY KEY);",
        )
        .expect("create legacy schema");

        let claude = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://anyrouter.example",
                "ANTHROPIC_AUTH_TOKEN": "upstream-test-token",
                "LEGACY_GATEWAY_TOKEN": "ccs-agent-shadow-secret",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-8[1M]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-fable-5[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-opus-4-6[1M]"
            }
        });
        conn.execute(
            "INSERT INTO providers
             (id, app_type, name, settings_config, meta, is_current)
             VALUES (?1, 'claude', 'AnyRouter', ?2, '{}', 1)",
            params!["claude-anyrouter", claude.to_string()],
        )
        .expect("insert Claude provider");
        conn.execute(
            "INSERT INTO providers
             (id, app_type, name, settings_config, meta, is_current)
             VALUES (?1, 'codex', 'Must not import', ?2, '{}', 1)",
            params![
                "codex-secret",
                json!({"auth": {"OPENAI_API_KEY": "codex-secret-token"}}).to_string()
            ],
        )
        .expect("insert excluded provider");
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('agent_gateway_config', ?1)",
            params![json!({"token": "ccs-agent-test-only", "enabled": true}).to_string()],
        )
        .expect("insert excluded gateway key");
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('webdav_sync', 'sync-secret-token')",
            [],
        )
        .expect("insert excluded sync state");
        conn.execute(
            "INSERT INTO mcp_servers
             (id, name, server_config, tags, enabled_claude, enabled_codex)
             VALUES ('shared-mcp', 'Shared MCP', '{}', '[]', 1, 1),
                    ('codex-only-mcp', 'Codex MCP', '{}', '[]', 0, 1)",
            [],
        )
        .expect("insert legacy MCP servers");
        conn.execute(
            "INSERT INTO prompts (id, app_type, name, content, enabled)
             VALUES ('claude-prompt', 'claude', 'Claude Prompt', 'claude text', 1),
                    ('codex-prompt', 'codex', 'Codex Prompt', 'codex text', 1)",
            [],
        )
        .expect("insert legacy prompts");
        conn.execute(
            "INSERT INTO skills
             (id, name, directory, enabled_claude, enabled_codex, installed_at)
             VALUES ('shared-skill', 'Shared Skill', '/legacy/shared', 1, 1, 1),
                    ('codex-only-skill', 'Codex Skill', '/legacy/codex', 0, 1, 1)",
            [],
        )
        .expect("insert legacy skills");
        conn.execute(
            "INSERT INTO profiles (id, name, payload)
             VALUES ('mixed-profile', 'Mixed', ?1),
                    ('codex-profile', 'Codex only', ?2)",
            params![
                json!({
                    "providers": {"claude": "claude-anyrouter", "codex": "codex-secret"},
                    "mcp": {"claude": ["shared-mcp"], "codex": ["codex-only-mcp"]},
                    "skills": {"claude": ["shared-skill"], "codex": ["codex-only-skill"]},
                    "prompts": {"claude": "claude-prompt", "codex": "codex-prompt"}
                })
                .to_string(),
                json!({"providers": {"codex": "codex-secret"}}).to_string()
            ],
        )
        .expect("insert legacy profiles");
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('current_profile_id_claude', 'mixed-profile')",
            [],
        )
        .expect("insert Claude profile selection");
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('current_profile_id_codex', 'codex-profile')",
            [],
        )
        .expect("insert excluded Codex profile selection");
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id) VALUES ('history-secret-token')",
            [],
        )
        .expect("insert excluded history");
        conn.execute(
            "INSERT INTO session_log_sync (file_path) VALUES ('/legacy/session.jsonl')",
            [],
        )
        .expect("insert excluded session sync");
        file
    }

    #[test]
    fn preview_contains_only_claude_data_and_never_exposes_secrets() {
        let source = legacy_database();

        let preview = super::preview_legacy_database(source.path()).expect("preview succeeds");

        assert_eq!(preview.providers.claude, 1);
        assert_eq!(preview.providers.claude_desktop, 0);
        assert_eq!(preview.excluded.other_agent_providers, 1);
        let candidate = preview
            .canonical_candidate
            .as_ref()
            .expect("remote Claude provider produces candidate");
        assert!(candidate.has_api_key);
        assert_eq!(candidate.base_url, "https://anyrouter.example");

        let serialized = serde_json::to_string(&preview).expect("preview serializes");
        assert!(!serialized.contains("upstream-test-token"));
        assert!(!serialized.contains("codex-secret-token"));
        assert!(!serialized.contains("ccs-agent-test-only"));
        assert!(!serialized.contains("sync-secret-token"));
    }

    #[test]
    fn execute_imports_claude_records_only_and_keeps_live_configuration_untouched() {
        let source = legacy_database();
        let target = crate::database::Database::memory().expect("target db");

        let result = super::execute_legacy_database_import(
            source.path(),
            &target,
            super::LegacyImportOptions::default(),
        )
        .expect("selective import succeeds");

        assert_eq!(result.imported.providers.claude, 1);
        assert_eq!(result.imported.mcp_servers, 1);
        assert_eq!(result.imported.skills, 1);
        assert_eq!(result.imported.prompts, 1);
        assert_eq!(result.imported.profiles, 1);
        assert!(result.canonical_profile.imported);
        assert!(!result.live_configuration_touched);

        assert!(target
            .get_all_providers("claude")
            .expect("Claude providers")
            .contains_key("claude-anyrouter"));
        let imported_provider = target
            .get_provider_by_id("claude-anyrouter", "claude")
            .expect("provider lookup")
            .expect("provider imported");
        assert!(!imported_provider
            .settings_config
            .to_string()
            .contains("ccs-agent-shadow-secret"));
        assert!(!target
            .get_all_providers("codex")
            .expect("Codex providers")
            .contains_key("codex-secret"));

        let mcp = target.get_all_mcp_servers().expect("MCP servers");
        assert_eq!(mcp.len(), 1);
        let imported_mcp = mcp.get("shared-mcp").expect("shared MCP imported");
        assert!(imported_mcp.apps.claude);
        assert!(!imported_mcp.apps.codex);

        let skills = target.get_all_installed_skills().expect("installed skills");
        assert_eq!(skills.len(), 1);
        let imported_skill = skills.get("shared-skill").expect("shared skill imported");
        assert!(imported_skill.apps.claude);
        assert!(!imported_skill.apps.codex);

        assert_eq!(
            target.get_prompts("claude").expect("Claude prompts").len(),
            1
        );
        assert!(target
            .get_prompts("codex")
            .expect("Codex prompts")
            .is_empty());
        let profiles = target.get_all_profiles().expect("profiles");
        assert_eq!(profiles.len(), 1);
        assert!(!profiles[0].payload.contains("codex-secret"));
        assert!(!profiles[0].payload.contains("codex-only"));
        assert_eq!(
            target
                .get_current_profile_id("claude")
                .expect("Claude profile selection"),
            Some("mixed-profile".to_string())
        );
        assert_eq!(
            target
                .get_current_profile_id("codex")
                .expect("Codex profile selection"),
            None
        );

        let stored_profile = target
            .get_setting(
                crate::agent_gateway::connection_profile::CLAUDE_CONNECTION_PROFILE_SETTING_KEY,
            )
            .expect("canonical setting")
            .expect("canonical imported");
        assert!(stored_profile.contains("upstream-test-token"));
        assert!(!stored_profile.contains("ccs-agent-test-only"));
        assert!(target
            .get_setting(crate::agent_gateway::AGENT_GATEWAY_CONFIG_KEY)
            .expect("gateway setting lookup")
            .is_none());
        assert!(target
            .get_setting("webdav_sync")
            .expect("sync lookup")
            .is_none());
    }

    #[test]
    fn execute_skips_local_gateway_provider_and_does_not_modify_source_database() {
        let source = legacy_database();
        let conn = Connection::open(source.path()).expect("reopen source");
        conn.execute(
            "INSERT INTO providers
             (id, app_type, name, settings_config, meta, is_current)
             VALUES ('legacy-local-gateway', 'claude-desktop', 'Old gateway', ?1, '{}', 0)",
            params![json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721/agent/v1",
                    "ANTHROPIC_AUTH_TOKEN": "ccs-agent-local-secret"
                }
            })
            .to_string()],
        )
        .expect("insert old local gateway provider");
        drop(conn);
        let before = std::fs::read(source.path()).expect("read source before import");
        let target = crate::database::Database::memory().expect("target db");

        let result = super::execute_legacy_database_import(
            source.path(),
            &target,
            super::LegacyImportOptions::default(),
        )
        .expect("selective import succeeds");

        let after = std::fs::read(source.path()).expect("read source after import");
        assert_eq!(
            after, before,
            "source database must remain byte-for-byte unchanged"
        );
        assert_eq!(result.excluded.local_gateway_providers, 1);
        assert!(!target
            .get_all_providers("claude-desktop")
            .expect("desktop providers")
            .contains_key("legacy-local-gateway"));
        assert!(!serde_json::to_string(&result)
            .expect("result serializes")
            .contains("ccs-agent-local-secret"));
        assert!(target
            .get_setting(crate::agent_gateway::AGENT_GATEWAY_CONFIG_KEY)
            .expect("gateway setting lookup")
            .is_none());
    }

    #[test]
    fn canonical_profile_requires_explicit_replace_when_target_is_already_configured() {
        let source = legacy_database();
        let target = crate::database::Database::memory().expect("target db");
        let setting_key =
            crate::agent_gateway::connection_profile::CLAUDE_CONNECTION_PROFILE_SETTING_KEY;
        let mut existing =
            crate::agent_gateway::connection_profile::ClaudeConnectionProfile::default();
        existing.base_url = "https://existing.example".to_string();
        existing.upstream_api_key = "existing-target-token".to_string();
        let existing_raw = serde_json::to_string(&existing).expect("existing profile serializes");
        target
            .set_setting(setting_key, &existing_raw)
            .expect("seed existing canonical profile");

        let protected = super::execute_legacy_database_import(
            source.path(),
            &target,
            super::LegacyImportOptions::default(),
        )
        .expect("protected import succeeds");
        assert!(!protected.canonical_profile.imported);
        assert_eq!(
            target
                .get_setting(setting_key)
                .expect("read protected profile")
                .expect("profile remains"),
            existing_raw
        );

        let replaced = super::execute_legacy_database_import(
            source.path(),
            &target,
            super::LegacyImportOptions {
                replace_canonical_profile: true,
            },
        )
        .expect("explicit replacement succeeds");
        assert!(replaced.canonical_profile.imported);
        assert!(replaced.canonical_profile.replaced_existing);
        let imported_raw = target
            .get_setting(setting_key)
            .expect("read imported profile")
            .expect("imported profile exists");
        let imported: crate::agent_gateway::connection_profile::ClaudeConnectionProfile =
            serde_json::from_str(&imported_raw).expect("imported profile parses");
        assert_eq!(imported.base_url, "https://anyrouter.example");
        assert_eq!(imported.upstream_api_key, "upstream-test-token");
        assert!(
            !imported.enabled,
            "import must never auto-enable the runtime"
        );
    }

    #[test]
    #[serial_test::serial]
    fn execute_does_not_write_claude_code_or_desktop_live_files() {
        struct EnvGuard(Option<std::ffi::OsString>);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                    None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
                }
            }
        }

        let source = legacy_database();
        let target = crate::database::Database::memory().expect("target db");
        let test_home = tempfile::tempdir().expect("test home");
        let _guard = EnvGuard(std::env::var_os("CC_SWITCH_TEST_HOME"));
        std::env::set_var("CC_SWITCH_TEST_HOME", test_home.path());

        let claude_settings = test_home.path().join(".claude/settings.json");
        let claude_mcp = test_home.path().join(".claude.json");
        let desktop_profile = test_home
            .path()
            .join("Library/Application Support/Claude-3p/config.json");
        for path in [&claude_settings, &claude_mcp, &desktop_profile] {
            std::fs::create_dir_all(path.parent().expect("sentinel parent"))
                .expect("create sentinel parent");
            std::fs::write(path, b"live-sentinel-must-not-change").expect("write live sentinel");
        }

        let result = super::execute_legacy_database_import(
            source.path(),
            &target,
            super::LegacyImportOptions::default(),
        )
        .expect("selective import succeeds");

        assert!(!result.live_configuration_touched);
        for path in [&claude_settings, &claude_mcp, &desktop_profile] {
            assert_eq!(
                std::fs::read(path).expect("read live sentinel"),
                b"live-sentinel-must-not-change"
            );
        }
    }

    #[test]
    fn preview_promotes_claude_desktop_model_routes_into_canonical_catalog() {
        let source = legacy_database();
        let conn = Connection::open(source.path()).expect("reopen source");
        conn.execute("DELETE FROM providers", [])
            .expect("remove existing providers");
        let settings = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://anyrouter.example",
                "ANTHROPIC_AUTH_TOKEN": "desktop-upstream-token"
            }
        });
        let meta = json!({
            "claudeDesktopModelRoutes": {
                "desktop-opus": {
                    "model": "claude-opus-4-8",
                    "labelOverride": "Opus 4.8 via Desktop",
                    "supports1m": true
                },
                "desktop-fable": {
                    "model": "claude-fable-5",
                    "labelOverride": "Fable 5 via Desktop",
                    "supports1m": true
                },
                "desktop-compact": {
                    "model": "claude-opus-4-6",
                    "labelOverride": "Opus 4.6 via Desktop",
                    "supports1m": false
                }
            }
        });
        conn.execute(
            "INSERT INTO providers
             (id, app_type, name, settings_config, meta, is_current)
             VALUES ('desktop-anyrouter', 'claude-desktop', 'Desktop AnyRouter', ?1, ?2, 1)",
            params![settings.to_string(), meta.to_string()],
        )
        .expect("insert desktop provider");
        drop(conn);

        let preview = super::preview_legacy_database(source.path()).expect("preview succeeds");
        let candidate = preview
            .canonical_candidate
            .expect("desktop provider produces canonical candidate");
        assert_eq!(candidate.source_app_type, "claude-desktop");
        let route = |role| {
            candidate
                .models
                .iter()
                .find(|model| model.role == role)
                .expect("role exists")
        };
        assert_eq!(
            route(crate::agent_gateway::connection_profile::ClaudeModelRole::Opus)
                .upstream_model_id,
            "claude-opus-4-8"
        );
        assert_eq!(
            route(crate::agent_gateway::connection_profile::ClaudeModelRole::Fable).client_model_id,
            "desktop-fable"
        );
        assert_eq!(
            route(crate::agent_gateway::connection_profile::ClaudeModelRole::Sonnet)
                .upstream_model_id,
            "claude-fable-5"
        );
        assert_eq!(
            route(crate::agent_gateway::connection_profile::ClaudeModelRole::Subagent)
                .upstream_model_id,
            "claude-opus-4-6"
        );
    }
}
