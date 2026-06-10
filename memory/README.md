# Cairn

**Your context, your agents, your control.**

A personal **Memory / Context OS** for the Agent era. You write notes; AI structures them; any MCP-capable agent (Claude, ChatGPT, Cursor, Gemini, …) can read what you authorize, signed and audited.

Built around a real technical thesis — not just "yet another LLM wrapper":

| Layer | What's in it |
|---|---|
| **Schema-constrained LLM extraction** | Claude tool-use API + JSON-Schema double validation. Reliably typed entities. |
| **Hierarchical memory (Phase 1: flat → Phase 2: Episodic / Semantic / Procedural)** | Ebbinghaus-inspired importance: `R = exp(-Δt / S)`, strength reinforced on access. |
| **Hybrid retrieval** | SQLite FTS5 BM25 + entity-type filter + importance + recency, weighted re-rank. |
| **Cryptographic audit log** | Merkle-chained `(prev_hash → hash)` + Ed25519 signature per entry. Anyone with your public key can verify history wasn't rewritten. |
| **MCP server** | Full JSON-RPC 2.0 over stdio. Permission matrix per-agent × entity-type. Audited on every call. |

The product looks like a note app. Underneath it is the API surface every one of your future agents will plug into.

## Layout

```
memory/
├── src-tauri/                 Rust core
│   ├── src/
│   │   ├── lib.rs             AppState + Tauri builder
│   │   ├── commands.rs        IPC commands → Next.js UI
│   │   ├── schema.rs          Domain types (mirrors src/lib/types.ts)
│   │   ├── db/                SQLite + queries
│   │   ├── extract/           LLM extraction (Anthropic now, on-device later)
│   │   │   ├── prompt.rs      Few-shot + JSON Schema
│   │   │   ├── anthropic.rs   tool_use schema-constrained decoder
│   │   │   └── validate.rs    Defence-in-depth JSON-Schema check
│   │   ├── importance.rs      Ebbinghaus scoring
│   │   ├── retrieval.rs       Hybrid BM25 + importance + recency
│   │   ├── audit.rs           Merkle-chain + Ed25519 audit
│   │   ├── mcp/               JSON-RPC 2.0 MCP server
│   │   └── bin/mcp_stdio.rs   Standalone MCP binary for Claude Desktop
│   └── migrations/            sqlx migrations
├── src/                       Next.js 15 UI (static export)
│   ├── app/                   pages: /, /entities, /entity, /agents, /audit
│   ├── components/            Capture / Timeline / EntityChip
│   └── lib/                   tauri.ts (typed invoke), types.ts (mirrors Rust)
└── docs/                      ARCHITECTURE / SETUP / MCP_INTEGRATION
```

## Quick start

```bash
cd memory
pnpm install
cp .env.example .env          # add your ANTHROPIC_API_KEY
pnpm tauri:dev                # launches Tauri shell with Next.js hot-reload
```

Once running:

1. Type something into the textarea (e.g. *"I prefer my coffee with low sugar, not too hot."*) → ⌘+Enter.
2. Watch it appear in the timeline as `extracting…`, then turn `extracted`.
3. Open `/entities` → see your extracted `Preference`.
4. Open `/agents` → grant `claude-desktop` read on `Preference` (it's seeded by default).
5. Add the MCP server to Claude Desktop config (see [docs/MCP_INTEGRATION.md](./docs/MCP_INTEGRATION.md)).
6. Ask Claude: *"What does the user prefer for coffee?"* — it'll call `search_memory` and answer.
7. Open `/audit` → see the access logged, hashed, and signed. Hit "verify chain".

## Why this is more than a note app

| Conventional memory product | Cairn |
|---|---|
| Locked to one model (Mem.ai → OpenAI, Anthropic Memory → Claude) | **Protocol-neutral**: any MCP-capable agent |
| Flat key-value store | **Typed, structured** (8 entity types) + relations + Ebbinghaus decay |
| "trust us" on privacy | **Cryptographic audit log** — verify-don't-trust |
| Schema-free text | **Schema-constrained extraction** via Claude tool_use + JSON-Schema validation |
| API call per note | Phase 2: **on-device 1.5B model** for free, instant, offline extraction |

## Roadmap

- **Phase 1 (current)**: Foundation. App + MCP + audit + extraction via Claude API.
- **Phase 2**: On-device extraction model (Qwen 2.5-1.5B distilled, MLX). Hierarchical memory (Episodic → Semantic via sleep consolidation).
- **Phase 3**: Personal embedding fine-tuning (contrastive on user feedback). CRDT multi-device sync.
- **Phase 4**: BBS+ selective-disclosure proofs. Family Context (shared schemas, separate data).

See [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) for the deep dive.

## License

TBD. Source-available for now.
