mod agent_gateway;
mod app_config;
mod app_store;
mod auto_launch;
mod claude_desktop_config;
mod claude_mcp;
mod claude_plugin;
mod claude_runtime;
mod codex_config;
mod codex_history_migration;
mod codex_state_db;
mod commands;
mod config;
mod database;
mod deeplink;
mod error;
mod gemini_config;
mod gemini_mcp;
mod grok_config;
pub mod hermes_config;
mod init_status;
mod lightweight;
#[cfg(target_os = "linux")]
mod linux_fix;
mod mcp;
mod model_capabilities;
mod openclaw_config;
mod opencode_config;
mod panic_hook;
mod product_scope;
mod prompt;
mod prompt_files;
mod provider;
mod provider_defaults;
mod proxy;
mod services;
mod session_manager;
mod settings;
mod store;

mod tray;
mod usage_events;
mod usage_script;

pub use app_config::{AppType, InstalledSkill, McpApps, McpServer, MultiAppConfig, SkillApps};
pub use codex_config::{get_codex_auth_path, get_codex_config_path, write_codex_live_atomic};
pub use commands::open_provider_terminal;
pub use commands::*;
pub use config::{get_claude_mcp_path, get_claude_settings_path, read_json_file};
pub use database::{Database, Profile};
pub use deeplink::{import_provider_from_deeplink, parse_deeplink_url, DeepLinkImportRequest};
pub use error::AppError;
pub use mcp::{
    import_from_claude, import_from_codex, import_from_gemini, import_from_grokbuild,
    remove_server_from_claude, remove_server_from_codex, remove_server_from_gemini,
    remove_server_from_grokbuild, sync_enabled_to_claude, sync_enabled_to_codex,
    sync_enabled_to_gemini, sync_single_server_to_claude, sync_single_server_to_codex,
    sync_single_server_to_gemini, sync_single_server_to_grokbuild,
};
pub use prompt::Prompt;
pub use provider::{Provider, ProviderMeta};
pub use services::{
    profile::{ProfilePayload, ProfileScope, ProfileService},
    provider::reapply_current_codex_official_live,
    skill::{migrate_skills_to_ssot, ImportSkillSelection},
    ConfigService, EndpointLatency, McpService, PromptService, ProviderService, ProxyService,
    SkillService, SpeedtestService,
};
pub use settings::{update_settings, AppSettings};
pub use store::AppState;
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use std::sync::Arc;
#[cfg(target_os = "macos")]
use tauri::image::Image;
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::RunEvent;
use tauri::{Emitter, Manager};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

#[cfg(target_os = "windows")]
fn set_windows_app_user_model_id(app: &tauri::AppHandle) {
    let app_id = app.config().identifier.clone();
    let wide_app_id: Vec<u16> = app_id.encode_utf16().chain(std::iter::once(0)).collect();

    let result = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(wide_app_id.as_ptr())
    };

    if result < 0 {
        log::warn!("设置 Windows AppUserModelID 失败: 0x{result:08X}");
    } else {
        log::debug!("Windows AppUserModelID 已设置为 {app_id}");
    }
}

fn redact_url_for_log(url_str: &str) -> String {
    match url::Url::parse(url_str) {
        Ok(url) => {
            let mut output = format!("{}://", url.scheme());
            if let Some(host) = url.host_str() {
                output.push_str(host);
            }
            output.push_str(url.path());

            let mut keys: Vec<String> = url.query_pairs().map(|(k, _)| k.to_string()).collect();
            keys.sort();
            keys.dedup();

            if !keys.is_empty() {
                output.push_str("?[keys:");
                output.push_str(&keys.join(","));
                output.push(']');
            }

            output
        }
        Err(_) => {
            let base = url_str.split('#').next().unwrap_or(url_str);
            match base.split_once('?') {
                Some((prefix, _)) => format!("{prefix}?[redacted]"),
                None => base.to_string(),
            }
        }
    }
}

/// 统一处理 ccgateway:// 深链接 URL
///
/// - 解析 URL
/// - 向前端发射 `deeplink-import` / `deeplink-error` 事件
/// - 可选：在成功时聚焦主窗口
fn handle_deeplink_url(
    app: &tauri::AppHandle,
    url_str: &str,
    focus_main_window: bool,
    source: &str,
) -> bool {
    if !url_str.starts_with("ccgateway://") {
        return false;
    }

    let redacted_url = redact_url_for_log(url_str);
    log::info!("✓ Deep link URL detected from {source}: {redacted_url}");
    log::debug!("Deep link URL (raw) from {source}: {url_str}");

    match crate::deeplink::parse_deeplink_url(url_str) {
        Ok(request) => {
            log::info!(
                "✓ Successfully parsed deep link: resource={}, app={:?}, name={:?}",
                request.resource,
                request.app,
                request.name
            );

            if let Err(e) = app.emit("deeplink-import", &request) {
                log::error!("✗ Failed to emit deeplink-import event: {e}");
            } else {
                log::info!("✓ Emitted deeplink-import event to frontend");
            }

            if focus_main_window {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                    #[cfg(target_os = "linux")]
                    {
                        linux_fix::nudge_main_window(window.clone());
                    }
                    log::info!("✓ Window shown and focused");
                }
            }
        }
        Err(e) => {
            log::error!("✗ Failed to parse deep link URL: {e}");

            if let Err(emit_err) = app.emit(
                "deeplink-error",
                serde_json::json!({
                    "url": url_str,
                    "error": e.to_string()
                }),
            ) {
                log::error!("✗ Failed to emit deeplink-error event: {emit_err}");
            }
        }
    }

    true
}

