# Security Policy / 安全策略

## Supported versions / 支持版本

Only the latest CC Gateway `0.1.x` pre-release receives security fixes.

仅最新的 CC Gateway `0.1.x` 预发布版接收安全修复。

## Report a vulnerability / 报告漏洞

Do not disclose vulnerabilities, API keys, local gateway keys, prompts, or private logs in a public issue. Use [GitHub Security Advisories](https://github.com/monologue1555/cc-gateway/security/advisories/new) for a private report.

请勿在公开 Issue 中披露漏洞、API Key、本地 Gateway Key、prompt 或私有日志。请通过 [GitHub Security Advisories](https://github.com/monologue1555/cc-gateway/security/advisories/new) 私下报告。

Include the affected version and platform, reproduction steps, impact, and a minimal redacted log when available. Never include a live credential.

报告时请包含受影响版本与平台、复现步骤、影响范围以及脱敏后的最小日志，切勿附带仍然有效的凭据。

## Local gateway boundary / 本地网关边界

CC Gateway is designed for loopback use. Port `15722` must not be exposed to a LAN or the public internet. Rotate any upstream key that has appeared in a public chat, issue, log, screenshot, or commit.

CC Gateway 仅按 loopback 场景设计，请勿将 `15722` 暴露到局域网或互联网。任何曾出现在公开聊天、Issue、日志、截图或提交中的上游 Key 都应立即轮换。
