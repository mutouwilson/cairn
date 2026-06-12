# Architecture

> The product looks like a note app. The interesting work is in five places:
> schema-constrained extraction, hierarchical memory, hybrid retrieval,
> cryptographic audit, and the MCP server.

## 1. Layers at a glance

```
┌────────────────────────────────────────────────────────────────────┐
│  Next.js UI (static export) — Capture / Entities / Agents / Audit  │
└──────────────────────────────┬─────────────────────────────────────┘
                               │ Tauri IPC (invoke + emit)
┌──────────────────────────────┴─────────────────────────────────────┐
│  Rust app core (src-tauri/src/lib.rs · AppState)                   │
│                                                                    │
│   ┌──────────────┐    ┌────────────────┐    ┌───────────────────┐  │
│   │  db::Db      │ ←→ │ extract::*     │ ←→ │ audit::AuditLogger│  │
│   │ sqlx · FTS5  │    │ LiveProviders  │    │ hash chain +      │  │
│   │ · sqlite-vec │    │ (10 families)  │    │ Ed25519 signatures│  │
│   │              │    │ + JSON Schema  │    │                   │  │
│   └────┬─────────┘    └────────────────┘    └───────────────────┘  │
│        │                                                           │
│        │           ┌──────────────────────────────────────────┐    │
│        └─────────→ │ retrieval::Retriever                     │    │
│                    │   RRF(BM25, vector kNN) → weighted       │    │
│                    │   importance + recency, × trust boost    │    │
│                    └──────────────────────────────────────────┘    │
│                                                                    │
│   background: consolidation worker (900 s) · import watcher ·      │
│               CaptureSupervisor (IMAP / iCal)                      │
│                                                                    │
│   ┌──────────────────┐  ┌────────────────┐  ┌──────────────────┐   │
│   │ mcp::serve_stdio │  │ api_server     │  │ mcp_bridge       │   │
│   │ (JSON-RPC 2.0    │  │ REST :7716     │  │ MCP SSE :7717    │   │
│   │  over stdio)     │  │ always-on      │  │ Settings toggle  │   │
│   └────────┬─────────┘  └───────┬────────┘  └────────┬─────────┘   │
└────────────┼────────────────────┼────────────────────┼─────────────┘
             ▼                    ▼                    ▼
  Claude Desktop / Cursor   Browser Extension    SSE-capable MCP
  / any MCP host            (ChatGPT, Gemini,    clients
                             Doubao, …)
```

The Tauri app and the standalone `cairn-mcp` binary both link the same
`cairn_lib` crate. The CLI binary opens the same SQLite database
in WAL mode so the GUI and an MCP client can run concurrently.

Background subsystems live inside the app process: the consolidation
worker (default 900 s tick), an import scanner/watcher that pulls
user-level memory files from Claude Code, Cursor and Codex into the
capture pipeline, and the `CaptureSupervisor` driving IMAP/iCal polling.
External chat UIs (ChatGPT, Gemini, Doubao — 12+ sites) reach Cairn
through the browser extension, which talks to the always-on REST API on
`127.0.0.1:7716`; the MCP SSE bridge on `127.0.0.1:7717` is opt-in via
Settings. A `bundle` module handles signed export/import of memory
bundles.

## 2. Data model

Eight entity types: `Person`, `Event`, `Preference`, `Belief`, `Goal`,
`Asset`, `Skill`, `Location`. Each entity has:

- canonical `name` (e.g. `"妈"`, not normalised to English)
- typed `properties` JSON
- LLM `confidence` (rejected below 0.6)
- **Ebbinghaus state**: `strength`, `last_accessed`, `access_count`, `base_importance`
- `is_sticky` (survives cascade deletion) and `archived_at` (soft-archive
  timestamp — archived entities are excluded from retrieval)
- `source_note_id` linking back to the raw note

Relations are first-class: `(from_entity, to_entity, type)` with provenance
back to the source note.

Two side tables support entity governance: `entity_aliases` routes
alternate names to a canonical entity, and `entity_edits` records manual
edits (which feed the retrieval trust boost).