/// 更新托盘菜单的Tauri命令
#[tauri::command]
async fn update_tray_menu(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    match tray::create_tray_menu(&app, state.inner()) {
        Ok(new_menu) => {
            if let Some(tray) = app.tray_by_id(tray::TRAY_ID) {
                tray.set_menu(Some(new_menu))
                    .map_err(|e| format!("更新托盘菜单失败: {e}"))?;
                return Ok(true);
            }
            Ok(false)
        }
        Err(err) => {
            log::error!("创建托盘菜单失败: {err}");
            Ok(false)
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_tray_icon() -> Option<Image<'static>> {
    const ICON_BYTES: &[u8] = include_bytes!("../icons/tray/macos/statusbar_template_3x.png");

    match Image::from_bytes(ICON_BYTES) {
        Ok(icon) => Some(icon),
        Err(err) => {
            log::warn!("Failed to load macOS tray icon: {err}");
            None
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 设置 panic hook，在应用崩溃时记录日志到 <app_config_dir>/crash.log（默认 ~/.cc-gateway/crash.log）
    panic_hook::setup_panic_hook();

    let mut builder = tauri::Builder::default();

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            log::info!("=== Single Instance Callback Triggered ===");
            log::debug!("Args count: {}", args.len());
            for (i, arg) in args.iter().enumerate() {
                log::debug!("  arg[{i}]: {}", redact_url_for_log(arg));
            }

            if crate::lightweight::is_lightweight_mode() {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式重建窗口失败: {e}");
                }
            }

            // Check for deep link URL in args (mainly for Windows/Linux command line)
            let mut found_deeplink = false;
            for arg in &args {
                if handle_deeplink_url(app, arg, false, "single_instance args") {
                    found_deeplink = true;
                    break;
                }
            }

            if !found_deeplink {
                log::info!("ℹ No deep link URL found in args (this is expected on macOS when launched via system)");
            }

            // Show and focus window regardless
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
                #[cfg(target_os = "linux")]
                {
                    linux_fix::nudge_main_window(window.clone());
                }
            }
        }));
    }

    let builder = builder
        // 注册 deep-link 插件（处理 macOS AppleEvent 和其他平台的深链接）
        .plugin(tauri_plugin_deep_link::init())
        // 拦截窗口关闭：根据设置决定是否最小化到托盘
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 数据库版本过新的恢复模式下没有托盘可唤回，关闭即退出，避免应用隐身后台
                let in_db_recovery = crate::init_status::get_init_error()
                    .map(|p| p.kind.as_deref() == Some("db_version_too_new"))
                    .unwrap_or(false);
                if in_db_recovery {
                    api.prevent_close();
                    window.app_handle().exit(0);
                    return;
                }

                let settings = crate::settings::get_settings();

                if settings.minimize_to_tray_on_close {
                    api.prevent_close();
                    let _ = window.hide();
                    #[cfg(target_os = "windows")]
                    {
                        let _ = window.set_skip_taskbar(true);
                    }
                    #[cfg(target_os = "macos")]
                    {
                        tray::apply_tray_policy(window.app_handle(), false);
                    }
                } else {
                    api.prevent_close();
                    window.app_handle().exit(0);
                }
            }
        })
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .build(),
        )
        .setup(|app| {
            let _ = rustls::crypto::ring::default_provider().install_default();

            // 预先刷新 Store 覆盖配置，确保后续路径读取正确（日志/数据库等）
            app_store::refresh_app_config_dir_override(app.handle());
            panic_hook::init_app_config_dir(crate::config::get_app_config_dir());
            #[cfg(target_os = "windows")]
            set_windows_app_user_model_id(app.handle());

            // 初始化日志（单文件输出到 <app_config_dir>/logs/cc-gateway.log）
            {
                use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};

                let log_dir = panic_hook::get_log_dir();

                // 确保日志目录存在
                if let Err(e) = std::fs::create_dir_all(&log_dir) {
                    eprintln!("创建日志目录失败: {e}");
                }

                // 启动时删除旧日志文件，实现单文件覆盖效果
                let log_file_path = log_dir.join(crate::config::APP_LOG_FILENAME);
                let _ = std::fs::remove_file(&log_file_path);

                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        // 初始化为 Trace，允许后续通过 log::set_max_level() 动态调整级别
                        .level(log::LevelFilter::Trace)
                        .targets([
                            Target::new(TargetKind::Stdout),
                            Target::new(TargetKind::Folder {
                                path: log_dir,
                                file_name: Some("cc-gateway".into()),
                            }),
                        ])
                        // 单文件模式：启动时删除旧文件，达到大小时轮转
                        // 注意：KeepSome(n) 内部会做 n-2 运算，n=1 会导致 usize 下溢
                        // KeepSome(2) 是最小安全值，表示不保留轮转文件
                        .rotation_strategy(RotationStrategy::KeepSome(2))
                        // 单文件大小限制 1GB
                        .max_file_size(1024 * 1024 * 1024)
                        .timezone_strategy(TimezoneStrategy::UseLocal)
                        .build(),
                )?;
            }

            // 注入 AppHandle 给 usage_events，让无 AppHandle 持有的写日志路径
            // 也能向前端推送 `usage-log-recorded`。
            // 放在日志系统初始化之后，确保 init 的日志能正常输出。
            usage_events::init(app.handle().clone());

            // 初始化数据库
            let app_config_dir = crate::config::get_app_config_dir();
            let db_path = app_config_dir.join(crate::config::APP_DATABASE_FILENAME);
            let json_path = app_config_dir.join("config.json");

            // 检查是否需要从 config.json 迁移到 SQLite
            let has_json = json_path.exists();
            let has_db = db_path.exists();

            // 如果需要迁移，先验证 config.json 是否可以加载（在创建数据库之前）
            // 这样如果加载失败用户选择退出，数据库文件还没被创建，下次可以正常重试
            let migration_config = if !has_db && has_json {
                log::info!("检测到旧版配置文件，验证配置文件...");

                // 循环：支持用户重试加载配置文件
                loop {
                    match crate::app_config::MultiAppConfig::load() {
                        Ok(config) => {
                            log::info!("✓ 配置文件加载成功");
                            break Some(config);
                        }
                        Err(e) => {
                            log::error!("加载旧配置文件失败: {e}");
                            // 弹出系统对话框让用户选择
                            if !show_migration_error_dialog(app.handle(), &e.to_string()) {
                                // 用户选择退出（此时数据库还没创建，下次启动可以重试）
                                log::info!("用户选择退出程序");
                                std::process::exit(1);
                            }
                            // 用户选择重试，继续循环
                            log::info!("用户选择重试加载配置文件");
                        }
                    }
                }
            } else {
                None
            };

            // 现在创建数据库（包含 Schema 迁移）
            //
            // 说明：从 v3.8.* 升级的用户通常会走到这里的 SQLite schema 迁移，
            // 若迁移失败（数据库损坏/权限不足/user_version 过新等），需要给用户明确提示，
            // 否则表现可能只是“应用打不开/闪退”。
            //
            // 预检：数据库版本过新时，必须先于任何 schema 写操作（create_tables 内含
            // DROP/ALTER 等 DDL）进入恢复界面，避免旧应用对读不懂的更新版 DB 落写。
            match crate::database::Database::stored_user_version_exceeds_supported(&db_path) {
                Ok(Some(version)) => {
                    log::warn!("数据库版本过新（v{version}），引导用户在应用内升级应用");
                    crate::init_status::set_init_error(crate::init_status::InitErrorPayload {
                        path: db_path.display().to_string(),
                        error: format!(
                            "数据库版本过新（{version}），当前应用仅支持 {}，请升级应用后再尝试。",
                            crate::database::SCHEMA_VERSION
                        ),
                        kind: Some("db_version_too_new".to_string()),
                        db_version: Some(version),
                        supported_version: Some(crate::database::SCHEMA_VERSION),
                    });
                    // 主窗口默认 visible:false，恢复界面必须强制显示
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                    return Ok(());
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!("预检数据库版本失败，继续正常初始化流程: {e}");
                }
            }

            let db = loop {
                match crate::database::Database::init() {
                    Ok(db) => break Arc::new(db),
                    Err(e) => {
                        log::error!("Failed to init database: {e}");

                        if !show_database_init_error_dialog(app.handle(), &db_path, &e.to_string())
                        {
                            log::info!("用户选择退出程序");
                            std::process::exit(1);
                        }

                        log::info!("用户选择重试初始化数据库");
                    }
                }
            };

            // 如果有预加载的配置，执行迁移
            if let Some(config) = migration_config {
                log::info!("开始执行数据迁移...");

                match db.migrate_from_json(&config) {
                    Ok(_) => {
                        log::info!("✓ 配置迁移成功");
                        // 标记迁移成功，供前端显示 Toast
                        crate::init_status::set_migration_success();
                        // 归档旧配置文件（重命名而非删除，便于用户恢复）
                        let archive_path = json_path.with_extension("json.migrated");
                        if let Err(e) = std::fs::rename(&json_path, &archive_path) {
                            log::warn!("归档旧配置文件失败: {e}");
                        } else {
                            log::info!("✓ 旧配置已归档为 config.json.migrated");
                        }
                    }
                    Err(e) => {
                        // 配置加载成功但迁移失败的情况极少（磁盘满等），仅记录日志
                        log::error!("配置迁移失败: {e}，将从现有配置导入");
                    }
                }
            }

            let app_state = AppState::new(db);

            // 设置 AppHandle 用于代理故障转移时的 UI 更新
            app_state.proxy_service.set_app_handle(app.handle().clone());

            // ============================================================
            // 按表独立判断的导入逻辑（各类数据独立检查，互不影响）
            // ============================================================

            // 1. 初始化默认 Skills 仓库（已有内置检查：表非空则跳过）
            match app_state.db.init_default_skill_repos() {
                Ok(count) if count > 0 => {
                    log::info!("✓ Initialized {count} default skill repositories");
                }
                Ok(_) => {} // 表非空，静默跳过
                Err(e) => log::warn!("✗ Failed to initialize default skill repos: {e}"),
            }

            // 1.1. 自动导入 Claude live 配置并 seed Claude 官方供应商。
            //
            // 先 import 后 seed 是有意为之：先把用户手动配置的 settings.json / auth.json / .env
            // 落成 "default" provider 设为 current，再追加官方预设（is_current=false）。
            // 这样用户切到官方预设时，回填机制会保护原 live 配置不丢失。
            //
            // 捕获首次运行快照。
            // 读失败时默认不弹，宁可漏弹也不要因为故障打扰用户。
            let first_run_already_confirmed = crate::settings::get_settings()
                .first_run_notice_confirmed
                .unwrap_or(false);
            let fresh_install_at_startup =
                app_state.db.is_providers_empty().unwrap_or(false);

            for app_type in crate::product_scope::supported_app_types() {
                if !crate::services::provider::should_import_default_config_on_startup(
                    &app_state,
                    &app_type,
                )
                .unwrap_or(false)
                {
                    log::debug!(
                        "○ {} already has providers; live import skipped",
                        app_type.as_str()
                    );
                    continue;
                }

                match crate::services::provider::import_default_config(
                    &app_state,
                    app_type.clone(),
                ) {
                    Ok(true) => log::info!(
                        "✓ Imported live config for {} as default provider",
                        app_type.as_str()
                    ),
                    Ok(false) => log::debug!(
                        "○ {} already has providers; live import skipped",
                        app_type.as_str()
                    ),
                    Err(e) => log::debug!(
                        "○ No live config to import for {}: {e}",
                        app_type.as_str()
                    ),
                }
            }

            for (seed_id, app_type) in [
                ("claude-official", crate::app_config::AppType::Claude),
                (
                    crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                    crate::app_config::AppType::ClaudeDesktop,
                ),
            ] {
                if let Err(error) = app_state.db.ensure_official_seed_by_id(seed_id, app_type) {
                    log::warn!("✗ Failed to seed Claude official provider: {error}");
                }
            }

            // 老用户 / 已确认的路径由 `fresh_install_at_startup` 自行拦截，这里不做写入。
            // 字段只由前端在用户点击"我知道了"时 save_settings 回写，语义是"用户显式确认过"。
            if !first_run_already_confirmed && fresh_install_at_startup {
                log::info!("✓ First-run welcome notice pending");
            }

            // 2. 导入 Claude MCP 服务器配置（表空时触发）
            if app_state.db.is_mcp_table_empty().unwrap_or(false) {
                log::info!("MCP table empty, importing from live configurations...");

                match crate::services::mcp::McpService::import_from_claude(&app_state) {
                    Ok(count) if count > 0 => {
                        log::info!("✓ Imported {count} MCP server(s) from Claude");
                    }
                    Ok(_) => log::debug!("○ No Claude MCP servers found to import"),
                    Err(e) => log::warn!("✗ Failed to import Claude MCP: {e}"),
                }

            }

            // 3. 导入 Claude 提示词文件（表空时触发）
            if app_state.db.is_prompts_table_empty().unwrap_or(false) {
                log::info!("Prompts table empty, importing from live configurations...");

                for app in [crate::app_config::AppType::Claude] {
                    match crate::services::prompt::PromptService::import_from_file_on_first_launch(
                        &app_state,
                        app.clone(),
                    ) {
                        Ok(count) if count > 0 => {
                            log::info!("✓ Imported {count} prompt(s) for {}", app.as_str());
                        }
                        Ok(_) => log::debug!("○ No prompt file found for {}", app.as_str()),
                        Err(e) => log::warn!("✗ Failed to import prompt for {}: {e}", app.as_str()),
                    }
                }
            }

            // 迁移旧的 app_config_dir 配置到 Store
            if let Err(e) = app_store::migrate_app_config_dir_from_settings(app.handle()) {
                log::warn!("迁移 app_config_dir 失败: {e}");
            }

            // 启动阶段不再无条件保存,避免意外覆盖用户配置。

            // 注册 deep-link URL 处理器（使用正确的 DeepLinkExt API）
            log::info!("=== Registering deep-link URL handler ===");

            // Linux 和 Windows 调试模式需要显式注册
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            {
                #[cfg(target_os = "linux")]
                {
                    // Use Tauri's path API to get correct path (includes app identifier)
                    // tauri-plugin-deep-link writes a desktop handler on Linux.
                    // Only register if .desktop file doesn't exist to avoid overwriting user customizations
                    let should_register = app
                        .path()
                        .data_dir()
                        .map(|d| !d.join("applications/cc-gateway-handler.desktop").exists())
                        .unwrap_or(true);

                    if should_register {
                        if let Err(e) = app.deep_link().register_all() {
                            log::error!("✗ Failed to register deep link schemes: {}", e);
                        } else {
                            log::info!("✓ Deep link schemes registered (Linux)");
                        }
                    } else {
                        log::info!("⊘ Deep link handler already exists, skipping registration");
                    }
                }

                #[cfg(all(debug_assertions, windows))]
                {
                    if let Err(e) = app.deep_link().register_all() {
                        log::error!("✗ Failed to register deep link schemes: {}", e);
                    } else {
                        log::info!("✓ Deep link schemes registered (Windows debug)");
                    }
                }
            }

            // 注册 URL 处理回调（所有平台通用）
            app.deep_link().on_open_url({
                let app_handle = app.handle().clone();
                move |event| {
                    log::info!("=== Deep Link Event Received (on_open_url) ===");
                    let urls = event.urls();
                    log::info!("Received {} URL(s)", urls.len());

                    if crate::lightweight::is_lightweight_mode() {
                        if let Err(e) = crate::lightweight::exit_lightweight_mode(&app_handle) {
                            log::error!("退出轻量模式重建窗口失败: {e}");
                        }
                    }

                    for (i, url) in urls.iter().enumerate() {
                        let url_str = url.as_str();
                        log::debug!("  URL[{i}]: {}", redact_url_for_log(url_str));

                        if handle_deeplink_url(&app_handle, url_str, true, "on_open_url") {
                            break; // Process only first ccgateway:// URL
                        }
                    }
                }
            });
            log::info!("✓ Deep-link URL handler registered");

            // 创建动态托盘菜单
            let menu = tray::create_tray_menu(app.handle(), &app_state)?;

            // 构建托盘
            let mut tray_builder = TrayIconBuilder::with_id(tray::TRAY_ID)
                .tooltip("CC Gateway") // 鼠标悬停提示
                .on_tray_icon_event(|tray, event| match event {
                    // 鼠标悬停/点击到托盘图标时，后台异步刷新用量缓存，
                    // 让用户下一次（或快速打开菜单的那一刻）看到较新的数字。
                    // refresh_all_usage_in_tray 内部有 10 秒防抖。
                    TrayIconEvent::Enter { .. } | TrayIconEvent::Click { .. } => {
                        let app = tray.app_handle().clone();
                        tauri::async_runtime::spawn(async move {
                            crate::tray::refresh_all_usage_in_tray(&app).await;
                        });
                    }
                    _ => log::debug!("unhandled event {event:?}"),
                })
                .menu(&menu)
                .on_menu_event(|app, event| {
                    tray::handle_tray_menu_event(app, &event.id.0);
                })
                .show_menu_on_left_click(true);

            // 使用平台对应的托盘图标（macOS 使用模板图标适配深浅色）
            #[cfg(target_os = "macos")]
            {
                if let Some(icon) = macos_tray_icon() {
                    tray_builder = tray_builder.icon(icon).icon_as_template(true);
                } else if let Some(icon) = app.default_window_icon() {
                    log::warn!("Falling back to default window icon for tray");
                    tray_builder = tray_builder.icon(icon.clone());
                } else {
                    log::warn!("Failed to load macOS tray icon for tray");
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                if let Some(icon) = app.default_window_icon() {
                    tray_builder = tray_builder.icon(icon.clone());
                } else {
                    log::warn!("Failed to get default window icon for tray");
                }
            }

            let _tray = tray_builder.build(app)?;
            crate::services::webdav_auto_sync::start_worker(
                app_state.db.clone(),
                app.handle().clone(),
            );
            crate::services::s3_auto_sync::start_worker(
                app_state.db.clone(),
                app.handle().clone(),
            );
            // 将同一个实例注入到全局状态，避免重复创建导致的不一致
            app.manage(app_state);

            // 从数据库加载日志配置并应用
            {
                let db = &app.state::<AppState>().db;
                if let Ok(log_config) = db.get_log_config() {
                    log::set_max_level(log_config.to_level_filter());
                    log::info!(
                        "已加载日志配置: enabled={}, level={}",
                        log_config.enabled,
                        log_config.level
                    );
                }
            }

            // 初始化 SkillService
            let skill_service = SkillService::new();
            app.manage(commands::skill::SkillServiceState(Arc::new(skill_service)));

            // Usage scripts share the provider usage command with the legacy
            // Copilot resolver. Keep the in-memory manager available without
            // exposing any Copilot OAuth commands in the product interface.
            {
                use crate::proxy::providers::copilot_auth::CopilotAuthManager;
                use commands::CopilotAuthState;
                use tokio::sync::RwLock;

                let app_config_dir = crate::config::get_app_config_dir();
                let manager = CopilotAuthManager::new(app_config_dir);
                app.manage(CopilotAuthState(Arc::new(RwLock::new(manager))));
            }

            // 初始化全局出站代理 HTTP 客户端
            {
                let db = &app.state::<AppState>().db;
                let proxy_url = db.get_global_proxy_url().ok().flatten();

                if let Err(e) = crate::proxy::http_client::init(proxy_url.as_deref()) {
                    log::error!(
                        "[GlobalProxy] [GP-005] Failed to initialize with saved config: {e}"
                    );

                    // 清除无效的代理配置
                    if proxy_url.is_some() {
                        log::warn!(
                            "[GlobalProxy] [GP-006] Clearing invalid proxy config from database"
                        );
                        if let Err(clear_err) = db.set_global_proxy_url(None) {
                            log::error!(
                                "[GlobalProxy] [GP-007] Failed to clear invalid config: {clear_err}"
                            );
                        }
                    }

                    // 使用直连模式重新初始化
                    if let Err(fallback_err) = crate::proxy::http_client::init(None) {
                        log::error!(
                            "[GlobalProxy] [GP-008] Failed to initialize direct connection: {fallback_err}"
                        );
                    }
                }
            }

            // 异常退出恢复 + 代理状态自动恢复
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>();

                // 检查是否有 Live 备份（表示上次异常退出时可能处于接管状态）
                let has_backups = match state.db.get_live_backup("claude").await {
                    Ok(backup) => backup.is_some(),
                    Err(e) => {
                        log::error!("检查 Live 备份失败: {e}");
                        false
                    }
                };
                // 检查 Live 配置是否仍处于被接管状态（包含占位符）
                let live_taken_over = state
                    .proxy_service
                    .detect_takeover_in_live_config_for_app(&crate::app_config::AppType::Claude);

                if has_backups || live_taken_over {
                    log::warn!("检测到上次异常退出（存在接管残留），正在恢复 Live 配置...");
                    if let Err(e) = state
                        .proxy_service
                        .restore_live_config_for_app_with_fallback_inner(
                            &crate::app_config::AppType::Claude,
                        )
                        .await
                    {
                        log::error!("恢复 Live 配置失败: {e}");
                    } else {
                        let _ = state.db.delete_live_backup("claude").await;
                        log::info!("Live 配置已恢复");
                    }
                }

                initialize_common_config_snippets(&state);

                // 检查 settings 表中的代理状态，自动恢复代理服务
                restore_proxy_state_on_startup(&state).await;

                // Periodic backup check (on startup)
                if let Err(e) = state.db.periodic_backup_if_needed() {
                    log::warn!("Periodic backup failed on startup: {e}");
                }

                // Periodic maintenance timer: run once per day while the app is running
                let db_for_timer = state.db.clone();
                tauri::async_runtime::spawn(async move {
                    const PERIODIC_MAINTENANCE_INTERVAL_SECS: u64 = 24 * 60 * 60;
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                        PERIODIC_MAINTENANCE_INTERVAL_SECS,
                    ));
                    interval.tick().await; // skip immediate first tick (already checked above)
                    loop {
                        interval.tick().await;
                        if let Err(e) = db_for_timer.periodic_backup_if_needed() {
                            log::warn!("Periodic maintenance timer failed: {e}");
                        }
                    }
                });

                // Session log usage sync: 启动时同步一次，之后每 60 秒检查
                let db_for_session_sync = state.db.clone();
                tauri::async_runtime::spawn(async move {
                    const SESSION_SYNC_INTERVAL_SECS: u64 = 60;

                    fn run_step<T>(name: &str, result: Result<T, crate::error::AppError>) {
                        if let Err(e) = result {
                            log::warn!("{name} failed: {e}");
                        }
                    }

                    let db = &db_for_session_sync;

                    // 首次同步
                    run_step(
                        "Usage cost startup backfill",
                        db.backfill_missing_usage_costs(),
                    );
                    run_step(
                        "Session usage initial sync",
                        crate::services::session_usage::sync_claude_session_logs(db),
                    );

                    // 定期同步
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                        SESSION_SYNC_INTERVAL_SECS,
                    ));
                    interval.tick().await; // skip immediate first tick
                    loop {
                        interval.tick().await;
                        run_step(
                            "Session usage periodic sync",
                            crate::services::session_usage::sync_claude_session_logs(db),
                        );
                    }
                });
            });

            // Linux: 禁用 WebKitGTK 硬件加速，防止 EGL 初始化失败导致白屏
            #[cfg(target_os = "linux")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.with_webview(|webview| {
                        use webkit2gtk::{WebViewExt, SettingsExt, HardwareAccelerationPolicy};
                        let wk_webview = webview.inner();
                        if let Some(settings) = WebViewExt::settings(&wk_webview) {
                            SettingsExt::set_hardware_acceleration_policy(&settings, HardwareAccelerationPolicy::Never);
                            log::info!("已禁用 WebKitGTK 硬件加速");
                        }
                    });
                }
            }

            // 静默启动：根据设置决定是否显示主窗口
            let settings = crate::settings::get_settings();
            if let Some(window) = app.get_webview_window("main") {
                // 在窗口首次显示前同步装饰状态，避免前端加载后再切换导致标题栏闪烁
                // 仅 Linux 生效：解决 Wayland 下系统窗口按钮不可用的问题
                #[cfg(target_os = "linux")]
                let _ = window.set_decorations(!settings.use_app_window_controls);
                if settings.silent_startup {
                    // 静默启动模式：保持窗口隐藏
                    let _ = window.hide();
                    #[cfg(target_os = "windows")]
                    let _ = window.set_skip_taskbar(true);
                    #[cfg(target_os = "macos")]
                    tray::apply_tray_policy(app.handle(), false);
                    log::info!("静默启动模式：主窗口已隐藏");
                } else {
                    // 正常启动模式：显示窗口
                    let _ = window.show();
                    log::info!("正常启动模式：主窗口已显示");

                    // Linux: 解决首次启动 UI 无响应问题（Tauri #10746 + wry #637）。
                    // 启动时 webview 未获取焦点 + surface 尺寸协商失败，导致点击无效。
                    // 这里做 set_focus + 伪 resize，等价于无视觉版本的"最大化-还原"。
                    #[cfg(target_os = "linux")]
                    {
                        linux_fix::nudge_main_window(window.clone());
                    }
                }
            }


            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_external,
            commands::get_init_error,
            commands::copy_text_to_clipboard,
            commands::open_file_dialog,
            commands::get_agent_gateway_state,
            commands::update_agent_gateway_config,
            commands::regenerate_agent_gateway_token,
            commands::test_agent_gateway_protocol,
            commands::get_claude_connection_profile,
            commands::update_claude_connection_profile,
            commands::test_claude_connection_model,
            commands::test_all_claude_connection_models,
            commands::get_claude_adapter_state,
            commands::set_claude_code_adapter_enabled,
            commands::set_claude_desktop_adapter_enabled,
            commands::get_gateway_diagnostics,
            commands::preview_legacy_cc_switch_import,
            commands::import_legacy_cc_switch_database,
            commands::set_window_theme,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|app_handle, event| {
        // 处理退出请求（所有平台）
        if let RunEvent::ExitRequested { api, code, .. } = &event {
            match classify_exit_request(*code) {
                // code 为 None 表示运行时自动触发（如隐藏窗口的 WebView 被回收导致无存活窗口），
                // 此时应仅阻止退出、保持托盘后台运行。
                ExitRequestAction::StayInTray => {
                    log::info!("运行时触发退出请求（无存活窗口），阻止退出以保持托盘后台运行");
                    api.prevent_exit();
                    return;
                }
                // code 为 RESTART_EXIT_CODE：app.restart() / 自更新 relaunch 发起的重启。
                // 这条路径上 prevent_exit() 会被 Tauri 忽略，事件循环必定退出，随后由
                // Tauri 在 RunEvent::Exit 后用新二进制 re-exec（macOS 会按更新后的
                // Info.plist 解析可执行名）。
                //
                // 绝不能复用下面的异步清理任务：该任务在 tokio 线程调 save_window_state，
                // 持有 window-state 插件锁的同时向主线程查询窗口几何；而主线程此刻正在
                // 退出事件循环，并在插件自带的 RunEvent::Exit 钩子里等待同一把锁——双方
                // 互等造成进程永久卡死（更新已安装但应用冻结、不再重启，见 #3998）。
                //
                // 重启路径交还 Tauri 默认流程即可：
                //   - 窗口状态：插件 Exit 钩子在主线程保存（同线程读取窗口几何，无死锁）
                //   - 托盘图标：Tauri 内部 cleanup_before_exit 清理，正常走 Drop
                //   - 代理/Live 配置：无需恢复，重启后新实例立即接管并恢复代理状态
                //   - 100ms 落盘等待：重启前的 DB 写入均为命令驱动、此刻已完成，
                //     与所有 Tauri 应用默认重启路径的行为一致，无需额外等待
                ExitRequestAction::DeferToTauriRestart => {
                    log::info!("收到重启请求 (code={code:?})，交由 Tauri 默认重启流程 re-exec");
                    return;
                }
                // 其它 Some(_)：用户主动调用 app.exit() 退出（如托盘菜单"退出"），
                // 此时执行清理后退出。
                ExitRequestAction::CleanupAndExit => {}
            }

            log::info!("收到用户主动退出请求 (code={code:?})，开始清理...");
            api.prevent_exit();

            let app_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                save_window_state_before_exit(&app_handle);
                cleanup_before_exit(&app_handle).await;
                // 先于 std::process::exit 显式移除托盘图标。
                // 进程直接退出时 Tauri 运行时不走正常 Drop 流程，
                // 不会向 Windows Shell 发送 NIM_DELETE，导致已退出的进程
                // 注册的图标仍残留在系统托盘（鼠标悬停 Shell 才会重绘发现进程已死）。
                remove_tray_icon_before_exit(&app_handle);
                log::info!("清理完成，退出应用");

                // 短暂等待确保所有 I/O 操作（如数据库写入）刷新到磁盘
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                // 使用 std::process::exit 避免再次触发 ExitRequested
                std::process::exit(0);
            });
            return;
        }

        #[cfg(target_os = "macos")]
        {
            match event {
                // macOS 在 Dock 图标被点击并重新激活应用时会触发 Reopen 事件，这里手动恢复主窗口
                RunEvent::Reopen { .. } => {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        #[cfg(target_os = "windows")]
                        {
                            let _ = window.set_skip_taskbar(false);
                        }
                        let _ = window.unminimize();
                        let _ = window.show();
                        let _ = window.set_focus();
                        tray::apply_tray_policy(app_handle, true);
                    } else if crate::lightweight::is_lightweight_mode() {
                        if let Err(e) = crate::lightweight::exit_lightweight_mode(app_handle) {
                            log::error!("退出轻量模式重建窗口失败: {e}");
                        }
                    }
                }
                // 处理通过自定义 URL 协议触发的打开事件（例如 ccgateway://...）
                RunEvent::Opened { urls } => {
                    if let Some(url) = urls.first() {
                        let url_str = url.to_string();
                        log::info!("RunEvent::Opened with URL: {url_str}");

                        if url_str.starts_with("ccgateway://") {
                            if crate::lightweight::is_lightweight_mode() {
                                if let Err(e) = crate::lightweight::exit_lightweight_mode(app_handle)
                                {
                                    log::error!("退出轻量模式重建窗口失败: {e}");
                                }
                            }

                            // 解析并广播深链接事件，复用与 single_instance 相同的逻辑
                            match crate::deeplink::parse_deeplink_url(&url_str) {
                                Ok(request) => {
                                    log::info!(
                                        "Successfully parsed deep link from RunEvent::Opened: resource={}, app={:?}",
                                        request.resource,
                                        request.app
                                    );

                                    if let Err(e) =
                                        app_handle.emit("deeplink-import", &request)
                                    {
                                        log::error!(
                                            "Failed to emit deep link event from RunEvent::Opened: {e}"
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::error!(
                                        "Failed to parse deep link URL from RunEvent::Opened: {e}"
                                    );

                                    if let Err(emit_err) = app_handle.emit(
                                        "deeplink-error",
                                        serde_json::json!({
                                            "url": url_str,
                                            "error": e.to_string()
                                        }),
                                    ) {
                                        log::error!(
                                            "Failed to emit deep link error event from RunEvent::Opened: {emit_err}"
                                        );
                                    }
                                }
                            }

                            // 确保主窗口可见
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.unminimize();
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app_handle, event);
        }
    });
}

// ============================================================
// 应用退出清理
// ============================================================

/// 应用退出前的清理工作
///
/// 在应用退出前检查代理服务器状态，如果正在运行则停止代理并恢复 Live 配置。
/// 确保 Claude Code/Codex/Gemini 的配置不会处于损坏状态。
/// 使用 stop_with_restore_keep_state 保留 settings 表中的代理状态，下次启动时自动恢复。
pub async fn cleanup_before_exit(app_handle: &tauri::AppHandle) {
    if let Some(state) = app_handle.try_state::<store::AppState>() {
        let proxy_service = &state.proxy_service;

        // 退出时也需要兜底：代理可能已崩溃/未运行，但 Live 接管残留仍在（占位符/备份）。
        let has_backups = match state.db.get_live_backup("claude").await {
            Ok(backup) => backup.is_some(),
            Err(e) => {
                log::error!("退出时检查 Live 备份失败: {e}");
                false
            }
        };
        let live_taken_over = proxy_service
            .detect_takeover_in_live_config_for_app(&crate::app_config::AppType::Claude);
        let needs_restore = has_backups || live_taken_over;

        if needs_restore {
            log::info!("检测到接管残留，开始恢复 Live 配置（保留代理状态）...");
            if let Err(e) = proxy_service
                .restore_live_config_for_app_with_fallback_inner(
                    &crate::app_config::AppType::Claude,
                )
                .await
            {
                log::error!("退出时恢复 Live 配置失败: {e}");
            } else {
                let _ = state.db.delete_live_backup("claude").await;
                log::info!("已恢复 Live 配置（代理状态已保留，下次启动将自动恢复）");
            }
        }

        // 停止共享 listener；Claude 的 enabled 状态仍保留供下次启动恢复。
        if proxy_service.is_running().await {
            log::info!("检测到代理服务器正在运行，开始停止...");
            if let Err(e) = proxy_service.stop().await {
                log::error!("退出时停止代理失败: {e}");
            }
            log::info!("代理服务器清理完成");
        }
    }
}

/// 主动从系统托盘移除托盘图标。
///
/// `std::process::exit` 会绕过 Tauri 运行时，触发不了 `TrayIcon::drop()`，
/// 也就不会向 Windows Shell 发 `NIM_DELETE`。结果是进程退出后托盘里
/// 仍保留一个死图标的缓存占位（Shell 不会主动重绘，需要鼠标悬停才刷新）。
///
/// 通过 `set_visible(false)` 走 `WM_USER_HIDE_TRAYICON` 消息路径，
/// 触发 tray-icon 内部的 `remove_tray_icon` → `Shell_NotifyIconW(NIM_DELETE)`，
/// 在进程结束前干净地把图标摘掉。其它平台 `set_visible(false)` 也是
/// 正常的隐藏/移除语义，作为跨平台兜底也安全。
pub(crate) fn remove_tray_icon_before_exit(app_handle: &tauri::AppHandle) {
    if let Some(tray) = app_handle.tray_by_id(tray::TRAY_ID) {
        if let Err(e) = tray.set_visible(false) {
            log::warn!("退出时移除托盘图标失败: {e}");
        } else {
            log::info!("已显式从系统托盘移除图标");
        }
    }
}

// ============================================================
// 启动时恢复代理状态
// ============================================================

/// Exact persisted consumers that CC Gateway is allowed to restore.
///
/// Keeping this as a closed plan prevents legacy rows (Codex, Gemini, etc.)
/// from becoming active merely because an old database still contains them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupRestorePlan {
    consumers: Vec<crate::claude_runtime::RuntimeConsumer>,
}

