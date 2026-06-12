# Architecture

Cairn is a **Personal Memory / Context OS** that lives on your machine and speaks **MCP** to every agent you grant access to. This document describes the runtime architecture, key modules and the design decisions behind them.

For setup and contributor instructions see [DEVELOPMENT.md](./DEVELOPMENT.md).

## Design pillars

| Pillar | What it means concretely |
|---|---|
| **Local-first** | Cairn runs as a Tauri desktop app. Memory data lives in a single SQLite file under `~/Library/Application Support/Cairn/` (override with `CAIRN_DATA_DIR`); a few small config/handshake JSON files sit in the same directory. Nothing leaves the device unless you explicitly grant access. |
| **Protocol-neutral** | Every external agent reads memory through a standard MCP (Model Context Protocol) server. Claude Desktop, Cursor, Cline, Codex — any MCP host plugs in the same way. |
| **Verify, don't trust** | Every write and every read is recorded on an append-only **hash chain** with an **Ed25519** signature per entry. You hold the public key; anyone can verify history wasn't rewritten. |
| **Schema-constrained extraction** | LLM output is validated against JSON Schema twice — the model is asked to emit structured `tool_use` calls, and the resulting object is re-validated before it hits the DB. |
| **Importance-aware retrieval** | Search blends BM25 (FTS5) and vector kNN (sqlite-vec) fused via RRF, entity-type filtering, Ebbinghaus recency decay (`R = exp(-Δt / S)`), access-reinforced strength, and a manual-edit trust boost. |

## High-level diagram

```
            ┌──────────────────────────────────────────────────────────────┐
            │                 Cairn Desktop App (Tauri 2)                  │
            │  ┌──────────────────────────────────────┐                    │
            │  │  Next.js 15 UI (static export)       │                    │
            │  │  /  /entities  /agents  /audit  ...  │                    │
            │  └─────────────────┬────────────────────┘                    │
            │                    │ Tauri IPC                               │
            │  ┌─────────────────▼────────────────────────────────────┐    │
            │  │  Rust core (cairn_lib)                               │    │
            │  │   capture · extract · embed · importance ·           │    │
            │  │   retrieval · audit (hash chain + Ed25519)           │    │
            │  │   consolidation · import · bundle · keystore         │    │
            │  └──┬─────────────────────┬──────────────────┬──────────┘    │
            │     │ sqlx                │ axum             │ axum          │
            │     ▼                     ▼                  ▼               │
            │  ┌──────────────┐ ┌────────────────┐ ┌────────────────┐      │
            │  │  SQLite +    │ │  REST API      │ │  MCP SSE bridge│      │
            │  │  FTS5 +      │ │ 127.0.0.1:7716 │ │ 127.0.0.1:7717 │      │
            │  │  sqlite-vec  │ │  (always-on)   │ │ Settings toggle│      │
            │  └──────▲───────┘ └───────┬────────┘ └───────┬────────┘      │
            └─────────┼──────────────────┼─────────────────┼───────────────┘
                      │ shared DB        │ HTTP            │ SSE
            ┌─────────┴────────┐         ▼                 ▼
            │  cairn-mcp       │   Browser Extension   SSE-capable
            │  (JSON-RPC 2.0   │   (ChatGPT, Gemini,   MCP clients
            │   over stdio)    │    Doubao, …)
            └─────────┬────────┘
                      │ stdio
                      ▼
            Claude Desktop · Cursor · Codex / any MCP host
```

Three binaries ship from the same Rust crate (`memory/src-tauri/Cargo.toml`):

- **`cairn`** — the GUI app launched by Tauri.
- **`cairn-mcp`** — a standalone MCP server over stdio, spawned by MCP hosts. Shares the same database as the GUI.
- **`cairn-migrate`** — one-shot plaintext → encrypted SQLite migration (built with `--features encrypted`, which vendors SQLCipher).

Two localhost HTTP servers run inside the GUI process:

- **REST API** (`api_server.rs`) — axum on `127.0.0.1:7716`, always-on. Routes: `/api/status`, `/api/capture`, `/api/search`, `/api/recent`, `/api/themes`. This is the endpoint the browser extension talks to.
- **MCP SSE bridge** (`mcp_bridge.rs`) — the same MCP server over SSE on `127.0.0.1:7717`, toggled on/off in Settings.

## Rust core modules

Source: `memory/src-tauri/src/`.

