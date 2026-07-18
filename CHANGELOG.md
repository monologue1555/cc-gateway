# Changelog

All notable changes to CC Gateway are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-07-18

First unsigned pre-release of the independent CC Gateway product.

### Added

- Canonical Anthropic-compatible connection profile with an AnyRouter-first default.
- Shared Rust listener for Claude Code, Claude Desktop, and local agent backends.
- Anthropic Messages, OpenAI Responses, Chat Completions, and model-catalog backend endpoints.
- Dedicated local gateway keys that isolate clients from the upstream credential.
- Real minimal upstream and per-model connectivity tests with categorized failures.
- Runtime readiness, consumer lifecycle, rollback, loopback bypass, and restart recovery.
- Redacted in-memory diagnostics with stream-completion tracking.
- Selective, read-only import of legacy Claude data.
- Independent product identity, data directory, deep-link scheme, and original icon set.

### Changed

- Product scope is limited to Claude Code and Claude Desktop; other agent configuration is not managed.
- Default listener port is `15722`.
- Build toolchain is fixed to Node.js 20 and pnpm 10.12.3.

### Removed

- Automatic updating, upstream promotional material, affiliate links, and prior brand assets.
- Non-Claude startup scanning, live configuration writes, OAuth handling, and session synchronization.

### Release notes

- macOS Universal and Windows x64 artifacts are unsigned and published as a pre-release.
- `/agent/v1/responses/compact` is not supported in v0.1.0.