Both `notes` and `entities` are mirrored into FTS5 virtual tables for BM25
retrieval. Triggers keep them in sync.

## 3. Extraction (multi-provider, schema-constrained)

`src-tauri/src/extract/`

The LLM is treated as an *untyped function* and constrained two ways:

1. **Tool-use API**. We define a `save_extraction` tool whose `input_schema`
   mirrors our internal `EXTRACTION_SCHEMA` and force `tool_choice` to it.
   The provider enforces structural validity server-side.
2. **Re-validation**. The parsed payload is re-checked with the `jsonschema`
   crate against the same schema. Defence in depth — if the API ever drifts,
   we still catch it.

Few-shot exemplars are kept short (two pairs) but cover both a high-emotion
event note and a flat preference note, so the model calibrates style and
confidence appropriately.

The provider is hidden behind a trait:

```rust
#[async_trait]
pub trait ExtractionProvider: Send + Sync {
    async fn extract(&self, text: &str) -> Result<ExtractionResult>;
}
```

Ten provider families ship behind this trait: Anthropic (native Messages
API) plus OpenAI, Gemini, Vercel AI Gateway, OpenRouter, Doubao, Minimax,
Kimi (Moonshot), GLM and Qwen (DashScope), all driven through a single
OpenAI-compatible client. Provider config lives in `providers.json` in the
data dir, wrapped by `LiveProviders` — edits in Settings hot-reload
immediately, no restart. The extract and embed slots are independent and
both optional: with no embed provider configured, retrieval degrades
gracefully to pure BM25.

On-device extraction (a distilled Qwen model served via MLX) is planned
but not yet implemented — see §8.

## 4. Importance scoring (Ebbinghaus)

`src-tauri/src/importance.rs`

```
R(t) = exp(-Δt / S)              ← retention probability
S'   = S + 2 / (1 + ln(n + 1))   ← reinforcement, diminishing returns
I    = clamp01(base + freq) × R  ← composite importance
```

- `base_importance` per type: Person 0.80, Belief 0.70, Preference 0.65, …
- `strength` starts at 1.0 day-decay; each access tops it up.
- Used by the retriever (for re-ranking) and by the forget job: each
  consolidation tick also runs `auto_archive_stale_entities()`, which
  soft-archives entities untouched for ~6 months (no access, no manual
  edit) by setting `archived_at`; archived entities are excluded from
  retrieval.

A learned ranker trained on user accept/reject signals remains future
work.

## 5. Hybrid retrieval

`src-tauri/src/retrieval.rs`

For each query:

1. Run **BM25** over notes and entities (separate FTS5 tables) and a
   **vector kNN** over sqlite-vec, in parallel. Vector hits with cosine
   distance above 0.65 are dropped (`CAIRN_VECTOR_DIST_MAX` overrides).
2. Fuse the BM25 and vector rank lists with **RRF** (k = 60) — reciprocal
   rank fusion, not a weighted third scorer.
3. **Filter** entity types by the agent's permission set.
4. Compute per-result **importance** and **recency** (half-life 7d).
5. Min-max normalise the RRF score, weighted-sum, then multiply by the
   manual-edit trust boost (+10% per human edit, capped at +30%):

```
final = (0.55 · rrf_norm + 0.25 · importance + 0.20 · recency) × trust_boost
```

6. Sort descending and return.

The query string is converted to FTS5 `"tok1" OR "tok2" …` form so partial
hits surface — the heavy lifting is done in the re-rank step. Queries
containing tokens shorter than 3 characters (most CJK queries) bypass the
trigram FTS index and fall back to a LIKE substring search.

Notes are matched by BM25 only — no vector fusion — and scored with a flat
BM25 prior: `0.6 + 0.4 · recency`.

## 6. Cryptographic audit log

`src-tauri/src/audit.rs`

Every MCP access (and extraction job) appends an `audit_log` row containing:

```
seq, id, agent_id, action, tool_name, arguments,
result_hash  = SHA-256(JSON(result)),
result_count = number of results returned,
timestamp,
prev_hash    = hash of previous row,
hash         = SHA-256(canonical JSON of this row, sorted keys, no hash/sig;
               result_count is part of the canonical payload),
signature    = Ed25519(hash)
```

Three guarantees:

1. **Append-only chain**: any deletion or reordering of a past row breaks
   `prev_hash` continuity. `verify_recent(limit)` checks the most recent
   N entries (default 500); pass a larger limit to walk the full chain.
2. **Tamper detection**: rewriting a field changes the recomputed `hash`,
   which no longer matches the stored `hash`. (Verified in the unit test
   `tamper_detected`.)
3. **Non-repudiation**: even if the SQL file were freely editable, an
   attacker would also need the Ed25519 signing key (held in the OS
   keychain; the legacy `audit_key` table remains only as a fallback and
   is migrated automatically) to forge new entries that pass
   `verify_recent()`.

The user (or any third-party auditor) gets the public key from the UI's
Audit page or `auditPublicKey()` IPC and can independently verify the chain.

## 7. MCP server

`src-tauri/src/mcp/`

Full JSON-RPC 2.0, matching the **2025-11-25** MCP spec, over two
transports: stdio (the `cairn-mcp` binary) and SSE (`GET /sse` +
`POST /messages?session_id=…`), the latter hosted by the bridge on
`127.0.0.1:7717`. The agent identity comes from the `CAIRN_AGENT_ID` env
var (default `claude-desktop`) — there is no authenticate handshake.

Methods:
- `initialize` → server capabilities + `instructions`
- `tools/list` → 5 tools
- `tools/call`
- `notifications/initialized` (ack)
- `ping`

Tools exposed:
- `search_memory(query, types?, limit?)` — hybrid retrieval, scoped by agent permissions.
- `get_preferences(domain?)` — fast path for the most common case.
- `get_themes(limit?, topic_prefix?)` — consolidated semantic-memory themes; the high-signal path for "what's been going on with X".
- `list_recent_notes(limit?)` — raw notes, requires `'*'` read scope.
- `record_observation(text)` — write, requires `write` scope; queues for extraction.

Every tool call:
1. Touches the agent (`last_seen`).
2. Applies permission filter (returns `denied: true` instead of error if
   the agent has no scope — Anthropic models handle this gracefully).
3. On read: calls `touch_entity()` for each returned entity → Ebbinghaus reinforcement.
4. Appends a signed audit entry.

Logs all go to stderr (stdout is the JSON-RPC channel).

## 8. Phase 2+ scorecard

What has shipped since Phase 1, and what remains future:

| Capability | Status |
|---|---|
| Multi-provider extraction | **Delivered.** `ExtractionProvider` trait + `LiveProviders` — providers hot-swap at runtime whenever `providers.json` changes, not just at startup. On-device extraction (Qwen 2.5-1.5B-Instruct distilled from Claude Opus on synthetic notes, served via MLX on Apple Silicon — same trait, same JSON Schema validation, ~80ms/note, $0/note, offline-capable) is planned, not yet implemented. |
| Hierarchical memory (Episodic→Semantic) | **Delivered in full.** `semantic_memories` + `consolidation_runs` tables, a 900 s background worker, supersede chains (`PRAGMA defer_foreign_keys`), the `/themes` page and the `get_themes` MCP tool. |
| Vector retrieval | **Delivered** — fused with BM25 via RRF rather than added as a weighted third scorer. |
| Selective-disclosure proofs (BBS+) | Future. Audit log already hashes results and signs deterministically — a prover can attest "agent X only ever called tool Y returning H(result) ∈ S". |
| Multi-device CRDT sync | Future. Notes have ULIDs (monotonic, k-sortable); entities dedupe by (type, name COLLATE NOCASE), which is CRDT-friendly. Entity governance shipped meanwhile: alias routing, manual merge, and `dedup_rejections` so rejected pairs never re-merge. |