| Module | Role |
|---|---|
| `lib.rs` | Tauri app builder + shared `AppState`. |
| `commands.rs` | IPC commands exposed to the Next.js UI via `invoke`. |
| `schema.rs` | Domain types mirroring `memory/src/lib/types.ts`. |
| `db/` | sqlx pool, migrations (with automatic pre-migration backups), query helpers — entities, notes, audit, embeddings, entity governance (dedup / aliases / manual edits / soft-archive). |
| `capture/` | Multi-surface ingest — manual notes, global hotkey, selection popover, email/IMAP, iCal calendar. |
| `import/` | Imports dev-tool memory files (Claude Code / Cursor / Codex) — scanner, watcher, parsers, reverse export. |
| `bundle/` | Signed memory-bundle export / import. |
| `extract/` | LLM-driven entity extraction across 10 provider families — Anthropic via its native Messages API, the rest through an OpenAI-compatible client. Strict JSON-Schema validation. |
| `importance.rs` | Ebbinghaus strength model. Importance reinforced on every access. |
| `retrieval.rs` | Hybrid retrieval — RRF-fused BM25 (FTS5) + vector kNN, entity-type filter, importance + recency weighting, manual-edit trust boost. |
| `embed/` · `embed_pipeline.rs` · `vec_init.rs` | Embeddings via `sqlite-vec`, async pipeline. |
| `consolidation/` | Sleep-time worker that clusters episodic entities into `semantic_memories` (themes); supersede chains keep history. |
| `audit.rs` | Append-only hash chain `(prev_hash → hash)` + Ed25519 signature per entry. `verify_recent(limit)` verifies the most recent entries offline (default 500; pass a larger limit for the full chain). |
| `mcp/` · `mcp_bridge.rs` · `bin/mcp_stdio.rs` | JSON-RPC 2.0 MCP server — stdio plus an SSE bridge on `127.0.0.1:7717`. Per-agent × per-entity-type permission matrix. Every call is logged + signed. |
| `api.rs` | REST route handlers (`/api/status`, `/api/capture`, `/api/search`, `/api/recent`, `/api/themes`). |
| `api_server.rs` | Boots the REST API on `127.0.0.1:7716` — optional bearer token, endpoint handshake written to `api_endpoint.json`. |
| `selection_popover/` · `global_shortcut.rs` | macOS Accessibility + CGEventTap selection popover and global hotkey. |
| `keystore/` | OS keychain integration (Apple Keychain / Linux secret-service / Windows Credential Manager) for signing keys. |
| `providers.rs` | Provider registry — `providers.json` + `LiveProviders` hot reload; settings changes take effect immediately, no restart. |
| `signals.rs` · `usage.rs` | Lightweight telemetry (all local). |

## End-to-end write path

```
capture (UI / hotkey / popover / IMAP / iCal / REST /api/capture / memory-file import)
        │
        ▼
 raw note → SQLite (status = "pending")  ──► audit:capture/api (REST entry point only)
        │
        ▼
 extract (LLM tool_use)
        │
        ├─ JSON-Schema validation (first pass — model side)
        │
        ▼
 candidate entities + relations
        │
        ├─ JSON-Schema re-validation (defence-in-depth)
        │
        ▼
 SQLite: entities + relations + provenance
        │
        ▼
 importance.rs: assign initial strength
        │
        ▼
 embed (optional, async) ──► sqlite-vec
        │
        ▼
 audit:extract/ok (payload includes the embedded count)
 — failed extractions keep the raw note and log audit:extract/raw_fallback instead
```

## End-to-end read path (MCP)

```
host (Claude Desktop / Cursor) ──► cairn-mcp (stdio, JSON-RPC 2.0)
        │
        ├─ agent identity from env CAIRN_AGENT_ID (default "claude-desktop") — no authenticate step
        │
        ├─ check permission matrix (agent_id × entity_type → allow/deny)
        │      │
        │      └─ deny ──► normal result with denied: true (not an error);
        │                  the audit action is still tools/call
        │
        ▼
 retrieval.rs:
   1. FTS5 BM25 over entities_fts(name, properties) + notes_fts — trigram tokenizer;
      queries with tokens < 3 chars (most CJK) fall back to LIKE substring search
   2. vector kNN over sqlite-vec (cosine distance ≤ 0.65, env CAIRN_VECTOR_DIST_MAX)
   3. fuse BM25 + vector ranks via RRF, min-max normalise
   4. filter by entity_type whitelist from permission matrix
   5. final_score = (0.55·rrf + 0.25·importance + 0.20·recency) × trust_boost
      (trust boost: +10% per manual edit, capped at +30%)
   6. top-k, return
        │
        ▼
 importance.rs: reinforce strength on read
        │
        ▼
 audit:tools/call (signed)  ──► return JSON to host
```