impl StartupRestorePlan {
    fn contains(&self, consumer: crate::claude_runtime::RuntimeConsumer) -> bool {
        self.consumers.contains(&consumer)
    }

    fn is_empty(&self) -> bool {
        self.consumers.is_empty()
    }
}

async fn startup_restore_plan(db: &database::Database) -> StartupRestorePlan {
    use crate::claude_runtime::RuntimeConsumer;

    let mut consumers = Vec::with_capacity(3);
    if db
        .get_proxy_config_for_app("claude")
        .await
        .is_ok_and(|config| config.enabled)
    {
        consumers.push(RuntimeConsumer::ClaudeCode);
    }
    if db
        .get_proxy_config_for_app("claude-desktop")
        .await
        .is_ok_and(|config| config.enabled)
    {
        consumers.push(RuntimeConsumer::ClaudeDesktop);
    }
    if crate::agent_gateway::load_agent_gateway_config(db).is_ok_and(|config| config.enabled) {
        consumers.push(RuntimeConsumer::Backend);
    }

    StartupRestorePlan { consumers }
}

/// Clear only the persisted intent for a consumer whose startup restoration
/// failed. Live-file repair and listener release are attempted separately;
/// this database fallback guarantees the UI never reports a route as enabled
/// when it is not usable.
async fn disable_failed_startup_consumer(
    db: &database::Database,
    consumer: crate::claude_runtime::RuntimeConsumer,
) -> Result<(), String> {
    use crate::claude_runtime::RuntimeConsumer;

    match consumer {
        RuntimeConsumer::ClaudeCode | RuntimeConsumer::ClaudeDesktop => {
            let app_id = match consumer {
                RuntimeConsumer::ClaudeCode => "claude",
                RuntimeConsumer::ClaudeDesktop => "claude-desktop",
                RuntimeConsumer::Backend => unreachable!(),
            };
            let mut config = db
                .get_proxy_config_for_app(app_id)
                .await
                .map_err(|error| error.to_string())?;
            config.enabled = false;
            db.update_proxy_config_for_app(config)
                .await
                .map_err(|error| error.to_string())
        }
        RuntimeConsumer::Backend => {
            let previous = crate::agent_gateway::load_agent_gateway_config(db)
                .map_err(|error| error.to_string())?;
            crate::agent_gateway::update_config(
                db,
                crate::agent_gateway::AgentGatewayConfigInput {
                    enabled: false,
                    emulate_claude_code: previous.emulate_claude_code,
                },
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        }
    }
}

async fn restore_proxy_state_on_startup(state: &store::AppState) {
    use crate::claude_runtime::RuntimeConsumer;

    let plan = startup_restore_plan(&state.db).await;

    if plan.is_empty() {
        log::debug!("启动时无需恢复代理状态");
        return;
    }

    log::info!("检测到上次 Claude 路由状态需要恢复: {:?}", plan.consumers);

    if plan.contains(RuntimeConsumer::Backend) {
        match state
            .proxy_service
            .acquire_claude_consumer(RuntimeConsumer::Backend)
            .await
        {
            Ok(server) => log::info!("✓ 已恢复 Backend Gateway，端口: {}", server.port),
            Err(error) => {
                log::error!("✗ 恢复 Backend Gateway 失败: {error}");
                if let Err(clear_error) =
                    disable_failed_startup_consumer(&state.db, RuntimeConsumer::Backend).await
                {
                    log::error!("清除 Backend Gateway 假启用状态失败: {clear_error}");
                }
            }
        }
    }

    if plan.contains(RuntimeConsumer::ClaudeDesktop) {
        match state
            .proxy_service
            .set_claude_desktop_consumer_enabled(true)
            .await
        {
            Ok(()) => log::info!("✓ 已恢复 Claude Desktop 本地路由"),
            Err(error) => {
                log::error!("✗ 恢复 Claude Desktop 本地路由失败: {error}");
                if let Err(rollback_error) = state
                    .proxy_service
                    .set_claude_desktop_consumer_enabled(false)
                    .await
                {
                    log::error!("回滚 Claude Desktop Live 配置失败: {rollback_error}");
                }
                let _ = state
                    .proxy_service
                    .release_claude_consumer(RuntimeConsumer::ClaudeDesktop)
                    .await;
                if let Err(clear_error) =
                    disable_failed_startup_consumer(&state.db, RuntimeConsumer::ClaudeDesktop).await
                {
                    log::error!("清除 Claude Desktop 假启用状态失败: {clear_error}");
                }
            }
        }
    }

    if plan.contains(RuntimeConsumer::ClaudeCode) {
        let app_type = "claude";
        match state
            .proxy_service
            .set_takeover_for_app(app_type, true)
            .await
        {
            Ok(()) => {
                log::info!("✓ 已恢复 {app_type} 的代理接管状态");
            }
            Err(e) => {
                log::error!("✗ 恢复 {app_type} 的代理接管状态失败: {e}");
                if let Err(rollback_error) = state
                    .proxy_service
                    .set_takeover_for_app(app_type, false)
                    .await
                {
                    log::error!("回滚 {app_type} Live 配置失败: {rollback_error}");
                }
                let _ = state
                    .proxy_service
                    .release_claude_consumer(RuntimeConsumer::ClaudeCode)
                    .await;
                if let Err(clear_error) =
                    disable_failed_startup_consumer(&state.db, RuntimeConsumer::ClaudeCode).await
                {
                    log::error!("清除 {app_type} 假启用状态失败: {clear_error}");
                }
            }
        }
    }
}

fn initialize_common_config_snippets(state: &store::AppState) {
    // Auto-extract common config snippets from clean live files when snippet is missing.
    // This must run before proxy takeover is restored on startup, otherwise we'd read
    // proxy-placeholder configs instead of the user's actual live settings.
    for app_type in crate::product_scope::supported_app_types() {
        if !state
            .db
            .should_auto_extract_config_snippet(app_type.as_str())
            .unwrap_or(false)
        {
            continue;
        }

        let settings = match crate::services::provider::ProviderService::read_live_settings(
            app_type.clone(),
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };

        match crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
            app_type.clone(),
            &settings,
        ) {
            Ok(snippet) if !snippet.is_empty() && snippet != "{}" => {
                match state.db.set_config_snippet(app_type.as_str(), Some(snippet)) {
                    Ok(()) => {
                        let _ = state.db.set_config_snippet_cleared(app_type.as_str(), false);
                        log::info!(
                            "✓ Auto-extracted common config snippet for {}",
                            app_type.as_str()
                        );
                    }
                    Err(e) => log::warn!(
                        "✗ Failed to save config snippet for {}: {e}",
                        app_type.as_str()
                    ),
                }
            }
            Ok(_) => log::debug!(
                "○ Live config for {} has no extractable common fields",
                app_type.as_str()
            ),
            Err(e) => log::warn!(
                "✗ Failed to extract config snippet for {}: {e}",
                app_type.as_str()
            ),
        }
    }

    let should_run_legacy_migration = state
        .db
        .is_legacy_common_config_migrated()
        .map(|done| !done)
        .unwrap_or(true);

    if should_run_legacy_migration {
        for app_type in [crate::app_config::AppType::Claude] {
            if let Err(e) = crate::services::provider::ProviderService::migrate_legacy_common_config_usage_if_needed(
                state,
                app_type.clone(),
            ) {
                log::warn!(
                    "✗ Failed to migrate legacy common-config usage for {}: {e}",
                    app_type.as_str()
                );
            }
        }

        if let Err(e) = state.db.set_legacy_common_config_migrated(true) {
            log::warn!("✗ Failed to persist legacy common-config migration flag: {e}");
        }
    }
}

