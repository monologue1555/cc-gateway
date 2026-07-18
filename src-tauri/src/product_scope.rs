use crate::app_config::AppType;

/// Applications intentionally exposed by CC Gateway.
///
/// The legacy implementations for other clients remain available to internal
/// migrations, but they are outside the product's public/runtime scope.
pub(crate) fn supported_app_types() -> [AppType; 2] {
    [AppType::Claude, AppType::ClaudeDesktop]
}

pub(crate) fn is_supported_app_type(app_type: &AppType) -> bool {
    matches!(app_type, AppType::Claude | AppType::ClaudeDesktop)
}

/// Parse an application identifier at a public CC Gateway boundary.
///
/// `AppType` intentionally retains legacy variants so old data can still be
/// read and migrated. Public commands must use this parser instead of
/// `AppType::from_str` so those dormant implementations cannot be activated.
pub(crate) fn parse_public_app_type(app_id: &str) -> Result<AppType, String> {
    match app_id {
        "claude" => Ok(AppType::Claude),
        "claude-desktop" => Ok(AppType::ClaudeDesktop),
        _ => Err(format!(
            "Application '{app_id}' is unsupported by CC Gateway; allowed app ids: claude, claude-desktop"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_public_app_type, supported_app_types};
    use crate::app_config::AppType;

    #[test]
    fn product_scope_lists_only_claude_clients() {
        assert_eq!(
            supported_app_types(),
            [AppType::Claude, AppType::ClaudeDesktop]
        );
    }

    #[test]
    fn public_app_parser_rejects_legacy_non_claude_clients() {
        for app_id in [
            "codex",
            "gemini",
            "grokbuild",
            "opencode",
            "openclaw",
            "hermes",
        ] {
            let error = parse_public_app_type(app_id).expect_err("app must be outside scope");
            assert!(error.contains("unsupported by CC Gateway"), "{error}");
        }
    }

    #[test]
    fn public_app_parser_accepts_only_canonical_claude_ids() {
        assert_eq!(parse_public_app_type("claude"), Ok(AppType::Claude));
        assert_eq!(
            parse_public_app_type("claude-desktop"),
            Ok(AppType::ClaudeDesktop)
        );

        for alias in ["Claude", " claude", "claude_desktop", "claudeDesktop"] {
            let error = parse_public_app_type(alias).expect_err("aliases are not public app ids");
            assert!(error.contains("allowed app ids"), "{error}");
        }
    }
}