## Storage layout

```
~/Library/Application Support/Cairn/         (macOS — see `directories` crate for cross-platform paths; override with CAIRN_DATA_DIR)
├── memory.db                                SQLite (FTS5 + sqlite-vec) — entities, notes, relations, audit, embeddings
├── memory.db-wal                            WAL
├── memory.db-shm                            Shared memory
├── providers.json                           LLM provider config — contains API keys
├── api_endpoint.json                        REST endpoint handshake {host, port, token?} for local callers
├── mcp_bridge.json                          MCP SSE bridge settings
├── selection_popover.json                   Selection popover settings
└── backups/                                 Automatic pre-migration DB backups (newest 10 kept)
```

The Ed25519 signing key is not stored here — it lives in the OS keychain; the in-DB `audit_key` table remains only as a legacy fallback and is migrated automatically. Logs go to stdout/stderr; there is no log file.

With the `encrypted` build feature and `CAIRN_ENCRYPT=1` set, the SQLite file is SQLCipher-encrypted using a key sourced from the OS keychain.

## Key design decisions (and why)

| Decision | Rationale |
|---|---|
| **Tauri over Electron** | 60 MB installer vs 200+ MB, ~80 MB idle RSS vs ~400 MB, native window perf. Webview is OS-provided, not bundled. |
| **Rust core in a separate crate** | Compile-time guarantees on schema, ownership, FFI safety. Same crate ships GUI + MCP server + migrate binary. |
| **SQLite + FTS5, not Postgres** | Single-file, zero-config, can be `cp`'d for backup. FTS5 BM25 is fast enough for 100K-1M entities. |
| **sqlite-vec, not external vector DB** | Same SQLite file holds vectors next to entities — no two-store sync problem. |
| **JSON-Schema validation twice** | Once at decode (`tool_use` parsing), once again on the assembled object. Catches model schema drift and downstream bugs separately. |
| **Hash chain + Ed25519, not "trust us"** | Cryptographic verification is the only honest privacy posture. Public key is the user's; anyone can run `verify_recent(limit)` offline (most recent 500 entries by default; a large limit walks the full chain). |
| **Per-agent × per-entity-type permission matrix** | Finer than MemoryLake-style per-project ACL; matches the intuition "Claude can read my code preferences but not my finances". |
| **MCP as the primary IO** | The protocol is already a standard and MCP is the agent contract; the localhost REST API (`127.0.0.1:7716`) serves the browser extension and other local callers. |
| **OS keychain for signing key** | The audit chain is only as strong as the key custody. Apple Keychain / Linux secret-service / Windows Credential Manager all give us hardware-backed storage where available. |
| **macOS-only popover / hotkey first** | Capture surfaces are platform-specific; ship the best macOS experience first and port later. |

## Out of scope (today)

- **Multi-device sync** — Phase 3 will introduce CRDT-based sync; the audit chain is designed to merge cleanly.
- **Web UI** — Cairn is local-first by design; a web mirror is a Phase 3+ concern.
- **Multimodal capture** (images / video / audio) — text first; embed-side will be the harder lift.
- **Cloud-managed memory** — explicitly not on the roadmap. If you want a cloud product, use MemoryLake or Mem0.

## Phase roadmap (one-liner)

1. **Phase 1** — Foundation. App + MCP + audit + Anthropic/Gateway extraction. ✅
2. **Phase 2 (shipped)** — Hierarchical memory (episodic → semantic via sleep consolidation). Vector retrieval (RRF-fused). Multi-provider LLM settings (`providers.json` + live reload). Localhost REST API + browser extension. MCP SSE bridge. Dev-tool memory import/export. SQLCipher encryption (`encrypted` feature + `CAIRN_ENCRYPT=1`). ✅
3. **Phase 3 (planned)** — On-device extraction (Qwen 2.5-1.5B distilled, MLX). Personal embedding fine-tuning. CRDT multi-device sync.
4. **Phase 4** — BBS+ selective-disclosure proofs. Family Context (shared schemas, separate data).