// ============================================================
// 迁移错误对话框辅助函数
// ============================================================

/// 检测是否为中文环境
fn is_chinese_locale() -> bool {
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .map(|lang| lang.starts_with("zh"))
        .unwrap_or(false)
}

/// 显示迁移错误对话框
/// 返回 true 表示用户选择重试，false 表示用户选择退出
fn show_migration_error_dialog(app: &tauri::AppHandle, error: &str) -> bool {
    let title = if is_chinese_locale() {
        "配置迁移失败"
    } else {
        "Migration Failed"
    };

    let message = if is_chinese_locale() {
        format!(
            "从旧版本迁移配置时发生错误：\n\n{error}\n\n\
            您的数据尚未丢失，旧配置文件仍然保留。\n\
            建议保留旧配置，并使用最新版本 CC Gateway 的选择性导入功能。\n\n\
            点击「重试」重新尝试迁移\n\
            点击「退出」关闭程序（可回退版本后重新打开）"
        )
    } else {
        format!(
            "An error occurred while migrating configuration:\n\n{error}\n\n\
            Your data is NOT lost - the old config file is still preserved.\n\
            Keep the legacy configuration and use the selective import in the latest CC Gateway.\n\n\
            Click 'Retry' to attempt migration again\n\
            Click 'Exit' to close the program"
        )
    };

    let retry_text = if is_chinese_locale() {
        "重试"
    } else {
        "Retry"
    };
    let exit_text = if is_chinese_locale() {
        "退出"
    } else {
        "Exit"
    };

    // 使用 blocking_show 同步等待用户响应
    // OkCancelCustom: 第一个按钮（重试）返回 true，第二个按钮（退出）返回 false
    app.dialog()
        .message(&message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            retry_text.to_string(),
            exit_text.to_string(),
        ))
        .blocking_show()
}

