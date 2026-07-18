<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="CC Gateway 图标">
</p>

# CC Gateway

CC Gateway 是面向 Claude Code、Claude Desktop 和本地 Agent Backend 的桌面管理器与协议网关。你只需配置一份 Anthropic 兼容上游（默认 AnyRouter），三类入口就会共用同一份连接资料和模型目录，同时不会把上游 Key 暴露给本地客户端。

> **v0.1.0 是未签名的预发布版。** 首版提供 macOS Universal 与 Windows x64 安装包，系统可能要求手动确认后才能运行。

[English](README.md) · [日本語](README_JA.md) · [Deutsch](README_DE.md)

## 核心能力

- 一份 canonical 连接配置统一保存 Base URL、Bearer/API Key 认证、上游凭据和模型目录。
- Claude Code 与 Claude Desktop 共用 `127.0.0.1:15722` 上的 Rust 本地 listener。
- 为用户自行管理的 Agent 提供 Anthropic Messages、OpenAI Responses 和 Chat Completions 三种协议。
- Claude Code、Claude Desktop、Backend 分别使用独立本地 Gateway Key；客户端不会拿到 AnyRouter Key。
- “测试 AnyRouter”会发送真实的最小 Messages 请求；“测试全部模型”可逐一确认受限 Key 能访问哪些模型。
- 只保留最近 50 条脱敏内存诊断，不记录 prompt、正文、认证头或密钥。

CC Gateway 只管理 Claude Code 与 Claude Desktop，不会写入 Codex、Gemini 或其他 Agent 的配置。

## 本地入口

| 用途 | 地址 |
| --- | --- |
| Claude Code | `http://127.0.0.1:15722/v1/messages` |
| Claude Desktop | `http://127.0.0.1:15722/claude-desktop` |
| Agent · Anthropic Messages | `http://127.0.0.1:15722/agent/v1/messages` |
| Agent · OpenAI Responses | `http://127.0.0.1:15722/agent/v1/responses` |
| Agent · Chat Completions | `http://127.0.0.1:15722/agent/v1/chat/completions` |
| Agent · 模型目录 | `http://127.0.0.1:15722/agent/v1/models` |

v0.1.0 明确不支持 `/agent/v1/responses/compact`。

## 默认 AnyRouter 模型目录

| 角色 | 客户端模型 | 实际上游模型 |
| --- | --- | --- |
| Opus | `claude-opus-4-8` | `claude-opus-4-8` |
| Fable / Sonnet | `claude-fable-5` | `claude-fable-5` |
| Haiku / Subagent / Fallback | `claude-opus-4-6` | `claude-opus-4-6` |

`[1M]` 只是客户端上下文声明，发送给上游前一定会被移除，不会变成真实模型 ID。

## 使用方式

1. 在“连接”页填写 AnyRouter Base URL 与上游 API Key，只需保存一次。
2. 先执行“测试 AnyRouter”；如果 Key 限制了模型，再执行“测试全部模型”。
3. 按需启用 Claude Code、Claude Desktop 或 Backend Gateway。
4. 其他 Agent 只需使用它自己的本地 Gateway Key，并连接上表中的 `/agent/v1/*` 入口。

应用会先启动并探测 listener，再写入客户端 Live 配置。停用适配器时，只恢复 CC Gateway 实际拥有的字段，保留用户后来添加的其他设置。

## 数据与迁移

- 数据目录：`~/.cc-gateway`
- 数据库：`~/.cc-gateway/cc-gateway.db`
- 日志：`~/.cc-gateway/logs/cc-gateway.log`
- 深链接：`ccgateway://`

选择性旧数据导入以只读方式打开旧数据库，只导入 Claude Code、Claude Desktop、Claude MCP/Skills/Prompts、凭据和模型映射；不会导入其他 Agent、接管状态、Gateway Key、同步设置或历史记录，也不会自动写入 Live 配置。

## 从源码构建

需要 Node.js 20、pnpm 10.12.3、Rust 1.85 或更高版本，以及 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
corepack enable
corepack prepare pnpm@10.12.3 --activate
pnpm install --frozen-lockfile
pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm tauri build
```

发布流水线会在干净的 GitHub Runner 构建未签名的 macOS Universal 与 Windows x64 产物。自动更新已关闭，安装前请核对 Release 中的 SHA-256 校验文件。

## 安全说明

- listener 必须保持在 loopback，不要把 `15722` 端口暴露到局域网或互联网。
- 上游 API Key 属于敏感凭据；如果它曾出现在公开聊天、Issue、日志或提交中，请立即轮换。
- 诊断中不会保存 prompt、响应正文、认证头或密钥。
- AnyRouter 是第三方服务。CC Gateway 无法证明中转服务是否等同于模型官方 API；发送敏感代码前请自行审查其条款和数据处理方式。

## 许可证与归属

CC Gateway 使用 [MIT License](LICENSE) 发布。项目是独立社区 Fork，上游归属与第三方说明见 [NOTICE](NOTICE)。本项目与 Anthropic、AnyRouter 均无隶属或官方合作关系。
