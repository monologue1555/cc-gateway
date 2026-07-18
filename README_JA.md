<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="CC Gateway icon">
</p>

# CC Gateway

CC Gateway は Claude Code、Claude Desktop、およびローカル Agent Backend 向けのデスクトップ管理ツール兼プロトコルゲートウェイです。Anthropic 互換の上流接続（既定は AnyRouter）を一度設定すると、3 つの入口が同じ接続情報とモデルカタログを共有します。

> **v0.1.0 は未署名のプレリリースです。** macOS Universal と Windows x64 を対象とします。

[English](README.md) · [简体中文](README_ZH.md) · [Deutsch](README_DE.md)

## 主な機能

- Base URL、認証方式、上流 API キー、モデルカタログを 1 つの接続プロファイルで管理します。
- Claude Code と Claude Desktop は `127.0.0.1:15722` の同じ Rust listener を利用します。
- ユーザー管理の Agent 向けに Anthropic Messages、OpenAI Responses、Chat Completions を公開します。
- 各ローカルクライアントには別々の Gateway Key を発行し、上流 API キーを公開しません。
- 実際の最小 Messages リクエストで接続と各モデルを検証します。
- 診断は最大 50 件のメタデータのみをメモリに保持し、prompt、本文、認証ヘッダー、キーは保存しません。

CC Gateway が設定を書き込む対象は Claude Code と Claude Desktop だけです。Codex、Gemini、その他 Agent の設定は管理しません。

## Agent Backend エンドポイント

- Anthropic: `http://127.0.0.1:15722/agent/v1/messages`
- Responses: `http://127.0.0.1:15722/agent/v1/responses`
- Chat: `http://127.0.0.1:15722/agent/v1/chat/completions`
- Models: `http://127.0.0.1:15722/agent/v1/models`

v0.1.0 では `/agent/v1/responses/compact` をサポートしません。

## 既定モデル

| 役割 | 上流モデル |
| --- | --- |
| Opus | `claude-opus-4-8` |
| Fable / Sonnet | `claude-fable-5` |
| Haiku / Subagent / Fallback | `claude-opus-4-6` |

`[1M]` はクライアント側のコンテキスト宣言であり、上流へ送る前に必ず除去されます。

## データとビルド

- データ: `~/.cc-gateway`
- DB: `~/.cc-gateway/cc-gateway.db`
- ログ: `~/.cc-gateway/logs/cc-gateway.log`
- Deep link: `ccgateway://`

```bash
corepack enable
corepack prepare pnpm@10.12.3 --activate
pnpm install --frozen-lockfile
pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm tauri build
```

必要条件は Node.js 20、pnpm 10.12.3、Rust 1.85 以上、および Tauri 2 のシステム依存関係です。自動更新は無効です。

## セキュリティとライセンス

ポート `15722` は loopback のみにしてください。公開された上流キーは直ちにローテーションしてください。AnyRouter は第三者サービスであり、機密コードを送信する前に利用規約とデータ処理を確認してください。

CC Gateway は [MIT License](LICENSE) で配布されます。上流の帰属は [NOTICE](NOTICE) を参照してください。本プロジェクトは Anthropic または AnyRouter の公式製品ではありません。