/// 显示数据库初始化/Schema 迁移失败对话框
/// 返回 true 表示用户选择重试，false 表示用户选择退出
fn show_database_init_error_dialog(
    app: &tauri::AppHandle,
    db_path: &std::path::Path,
    error: &str,
) -> bool {
    let title = if is_chinese_locale() {
        "数据库初始化失败"
    } else {
        "Database Initialization Failed"
    };

    let message = if is_chinese_locale() {
        format!(
            "初始化数据库或迁移数据库结构时发生错误：\n\n{error}\n\n\
            数据库文件路径：\n{db}\n\n\
            您的数据尚未丢失，应用不会自动删除数据库文件。\n\
            常见原因包括：数据库版本过新、文件损坏、权限不足、磁盘空间不足等。\n\n\
            建议：\n\
            1) 先备份整个配置目录（包含 cc-gateway.db）\n\
            2) 如果提示“数据库版本过新”，请升级到更新版本\n\
            3) 如果刚升级出现异常，可回退旧版本导出/备份后再升级\n\n\
            点击「重试」重新尝试初始化\n\
            点击「退出」关闭程序",
            db = db_path.display()
        )
    } else {
        format!(
            "An error occurred while initializing or migrating the database:\n\n{error}\n\n\
            Database file path:\n{db}\n\n\
            Your data is NOT lost - the app will not delete the database automatically.\n\
            Common causes include: newer database version, corrupted file, permission issues, or low disk space.\n\n\
            Suggestions:\n\
            1) Back up the entire config directory (including cc-gateway.db)\n\
            2) If you see “database version is newer”, please upgrade CC Gateway\n\
            3) If this happened right after upgrading, consider rolling back to export/backup then upgrade again\n\n\
            Click 'Retry' to attempt initialization again\n\
            Click 'Exit' to close the program",
            db = db_path.display()
        )
    };

    let retry_text = if is_chinese_locale() {
        "重试"
    } else {
        "Retry"
    };
    let exit_text = if is_chinese_locale() {
        "退出"
    } else {
        "Exit"
    };

    app.dialog()
        .message(&message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            retry_text.to_string(),
            exit_text.to_string(),
        ))
        .blocking_show()
}

