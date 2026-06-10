# Architecture

Cairn is a **Personal Memory / Context OS** that lives on your machine and speaks **MCP** to every agent you grant access to. This document describes the runtime architecture, key modules and the design decisions behind them.

For setup and contributor instructions see [DEVELOPMENT.md](./DEVELOPMENT.md).

## Design pillars

| Pillar | What it means concretely |
|---|---|
| **Local-first** | Cairn runs as a Tauri desktop app. All data lives in a single SQLite file under `~/Library/Application Support/cairn/`. Nothing leaves the device unless you explicitly grant access. |
| **Protocol-neutral** | Every external agent reads memory through a standard MCP (Model Context Protocol) server. Claude Desktop, Cursor, Cline, Codex — any MCP host plugs in the same way. |
| **Verify, don't trust** | Every write and every read is recorded on an append-only **Merkle chain** with an **Ed25519** signature per entry. You hold the public key; anyone can verify history wasn't rewritten. |
| **Schema-constrained extraction** | LLM output is validated against JSON Schema twice — the model is asked to emit structured `tool_use` calls, and the resulting object is re-validated before it hits the DB. |
| **Importance-aware retrieval** | Search blends BM25 (FTS5), entity-type filtering, Ebbinghaus recency decay (`R = exp(-Δt / S)`) and access-reinforced strength. |

## High-level diagram

```
                    ┌──────────────────────────────────────────────┐
                    │           Cairn Desktop App (Tauri 2)        │
                    │  ┌──────────────────────────────────────┐    │
                    │  │  Next.js 15 UI (static export)       │    │
                    │  │  /  /entities  /agents  /audit  ...  │    │
                    │  └─────────────────┬────────────────────┘    │
                    │                    │ Tauri IPC               │
                    │  ┌─────────────────▼────────────────────┐    │
                    │  │  Rust core (cairn_lib)               │    │
                    │  │   capture · extract · embed ·        │    │
                    │  │   importance · retrieval ·           │    │
                    │  │   audit (Merkle + Ed25519) ·         │    │
                    │  │   consolidation · keystore           │    │
                    │  └──┬───────────────────────────┬───────┘    │
                    │     │                           │            │
                    │     │ sqlx                      │ stdio      │
                    │     ▼                           ▼            │
                    │  ┌──────────────┐    ┌───────────────────┐  │
                    │  │  SQLite +    │    │  cairn-mcp        │  │
                    │  │  FTS5 +      │    │  (JSON-RPC 2.0    │  │
                    │  │  sqlite-vec  │    │   over stdio)     │  │
                    │  └──────────────┘    └─────────┬─────────┘  │
                    └────────────────────────────────┼────────────┘
                                                     │
                                       ┌─────────────┼──────────────┐
                                       ▼             ▼              ▼
                                 Claude Desktop   Cursor       Codex / any MCP host
```

Three binaries ship from the same Rust crate (`memory/src-tauri/Cargo.toml`):

- **`cairn`** — the GUI app launched by Tauri.
- **`cairn-mcp`** — a standalone MCP server over stdio, spawned by MCP hosts. Shares the same database as the GUI.
- **`cairn-migrate`** — one-shot plaintext → encrypted SQLite migration (built with `--features encrypted`, which vendors SQLCipher).

## Rust core modules

Source: `memory/src-tauri/src/`.

| Module | Role |
|---|---|
| `lib.rs` | Tauri app builder + shared `AppState`. |
| `commands.rs` | IPC commands exposed to the Next.js UI via `invoke`. |
| `schema.rs` | Domain types mirroring `memory/src/lib/types.ts`. |
| `db/` | sqlx pool, migrations, query helpers (entities, notes, audit, embeddings). |
| `capture/` | Multi-surface ingest — manual notes, global hotkey, selection popover, email/IMAP, iCal calendar. |
| `extract/` | LLM-driven entity extraction (Anthropic, Vercel AI Gateway, or local). Strict JSON-Schema validation. |
| `importance.rs` | Ebbinghaus strength model. Importance reinforced on every access. |
| `retrieval.rs` | Hybrid retrieval — BM25 (FTS5) + entity-type filter + importance + recency, re-ranked. |
| `embed/` · `embed_pipeline.rs` · `vec_init.rs` | Phase-2 embeddings via `sqlite-vec`, async pipeline. |
| `consolidation/` | Sleep-time worker that distils episodic notes into semantic entities. |
| `audit.rs` | Append-only Merkle chain `(prev_hash → hash)` + Ed25519 signature per entry. `verify_chain()` walks the whole history offline. |
| `mcp/` · `mcp_bridge.rs` · `bin/mcp_stdio.rs` | JSON-RPC 2.0 MCP server. Per-agent × per-entity-type permission matrix. Every call is logged + signed. |
| `selection_popover/` · `global_shortcut.rs` | macOS Accessibility + CGEventTap selection popover and global hotkey. |
| `keystore/` | OS keychain integration (Apple Keychain / Linux secret-service / Windows Credential Manager) for signing keys. |
| `providers.rs` | LLM provider abstraction — gateway / direct anthropic / local. |
| `signals.rs` · `usage.rs` | Lightweight telemetry (all local). |

