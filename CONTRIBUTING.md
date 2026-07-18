# Contributing to CC Gateway / 参与贡献

Thank you for contributing. Keep changes focused on Claude Code, Claude Desktop, and the local backend gateway. Features that manage unrelated agent configuration are outside the v0.1 product scope.

感谢参与贡献。改动应聚焦 Claude Code、Claude Desktop 与本地 Backend Gateway；管理其他 Agent 配置的功能不属于 v0.1 产品范围。

## Development environment / 开发环境

- Node.js 20
- pnpm 10.12.3
- Rust 1.85 or newer
- Tauri 2 platform prerequisites

```bash
corepack enable
corepack prepare pnpm@10.12.3 --activate
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

## Pull requests / Pull Request 要求

- Open an issue before a large feature or architecture change.
- Keep credentials, prompts, response bodies, and private logs out of tests and commits.
- Preserve the loopback-only listener, local/upstream key separation, and rollback behavior.
- Update all affected locales for user-facing text.
- Add tests for protocol conversion, streaming completion, model mapping, or lifecycle changes.
- Keep the original MIT `LICENSE` and update `NOTICE` when attribution changes.

提交较大的功能或架构改动前请先开 Issue；测试与提交中不得出现凭据、prompt、响应正文或私有日志。涉及协议转换、流式完成、模型映射或生命周期的改动必须补充测试，并保留 MIT `LICENSE` 与必要归属说明。

See the [Code of Conduct](CODE_OF_CONDUCT.md) and [Security Policy](SECURITY.md) before participating.