// ============================================================
// 退出请求分类
// ============================================================

/// `RunEvent::ExitRequested` 的三类来源，处理方式必须区分。
///
/// 关键约束：重启请求（`code == RESTART_EXIT_CODE`）上 `prevent_exit()` 会被
/// Tauri 静默忽略（见 `ExitRequestApi::prevent_exit` 文档），事件循环必定继续
/// 退出并触发各插件的 `RunEvent::Exit` 钩子；任何与之并发的自定义清理任务都
/// 可能与插件退出钩子争用同一状态而死锁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitRequestAction {
    /// `code` 为 `None`：运行时自动触发（如隐藏窗口的 WebView 被回收导致无存活
    /// 窗口），阻止退出、保持托盘后台运行。
    StayInTray,
    /// `code` 为 `RESTART_EXIT_CODE`：`app.restart()` / 自更新 relaunch 发起的
    /// 重启，不拦截、不做自定义清理，交还 Tauri 默认 re-exec 流程。
    DeferToTauriRestart,
    /// 其它 `Some(_)`：用户主动退出（托盘「退出」等），执行完整异步清理后结束进程。
    CleanupAndExit,
}

fn classify_exit_request(code: Option<i32>) -> ExitRequestAction {
    match code {
        None => ExitRequestAction::StayInTray,
        Some(tauri::RESTART_EXIT_CODE) => ExitRequestAction::DeferToTauriRestart,
        Some(_) => ExitRequestAction::CleanupAndExit,
    }
}