## End-to-end write path

```
capture (UI / hotkey / popover / IMAP / iCal)
        │
        ▼
 raw note → SQLite (status = "pending")  ──► audit:capture
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
 SQLite: entities + relations + provenance  ──► audit:extract
        │
        ▼
 importance.rs: assign initial strength
        │
        ▼
 embed (optional, async) ──► sqlite-vec  ──► audit:embed
```

## End-to-end read path (MCP)

```
host (Claude Desktop / Cursor) ──► cairn-mcp (stdio, JSON-RPC 2.0)
        │
        ├─ authenticate(agent_id)
        │
        ├─ check permission matrix (agent_id × entity_type → allow/deny)
        │      │
        │      └─ deny ──► audit:access-denied + return error
        │
        ▼
 retrieval.rs:
   1. FTS5 BM25 over (entities.subject || ' ' || entities.object || ' ' || notes.text)
   2. filter by entity_type whitelist from permission matrix
   3. importance_score = base * exp(-Δt / strength)
   4. final_score = α·bm25 + β·importance + γ·recency
   5. top-k, re-rank, return
        │
        ▼
 importance.rs: reinforce strength on read
        │
        ▼
 audit:read (signed)  ──► return JSON to host
```

## Storage layout

```
~/Library/Application Support/cairn/         (macOS — see `directories` crate for cross-platform paths)
├── cairn.db                                 SQLite (FTS5 + sqlite-vec) — entities, notes, relations, audit, embeddings
├── cairn.db-wal                             WAL
├── cairn.db-shm                             Shared memory
├── keystore/                                Ed25519 signing key — actually lives in OS keychain;
│                                            this directory only holds a key id pointer
└── logs/
    └── cairn.log
```

With `--features encrypted` (Phase 3b), the SQLite file is SQLCipher-encrypted using a key sourced from the OS keychain.

## Key design decisions (and why)

| Decision | Rationale |
|---|---|
| **Tauri over Electron** | 60 MB installer vs 200+ MB, ~80 MB idle RSS vs ~400 MB, native window perf. Webview is OS-provided, not bundled. |
| **Rust core in a separate crate** | Compile-time guarantees on schema, ownership, FFI safety. Same crate ships GUI + MCP server + migrate binary. |
| **SQLite + FTS5, not Postgres** | Single-file, zero-config, can be `cp`'d for backup. FTS5 BM25 is fast enough for 100K-1M entities. |
| **sqlite-vec, not external vector DB** | Same SQLite file holds vectors next to entities — no two-store sync problem. |
| **JSON-Schema validation twice** | Once at decode (`tool_use` parsing), once again on the assembled object. Catches model schema drift and downstream bugs separately. |
| **Merkle chain + Ed25519, not "trust us"** | Cryptographic verification is the only honest privacy posture. Public key is the user's; anyone can `verify_chain()` offline. |
| **Per-agent × per-entity-type permission matrix** | Finer than MemoryLake-style per-project ACL; matches the intuition "Claude can read my code preferences but not my finances". |
| **MCP as the primary IO** | The protocol is already a standard. REST/SDK can come later if useful, but MCP is the contract. |
| **OS keychain for signing key** | The audit chain is only as strong as the key custody. Apple Keychain / Linux secret-service / Windows Credential Manager all give us hardware-backed storage where available. |
| **macOS-only popover / hotkey first** | Capture surfaces are platform-specific; ship the best macOS experience first and port later. |

## Out of scope (today)

- **Multi-device sync** — Phase 3 will introduce CRDT-based sync; the audit chain is designed to merge cleanly.
- **Web UI** — Cairn is local-first by design; a web mirror is a Phase 3+ concern.
- **Multimodal capture** (images / video / audio) — text first; embed-side will be the harder lift.
- **Cloud-managed memory** — explicitly not on the roadmap. If you want a cloud product, use MemoryLake or Mem0.

## Phase roadmap (one-liner)

1. **Phase 1 (current)** — Foundation. App + MCP + audit + Anthropic/Gateway extraction. ✅
2. **Phase 2** — On-device extraction (Qwen 2.5-1.5B distilled, MLX). Hierarchical memory (episodic → semantic via sleep consolidation).
3. **Phase 3** — Personal embedding fine-tuning. CRDT multi-device sync.
4. **Phase 4** — BBS+ selective-disclosure proofs. Family Context (shared schemas, separate data).
