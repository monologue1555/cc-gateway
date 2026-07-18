<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="CC Gateway icon">
</p>

# CC Gateway

CC Gateway ist ein fokussierter Desktop-Manager und lokales Protokoll-Gateway für Claude Code, Claude Desktop und lokale Agent-Backends. Eine Anthropic-kompatible Upstream-Verbindung – standardmäßig AnyRouter – wird einmal konfiguriert und von allen drei Zugängen gemeinsam genutzt.

> **v0.1.0 ist eine unsignierte Vorabversion** für macOS Universal und Windows x64.

[English](README.md) · [简体中文](README_ZH.md) · [日本語](README_JA.md)

## Funktionen

- Ein Verbindungsprofil für Base URL, Authentifizierung, Upstream-API-Key und Modellkatalog.
- Ein gemeinsamer Rust-Listener unter `127.0.0.1:15722` für Claude Code und Claude Desktop.
- Lokale Anthropic-Messages-, OpenAI-Responses- und Chat-Completions-Endpunkte für selbst verwaltete Agents.
- Getrennte Gateway Keys pro lokalem Client; der Upstream-Key wird nicht an Clients ausgegeben.
- Echte minimale Messages-Anfragen zum Testen der Verbindung und jedes freigegebenen Modells.
- Höchstens 50 redigierte Diagnoseeinträge im Arbeitsspeicher, ohne Prompts, Inhalte, Auth-Header oder Schlüssel.

CC Gateway verwaltet ausschließlich Claude Code und Claude Desktop. Konfigurationen von Codex, Gemini oder anderen Agents werden nicht verändert.

## Agent-Backend-Endpunkte

- Anthropic: `http://127.0.0.1:15722/agent/v1/messages`
- Responses: `http://127.0.0.1:15722/agent/v1/responses`
- Chat: `http://127.0.0.1:15722/agent/v1/chat/completions`
- Models: `http://127.0.0.1:15722/agent/v1/models`

`/agent/v1/responses/compact` wird in v0.1.0 nicht unterstützt.

## Standardmodelle

| Rolle | Upstream-Modell |
| --- | --- |
| Opus | `claude-opus-4-8` |
| Fable / Sonnet | `claude-fable-5` |
| Haiku / Subagent / Fallback | `claude-opus-4-6` |

Das optionale Suffix `[1M]` ist nur eine Kontextdeklaration des Clients und wird vor der Upstream-Anfrage entfernt.

## Daten und Build

- Daten: `~/.cc-gateway`
- Datenbank: `~/.cc-gateway/cc-gateway.db`
- Log: `~/.cc-gateway/logs/cc-gateway.log`
- Deep Link: `ccgateway://`

```bash
corepack enable
corepack prepare pnpm@10.12.3 --activate
pnpm install --frozen-lockfile
pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm tauri build
```

Benötigt werden Node.js 20, pnpm 10.12.3, Rust 1.85 oder neuer und die Tauri-2-Systemabhängigkeiten. Automatische Updates sind deaktiviert.

## Sicherheit und Lizenz

Port `15722` darf nur über Loopback erreichbar sein. Ein öffentlich gewordener Upstream-Key muss sofort rotiert werden. AnyRouter ist ein Drittanbieter; prüfen Sie dessen Bedingungen und Datenverarbeitung, bevor Sie vertraulichen Code senden.

CC Gateway steht unter der [MIT License](LICENSE). Hinweise zur Herkunft enthält [NOTICE](NOTICE). Dieses Projekt ist kein offizielles Produkt von Anthropic oder AnyRouter.