// ============================================================
// 在应用主动退出前显式持久化窗口状态
// ============================================================

fn window_state_flags() -> StateFlags {
    StateFlags::POSITION | StateFlags::SIZE | StateFlags::MAXIMIZED
}

/// 当前应用的退出路径会拦截 `ExitRequested` 并最终直接 `std::process::exit(0)`，
/// 这里需要在真正结束进程前手动落盘，避免 window-state 插件的默认退出钩子被绕过。
pub fn save_window_state_before_exit(app_handle: &tauri::AppHandle) {
    if let Err(err) = app_handle.save_window_state(window_state_flags()) {
        log::error!("退出前保存窗口状态失败: {err}");
    } else {
        log::info!("已在退出前保存窗口状态");
    }
}

/// 主动释放 single-instance 锁。
///
/// macOS single-instance 使用 `/tmp/{identifier}.sock`。我们有若干路径会直接
/// `std::process::exit(0)`，不会触发插件挂在 `RunEvent::Exit` 上的清理钩子。
/// 重启前主动 destroy 可以避免新进程误连旧 listener 后自行退出。
pub fn destroy_single_instance_lock(app_handle: &tauri::AppHandle) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    tauri_plugin_single_instance::destroy(app_handle);
}

/// 清理托盘图标、释放 single-instance 锁后重启当前应用。
///
/// 直接走 `tauri::process::restart`（spawn 新进程 + `exit(0)`），不经过事件
/// 循环退出，因此 Tauri 内部的 `cleanup_before_exit` 和各插件的
/// `RunEvent::Exit` 钩子都不会执行。需要的清理由调用方与本函数显式补偿：
/// 窗口状态、代理/Live 恢复（调用方）；托盘图标、single-instance 锁（本函数）。
///
/// 有意不调 `AppHandle::cleanup_before_exit()`：它会在调用线程上 Drop 托盘
/// 图标，而 macOS 的 NSStatusItem 操作要求主线程；`set_visible(false)` 走
/// `run_item_main_thread` 代理，跨线程安全（见 `remove_tray_icon_before_exit`）。
pub fn restart_process(app_handle: &tauri::AppHandle) -> ! {
    remove_tray_icon_before_exit(app_handle);
    destroy_single_instance_lock(app_handle);
    tauri::process::restart(&app_handle.env());
}

