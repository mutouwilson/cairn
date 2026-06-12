<div align="center">

# Cairn

**Your memory, structured by AI — readable by every agent, owned by you.**

*Local-first · cryptographically auditable · works with Claude, Cursor, ChatGPT & any MCP agent.*

[![CI](https://github.com/mutouwilson/cairn/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/mutouwilson/cairn/actions/workflows/ci.yml)
[![License: FSL-1.1-ALv2](https://img.shields.io/badge/license-FSL--1.1--ALv2-blue.svg)](./LICENSE)
[![Status: alpha](https://img.shields.io/badge/status-alpha-orange.svg)](#status)
[![Latest release](https://img.shields.io/github/v/release/mutouwilson/cairn?include_prereleases&sort=semver&label=download)](https://github.com/mutouwilson/cairn/releases)
![Made with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg?logo=rust)
![Tauri 2](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=black)
![Next.js 15](https://img.shields.io/badge/Next.js-15-black?logo=next.js)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://www.conventionalcommits.org)

</div>

![Cairn in action — passive recall in ChatGPT, structured entities, signed audit chain](./docs/demo.gif)

Write a note. Cairn's AI structures it into typed **entities, relations, and themes**. Any MCP-capable agent — Claude Desktop, Cursor, Codex, Cline — then reads exactly what you authorize. Every read and write is **signed and hashed into a tamper-evident chain**, so you can prove your memory wasn't touched behind your back.

- 🔒 **Local-first & private** — a single SQLite file on *your* machine. No cloud, no account.
- 🔗 **One memory, every agent** — shared over MCP, plus a browser extension that works across 10+ AI chat sites (ChatGPT, Claude, Gemini, DeepSeek, Kimi, 豆包…).
- 🧾 **Verify, don't trust** — SHA-256 hash chain + Ed25519 signatures; every access is provable, offline.

**[⬇ Download for macOS / Windows / Linux »](https://github.com/mutouwilson/cairn/releases)**  ·  or [build from source](#quick-start)

> [!IMPORTANT]
> **Status:** alpha, source-available under [FSL-1.1](./LICENSE) (each release converts to Apache-2.0 after two years). Prebuilt desktop apps + the Chrome extension are in [Releases](https://github.com/mutouwilson/cairn/releases). On-device extraction (V5) and dev-tool memory sync (V6) are in progress.

## Why Cairn

| Other memory products | Cairn |
|---|---|
| Locked to one model (Mem.ai → OpenAI, Anthropic Memory → Claude) | **Protocol-neutral** via MCP — any MCP-capable agent |
| Flat key-value store | **Typed, structured** (8 entity types) + relations + Ebbinghaus decay |
| "Trust us" on privacy | **Cryptographic audit log** — hash chain + Ed25519, verify-don't-trust |
| Schema-free text | **Schema-constrained extraction** via `tool_use` + JSON-Schema validation |
| API call per note | Phase 2: **on-device 1.5 B model** — free, instant, offline |

## Features

- **Local-first** — single SQLite file under `~/Library/Application Support/Cairn/`. Nothing leaves the device unless you grant access.
- **MCP-native** — `cairn-mcp` speaks JSON-RPC 2.0 over stdio, or serve MCP over SSE (`127.0.0.1:7717`) straight from the app; drop it into Claude Desktop / Cursor in 30 seconds.
- **Per-agent × per-entity-type permission matrix** — grant Claude read on `Preference` but not `Finance`, in one click.
- **Hash-chained audit log** — every read, write, extract and grant is hashed and Ed25519-signed; verify it offline from the Audit page.
- **Hybrid retrieval** — FTS5 BM25 + sqlite-vec kNN fused via RRF, re-ranked by importance + Ebbinghaus recency + manual-edit trust boost.
- **Multi-surface capture** — manual note, ⌘⇧M hotkey, selection popover, IMAP, iCal subscription.
- **Three binaries, one crate** — `cairn` (GUI), `cairn-mcp` (MCP server), `cairn-migrate` (encrypted DB migration).
- **Optional at-rest encryption** — `--features encrypted` builds a vendored SQLCipher; key in your OS keychain.

## Quick start

```bash
git clone https://github.com/mutouwilson/cairn.git
cd cairn/memory
pnpm install
pnpm tauri:dev                # Tauri shell with Next.js hot-reload
# AI provider: configure in-app via Settings → AI providers (10 families);
# `cp .env.example .env` + AI_GATEWAY_API_KEY works as a headless fallback.
```

The first `cargo build` pulls a lot — give it 5–10 minutes; subsequent builds are incremental.

See [DEVELOPMENT.md](./DEVELOPMENT.md) for full setup, MCP integration and common pitfalls.

## Browser extension

Brings your memory into the AI chat sites you already use: as you type, relevant memories surface inline and inject with one keypress; any selection on any page can be saved back to Cairn.

**Install** (not on the Chrome Web Store yet — load unpacked):

1. Make sure the **Cairn desktop app is running** — the extension talks to it at `http://127.0.0.1:7716`, all local.
2. Download `cairn-chrome-extension-*.zip` from [Releases](https://github.com/mutouwilson/cairn/releases) and unzip it to a folder you keep around (Chrome loads it from disk).
3. Open `chrome://extensions` → enable **Developer mode** → **Load unpacked** → select the unzipped folder.
4. Click the Cairn icon → it should show *connected*. If you start Cairn with `CAIRN_API_TOKEN=…`, paste the same token in the extension's Options page.

**Use:**

- **Passive recall** — type in a supported chat composer; when your words match stored memories, a pill appears. The picker opens on strong matches: `1/2/3` toggle rows, `Enter` injects the selected memories as a context block, `Esc` dismisses. (IME-safe — composition keys are never intercepted.)
- **Summon** — type `@cairn <query>` in the composer to search explicitly.
- **Save from anywhere** — select text on any page → right-click → *Save selection to Cairn* (or *Save this page to Cairn*, or *Search Cairn for "…"*).
- **Quick search** — `⌘⇧L` / `Ctrl+Shift+L` opens the search popup; or type `cairn <query>` in the address bar.

**Works on:** ChatGPT, Claude, Gemini, DeepSeek, Kimi, 豆包, 文心一言, 通义千问, Mira, Manus, Genspark — toggle per-site in Options.

## Architecture

```
       Next.js 15 UI  ←─Tauri IPC─→  Rust core (cairn_lib)
                                       │
                                       ├─ capture / extract / embed / import
                                       ├─ retrieval (BM25 + vector, RRF-fused)
                                       ├─ audit  (hash chain + Ed25519)
                                       ├─ mcp    (stdio · SSE :7717)
                                       └─ REST   (/api/* :7716)
                                            │
                                            ▼
                       Claude Desktop · Cursor · Codex · browser extension …
```

Full module map and design decisions in [ARCHITECTURE.md](./ARCHITECTURE.md).

## Repo layout

| Path | What's in it |
|---|---|
| [`memory/`](./memory) | Cairn desktop app — Tauri shell, Next.js UI, Rust core |
| [`memory/src-tauri/`](./memory/src-tauri) | Rust core: SQLite + FTS5, audit chain, MCP, capture surfaces |
| [`memory/tools/browser-ext/`](./memory/tools/browser-ext) | Browser extension (selection capture) |
| [`tools/distill/`](./tools/distill) | On-device extraction model (Qwen 2.5-1.5B distillation, MLX) — Phase 2 |
| [`tools/share-extension/`](./tools/share-extension) | macOS share-sheet integration |

## Roadmap

- **Phase 1 (now)** — App + MCP + audit + Anthropic/Gateway extraction ✅
- **Phase 2** — On-device extraction (Qwen 2.5-1.5B, MLX) · hierarchical memory · dev-tool sync (V6)
- **Phase 3** — Personal embedding fine-tuning · CRDT multi-device sync
- **Phase 4** — BBS+ selective-disclosure proofs · Family Context

## Status

| Surface | Status |
|---|---|
| Capture (manual, hotkey, selection, IMAP, iCal) | ✅ shipping |
| Schema-constrained extraction (Anthropic API + Gateway) | ✅ shipping |
| MCP stdio server + permission matrix | ✅ shipping |
| Audit chain (hash chain + Ed25519) | ✅ shipping |
| Hybrid retrieval (FTS5 + importance + recency) | ✅ shipping |
| Optional encrypted SQLite (SQLCipher) | 🧪 experimental |
| **On-device extraction (Qwen 2.5-1.5B)** | 🚧 in progress |
| **Dev-tool memory sync (V6)** | 🚧 in progress |
| CRDT multi-device sync | ⏳ planned |

## Contributing

Cairn is in fast-moving alpha — **issues and bug reports are very welcome**; for code, open an issue to discuss before sending a PR. See [CONTRIBUTING.md](./CONTRIBUTING.md) for the rules, and [SECURITY.md](./SECURITY.md) for vulnerability reports.

## License

[Functional Source License (FSL-1.1-ALv2)](./LICENSE) — **Fair Source**, not OSI "open source". The code is fully public and auditable, and you may use, modify, and self-host it for any **non-competing** purpose. You may **not** use it to build a commercial product or service that competes with Cairn. Two years after each release, that version automatically converts to **Apache-2.0**.

Want to use Cairn in a competing or proprietary product before then? A separate commercial license is available — open an issue to get in touch.
