# Architecture

> The product looks like a note app. The interesting work is in five places:
> schema-constrained extraction, hierarchical memory, hybrid retrieval,
> cryptographic audit, and the MCP server.

## 1. Layers at a glance

```
┌──────────────────────────────────────────────────────────────────┐
│  Next.js UI (static export) — Capture / Entities / Agents / Audit │
└──────────────────────────────┬───────────────────────────────────┘
                               │ Tauri IPC (invoke + emit)
┌──────────────────────────────┴───────────────────────────────────┐
│  Rust app core (src-tauri/src/lib.rs · AppState)                  │
│                                                                   │
│   ┌──────────┐    ┌──────────────┐    ┌─────────────────────┐    │
│   │  db::Db  │ ←→ │ extract::*   │ ←→ │ audit::AuditLogger  │    │
│   │ sqlx +   │    │ Anthropic +  │    │ Merkle chain + Ed25519│   │
│   │ FTS5     │    │ JSON Schema  │    │                     │    │
│   └────┬─────┘    └──────────────┘    └─────────────────────┘    │
│        │                                                          │
│        │           ┌──────────────────────────────────────────┐  │
│        └─────────→ │ retrieval::Retriever                     │  │
│                    │   BM25 + importance + recency re-rank    │  │
│                    └──────────────────────────────────────────┘  │
│                               │                                   │
│                    ┌──────────┴──────────┐                        │
│                    │  mcp::serve_stdio   │ JSON-RPC 2.0           │
│                    └──────────┬──────────┘                        │
└───────────────────────────────┼──────────────────────────────────┘
                                ▼
                  Claude Desktop / Cursor / ChatGPT
```

The Tauri app and the standalone `cairn-mcp` binary both link the same
`cairn_lib` crate. The CLI binary opens the same SQLite database
in WAL mode so the GUI and an MCP client can run concurrently.

## 2. Data model

Eight entity types: `Person`, `Event`, `Preference`, `Belief`, `Goal`,
`Asset`, `Skill`, `Location`. Each entity has:

- canonical `name` (e.g. `"妈"`, not normalised to English)
- typed `properties` JSON
- LLM `confidence` (rejected below 0.6)
- **Ebbinghaus state**: `strength`, `last_accessed`, `access_count`, `base_importance`
- `source_note_id` linking back to the raw note

Relations are first-class: `(from_entity, to_entity, type)` with provenance
back to the source note.

Both `notes` and `entities` are mirrored into FTS5 virtual tables for BM25
retrieval. Triggers keep them in sync.

## 3. Extraction (Phase 1 = Anthropic, Phase 2 = on-device)

`src-tauri/src/extract/`

The LLM is treated as an *untyped function* and constrained two ways:

1. **Tool-use API**. We define a `save_extraction` tool whose `input_schema`
   mirrors our internal `EXTRACTION_SCHEMA` and force `tool_choice` to it.
   Anthropic enforces structural validity server-side.
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

Phase 2 swaps in a `LocalProvider` backed by a Qwen 2.5-1.5B-Instruct model
distilled from Claude Opus on synthetic notes, served via MLX on Apple
Silicon. Same trait, same JSON Schema validation, ~80ms/note, $0/note,
offline-capable.

## 4. Importance scoring (Ebbinghaus)

`src-tauri/src/importance.rs`

```
R(t) = exp(-Δt / S)              ← retention probability
S'   = S + 2 / ln(1 + n + 1)     ← reinforcement, diminishing returns
I    = clamp01(base + freq) × R  ← composite importance
```

- `base_importance` per type: Person 0.80, Belief 0.70, Preference 0.65, …
- `strength` starts at 1.0 day-decay; each access tops it up.
- Used by both the retriever (for re-ranking) and any future "forget" job
  that prunes entities below an importance floor.

Phase 2 replaces the hand-tuned weights with a learned ranker trained on
user accept/reject signals.

## 5. Hybrid retrieval

`src-tauri/src/retrieval.rs`

For each query:

1. Run **BM25** over notes and entities (separate FTS5 tables, in parallel).
2. **Filter** entity types by the agent's permission set.
3. Compute per-result **importance** and **recency** (half-life 7d).
4. Min-max normalise BM25, then weighted sum:

```
final = 0.5 · bm25_norm + 0.3 · importance + 0.2 · recency
```

5. Sort descending and return.

The query string is converted to FTS5 `"tok1" OR "tok2" …` form so partial
hits surface — the heavy lifting is done in the re-rank step.

Phase 2 adds a vector retriever as a third scorer; the weighted-sum loop is
unchanged.

## 6. Cryptographic audit log

`src-tauri/src/audit.rs`

Every MCP access (and extraction job) appends an `audit_log` row containing:

```
id, agent_id, action, tool_name, arguments,
result_hash = SHA-256(JSON(result)),
timestamp,
prev_hash   = hash of previous row,
hash        = SHA-256(canonical JSON of this row, sorted keys, no hash/sig),
signature   = Ed25519(hash)
```

Three guarantees:

1. **Append-only chain**: any deletion or reordering of a past row breaks
   `prev_hash` continuity. `verify_recent()` checks the full chain.
2. **Tamper detection**: rewriting a field changes the recomputed `hash`,
   which no longer matches the stored `hash`. (Verified in the unit test
   `tamper_detected`.)
3. **Non-repudiation**: even if the SQL file were freely editable, an
   attacker would also need the Ed25519 signing key (kept in
   `audit_key` table — Phase 2 moves to OS keychain) to forge new entries
   that pass `verify_recent()`.

The user (or any third-party auditor) gets the public key from the UI's
Audit page or `auditPublicKey()` IPC and can independently verify the chain.

## 7. MCP server

`src-tauri/src/mcp/`

Full JSON-RPC 2.0 over stdio, matching the **2025-11-25** MCP spec.

Methods:
- `initialize` → server capabilities + `instructions`
- `tools/list` → 4 tools
- `tools/call`
- `notifications/initialized` (ack)
- `ping`

Tools exposed:
- `search_memory(query, types?, limit?)` — hybrid retrieval, scoped by agent permissions.
- `get_preferences(domain?)` — fast path for the most common case.
- `list_recent_notes(limit?)` — raw notes, requires `'*'` read scope.
- `record_observation(text)` — write, requires `write` scope; queues for extraction.

Every tool call:
1. Touches the agent (`last_seen`).
2. Applies permission filter (returns `denied: true` instead of error if
   the agent has no scope — Anthropic models handle this gracefully).
3. On read: calls `touch_entity()` for each returned entity → Ebbinghaus reinforcement.
4. Appends a signed audit entry.

Logs all go to stderr (stdout is the JSON-RPC channel).

## 8. The shape of Phase 2+

Already shaped in the architecture:

| Future capability | What's already in place |
|---|---|
| On-device extraction | `ExtractionProvider` trait — swap `Arc<dyn>` at startup. |
| Hierarchical memory (Episodic→Semantic) | `strength` + `last_accessed` + `access_count` columns already track the inputs a consolidator needs. |
| Vector retrieval | `Retriever` accepts pluggable scorers; weighted-sum loop won't change. |
| Selective-disclosure proofs (BBS+) | Audit log already hashes results and signs deterministically — a prover can attest "agent X only ever called tool Y returning H(result) ∈ S". |
| Multi-device CRDT sync | Notes have ULIDs (monotonic, k-sortable); entities dedupe by (type, name COLLATE NOCASE), which is CRDT-friendly. |