#[cfg(test)]
mod tests {
    use super::{
        classify_exit_request, disable_failed_startup_consumer, startup_restore_plan,
        ExitRequestAction,
    };
    use crate::claude_runtime::RuntimeConsumer;
    use crate::database::Database;

    #[test]
    fn no_code_keeps_app_alive_in_tray() {
        assert_eq!(classify_exit_request(None), ExitRequestAction::StayInTray);
    }

    #[test]
    fn restart_exit_code_defers_to_tauri_default_restart() {
        assert_eq!(
            classify_exit_request(Some(tauri::RESTART_EXIT_CODE)),
            ExitRequestAction::DeferToTauriRestart
        );
    }

    #[test]
    fn user_exit_codes_run_cleanup_then_exit() {
        assert_eq!(
            classify_exit_request(Some(0)),
            ExitRequestAction::CleanupAndExit
        );
        assert_eq!(
            classify_exit_request(Some(1)),
            ExitRequestAction::CleanupAndExit
        );
    }

    #[test]
    fn public_ipc_surface_is_the_claude_only_allowlist() {
        let source = include_str!("lib.rs");
        let (_, handler_tail) = source
            .split_once(".invoke_handler(tauri::generate_handler![")
            .expect("invoke handler declaration");
        let (handler_body, _) = handler_tail
            .split_once("]);")
            .expect("invoke handler terminator");
        let commands = handler_body
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("commands::"))
            .map(|line| line.trim_end_matches(','))
            .collect::<Vec<_>>();

        assert_eq!(
            commands,
            vec![
                "open_external",
                "get_init_error",
                "copy_text_to_clipboard",
                "open_file_dialog",
                "get_agent_gateway_state",
                "update_agent_gateway_config",
                "regenerate_agent_gateway_token",
                "test_agent_gateway_protocol",
                "get_claude_connection_profile",
                "update_claude_connection_profile",
                "test_claude_connection_model",
                "test_all_claude_connection_models",
                "get_claude_adapter_state",
                "set_claude_code_adapter_enabled",
                "set_claude_desktop_adapter_enabled",
                "get_gateway_diagnostics",
                "preview_legacy_cc_switch_import",
                "import_legacy_cc_switch_database",
                "set_window_theme",
            ]
        );
    }

    #[tokio::test]
    async fn startup_restore_plan_includes_only_the_three_claude_consumers() {
        let db = Database::memory().expect("initialize database");
        for app_id in ["claude", "claude-desktop", "codex", "gemini", "grokbuild"] {
            let mut config = db
                .get_proxy_config_for_app(app_id)
                .await
                .expect("read proxy config");
            config.enabled = true;
            db.update_proxy_config_for_app(config)
                .await
                .expect("enable proxy config");
        }
        db.set_setting(
            crate::agent_gateway::AGENT_GATEWAY_CONFIG_KEY,
            r#"{"enabled":true,"token":"ccg-agent-test","emulateClaudeCode":true}"#,
        )
        .expect("persist Backend Gateway state");

        let plan = startup_restore_plan(&db).await;

        assert_eq!(
            plan.consumers,
            vec![
                RuntimeConsumer::ClaudeCode,
                RuntimeConsumer::ClaudeDesktop,
                RuntimeConsumer::Backend,
            ]
        );
    }

    #[tokio::test]
    async fn failed_startup_restore_clears_only_the_failed_claude_consumers() {
        let db = Database::memory().expect("initialize database");
        for app_id in ["claude", "claude-desktop", "codex", "gemini"] {
            let mut config = db
                .get_proxy_config_for_app(app_id)
                .await
                .expect("read proxy config");
            config.enabled = true;
            db.update_proxy_config_for_app(config)
                .await
                .expect("enable proxy config");
        }
        db.set_setting(
            crate::agent_gateway::AGENT_GATEWAY_CONFIG_KEY,
            r#"{"enabled":true,"token":"ccg-agent-test","emulateClaudeCode":true}"#,
        )
        .expect("persist Backend Gateway state");

        for consumer in [
            RuntimeConsumer::ClaudeCode,
            RuntimeConsumer::ClaudeDesktop,
            RuntimeConsumer::Backend,
        ] {
            disable_failed_startup_consumer(&db, consumer)
                .await
                .expect("clear failed startup consumer");
        }

        assert!(
            !db.get_proxy_config_for_app("claude")
                .await
                .expect("read Claude Code state")
                .enabled
        );
        assert!(
            !db.get_proxy_config_for_app("claude-desktop")
                .await
                .expect("read Claude Desktop state")
                .enabled
        );
        let backend =
            crate::agent_gateway::load_agent_gateway_config(&db).expect("read Backend state");
        assert!(!backend.enabled);
        assert!(backend.emulate_claude_code);
        for legacy_app in ["codex", "gemini"] {
            assert!(
                db.get_proxy_config_for_app(legacy_app)
                    .await
                    .expect("read legacy state")
                    .enabled,
                "startup rollback must not mutate {legacy_app}"
            );
        }
    }
}
