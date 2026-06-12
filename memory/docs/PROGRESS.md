# Progress Log

Chronological record of what each phase delivered. Updated whenever a milestone
lands on `main`. Lists are kept honest: nothing here is aspirational; aspirations
live in `ROADMAP.md`.

The product target is **a production-grade personal Memory / Context OS on
months-long horizon**. Every entry should make the next iteration cheaper and the
overall quality higher — no shortcut workarounds, no dangling TODOs, every claim
covered by a test or an explicit `untested` note.

---

## Catch-up entry (2026-06-12) — the alpha.1 → alpha.3 window

Everything below this entry was written on 2026-05-13; this entry compresses
the four weeks since. Day-to-day changes are recorded in the root
`CHANGELOG.md` from here on — this file stays the per-phase narrative.

**Shipped since 2026-05-13**

- **Phase 6b · In-app MCP SSE bridge** — Settings toggle boots an in-process
  SSE server on `127.0.0.1:7717` (`mcp_bridge.rs`); shares the GUI's DB
  handle. Standalone `cairn-mcp --transport sse` kept for headless use.
- **Phase 6d · Provider settings page** — 10 provider families (Vercel
  Gateway, OpenRouter, OpenAI, Anthropic, Gemini, Doubao, Minimax, Moonshot,
  GLM, Qwen) configurable in-app, hot-reloaded without restart.
- **Local REST API server** — always-on `127.0.0.1:7716` (`CAIRN_API_PORT`;
  optional `CAIRN_API_TOKEN` bearer auth); endpoints status / capture /
  search / recent / themes; the browser extension's backend.
- **i18n** — full `en` + `zh-CN` dictionaries under `src/lib/i18n`.
- **Phase 7 · V6 dev-tool memory import/export** — CLAUDE.md / Cursor /
  Codex import, managed-block export, file watcher, skip + unlinked-doc
  handling.
- **Browser extension completed** (5e was a scaffold) — 12+ supported sites,
  passive recall, IME fixes; distributed as a load-unpacked zip with each
  GitHub Release.
- **Release + CI infrastructure** — v0.1.0-alpha.1 → alpha.3 shipped;
  `release.yml` builds macos-arm64 / linux-x64 / windows-x64 bundles + the
  extension zip; `ci.yml` runs migrations-immutable / frontend / rust jobs.
- **Relicensed** to FSL-1.1-ALv2.
- **Migration guardrails** — immutable-migrations CI gate, pre-migration
  runtime backups, prev-release upgrade smoke test
  (`cargo test --test upgrade` via `CAIRN_UPGRADE_OLD_MIGRATIONS_DIR`).
- **Consolidation fixes (×3)** — CJK boundary panic, supersede deadlock,
  worker hot-reconfiguration.

**Verification snapshot (2026-06-12)** — `pnpm build` ✓ **12 static routes** ·
MCP `tools/list` **5 tools** · **16 migrations** in
`memory/src-tauri/migrations/`.

---

## Phase 1 — Foundation (2026-05-13 ✓)

**Capability shipped:** capture → extract → store → query → MCP → audit, end-to-end.

**Modules landed**
- `schema.rs` — 8 entity types (Person/Event/Preference/Belief/Goal/Asset/Skill/Location), relations, draft types. Properties typed as JSON columns; sensitivity ranking baked in.
- `db/` — SQLite + sqlx + WAL. Initial migration with FTS5 mirrors for notes and entities (triggers keep them in sync). Indices on type/name/updated_at.
- `extract/anthropic.rs` — Claude `tool_use` for schema-constrained decoding, two few-shot pairs, retries with exponential backoff.
- `extract/validate.rs` — `jsonschema`-crate validation of the LLM payload as defence in depth.
- `importance.rs` — Ebbinghaus model: `R = exp(-Δt/S)`, reinforcement with diminishing returns, type-specific base importance.
- `retrieval.rs` — Hybrid BM25 + importance + recency weighted re-rank (0.5/0.3/0.2). Pluggable shape ready for the vector layer.
- `audit.rs` — Merkle hash chain + Ed25519 signature per entry. Generates and stores keypair on first run; exposes `verify_recent()` and the public key.
- `mcp/` — Full JSON-RPC 2.0 stdio server, MCP spec 2025-11-25. Four tools: `search_memory`, `get_preferences`, `list_recent_notes`, `record_observation`. Permission-matrix filtered, audited on every call, Ebbinghaus reinforcement on read.
- `commands.rs` — Tauri IPC: capture/list_notes/list_entities/get_entity/search/agents/permissions/audit/verify.
- `bin/mcp_stdio.rs` — Standalone binary for Claude Desktop / Cursor.

**UI landed**
- Next.js 15 static export, Tauri 2 webview host.
- Pages: `/` Capture+Timeline, `/entities`, `/entity` (detail), `/agents` (permissions matrix), `/audit` (Merkle viewer + verify button).
- Components: `Capture` (⌘+Enter), `Timeline` (live via `note_processed` event), `EntityChip` (color-coded by type), permissions matrix UI with per-cell scope toggle.

**Verification**
- `cargo check` ✓ · `cargo test --lib` 17/17 ✓ (incl. `audit::tests::tamper_detected`).
- `cargo build --release --bin cairn-mcp` ✓
- `pnpm exec tsc --noEmit` ✓ · `pnpm build` ✓ (5 routes statically exported).
- MCP smoke test ✓ — `initialize` → `tools/list` (4 tools) → `tools/call` round-trip.

---

## Phase 2 — Real technical depth (2026-05-13 ✓ all four)

### 2a · Provider routing via Vercel AI Gateway

**Capability shipped:** swap any model behind the extraction pipeline via env config, no code change.

- `extract/openai_compat.rs` — shared OpenAI-style chat/completions client with function-calling tool. Generic `invoke_chat_tool` is reused by Phase 2c's consolidation worker.
- `extract/gateway.rs` — Vercel AI Gateway (`https://ai-gateway.vercel.sh/v1`). Auth via `AI_GATEWAY_API_KEY` / `AI_GATEWAY_TOKEN`. Base URL overridable.
- `extract/local.rs` — OpenAI-compatible local server (llama.cpp / MLX / Ollama / LM Studio). Endpoint via `CAIRN_LOCAL_ENDPOINT`.
- `extract/mod.rs` — `build_default()` selector: explicit env first, then auto-detect (gateway > anthropic > null).
- `extract/anthropic.rs` — preserved as legacy provider for users wanting direct Anthropic.
- Env contract: `CAIRN_EXTRACTION_PROVIDER`, `CAIRN_EXTRACTION_MODEL` (e.g. `anthropic/claude-haiku-4-5-20251001`, `openai/gpt-5-mini`, `google/gemini-2.5-flash`).

### 2b · On-device extraction model (distillation toolkit)

**Capability shipped:** reproducible pipeline to train a 1.5B Qwen LoRA that replaces the gateway call once distilled.

- `tools/distill/` Python project (`distill-{synth,train,eval,export}` scripts).
- `schema.py` — JSON Schema + Pydantic model mirroring `extract/prompt.rs::EXTRACTION_SCHEMA`. Drift between Rust and Python would be a CI fail.
- `synth.py` — two-stage (brainstorm → teacher-extract) via Vercel Gateway, resumable JSONL output, schema-validated, async with semaphore-limited concurrency.
- `seeds.py` — 5 stratification axes (language × domain × mood × entity-focus × complexity) for synthetic diversity.
- `train.py` — Qwen 2.5-1.5B-Instruct + LoRA (rank 32, alpha 64, attn+mlp target modules). Outputs adapter, tokenizer, and a `manifest.json` capturing the full run config.
- `eval.py` — per-type F1, relation F1, JSON-validity rate, latency P50/P95. Writes structured `eval.json` next to the run.
- `export.py` — LoRA merge → MLX 4-bit (Apple Silicon) or GGUF Q4_K_M (llama.cpp).
- `tests/test_schema.py` — 12 tests covering every entity-type round trip and edge cases.

The toolkit ships code-only — no trained weights bundled. README documents hardware budget, quality gates (per-type F1 floors), and end-to-end run.

### 2c · Hierarchical memory + sleep consolidation

**Capability shipped:** background worker turns clusters of episodic entities into stable semantic summaries; exposed to UI and to agents via MCP.

- Migration `0003_hierarchical_memory.sql` — `semantic_memories` (with partial-unique active-topic index + supersede chain) + `consolidation_runs` audit table.
- `consolidation/topics.rs` — three discovery strategies (person-centric, preference-domain-centric, goal-centric) with explicit "≥3 supporting entities" floor.
- `consolidation/summarize.rs` — schema-constrained LLM call (reuses `extract/openai_compat::invoke_chat_tool`) producing `{ title, summary, confidence }`.
- `consolidation/run.rs` — orchestrator: writes a `consolidation_runs` row, walks topics, upserts semantic memories with supersede linkage.
- `consolidation/worker.rs` — tokio task spawned by `AppState::init`. Cadence via `CAIRN_CONSOLIDATION_INTERVAL_SECS` (default 900 s); 0 disables.
- IPC: `trigger_consolidation`, `list_themes`, `get_theme`, `list_consolidation_runs`.
- MCP: new `get_themes` tool with `topic_prefix` filter and `*`-or-(Person+Goal) permission gate.
- UI: `/themes` page — active themes list, manual "run consolidation" button, recent runs strip.
- Tests: `consolidation::topics::tests::preference_domain_topic_emerges_at_three_entries` and `person_topic_requires_three_related_entities` — verify the discovery floor and evidence aggregation against a real SQLite DB.

### 2d · Vector retrieval (sqlite-vec)

**Capability shipped:** hybrid retrieval combining BM25 with vector kNN via Reciprocal Rank Fusion, behind the existing `Retriever::search` API.

- `vec_init.rs` — `sqlite3_auto_extension(sqlite_vec_init)` once-only registration. Wrapped in `Db::open` so every consumer (Tauri app + MCP binary + tests) loads it before opening connections.
- Migration `0002_vector.sql` — `entity_vecs` (vec0 virtual table, 1024-d cosine) + `entity_embedding_meta` (model/dim/text-hash so re-embeds are skipped when nothing changed).
- `embed/` — `EmbeddingProvider` trait, gateway impl (`/v1/embeddings`) and local impl. Default dimension 1024. Local server dim alignment handles Matryoshka models via truncation + L2 renorm.
- `embed_pipeline.rs` — idempotent per-note embedding orchestrator; `embed_entities` skips entities whose `(text_hash, model)` is unchanged. `embed_pending` drains the backlog.
- `retrieval.rs` — BM25 + vector kNN streams, fused via RRF (k=60), then weighted with importance (Ebbinghaus) and recency. Exposes per-result diagnostics.
- `commands.rs::reembed_pending` IPC for backfill after switching embedding models.
- Tests: `retrieval::tests::rrf_promotes_double_hits` proves entities present in both streams rank above either-stream hits.

---

## Phase 3 — Multi-device + privacy hardening (2026-05-13 ✓ four pieces shipped)

### 3a · Audit signing key → OS Keychain ✓

- `keystore/` module: `KeyStore` trait + `KeychainKeyStore` (via the `keyring` crate; cross-platform on macOS Keychain, Linux libsecret, Windows Credential Manager) + `DbKeyStore` (legacy + CI fallback) + `KeyStoreService` (primary/fallback + automatic legacy migration on first run).
- `audit::AuditLogger::init` rewritten to go through the keystore service. Legacy `audit_key` SQL rows are moved into the keychain on first start; primary write succeeds → fallback row deleted.
- Tests: `keystore::db_fallback::tests::db_keystore_round_trip` + `keystore::keychain::tests::round_trip_macos` (gated by `CAIRN_KEYCHAIN_TEST_DISABLE` for CI).
- Verified live: a fresh smoke run logs `generated fresh audit key store="keychain" account="audit-signing-key"`.

### 3b · Encrypted SQLite via SQLCipher (opt-in) ✓

- `encrypted` cargo feature: enables `libsqlite3-sys/bundled-sqlcipher-vendored-openssl`, so a `--features encrypted` build ships SQLCipher (no system OpenSSL required).
- `db::Db::open` reads `CAIRN_ENCRYPT=1` and, when set on an `encrypted`-built binary, derives the 32-byte page key from the keychain (`db-key` account) and applies `PRAGMA key = "x'…'"` + `PRAGMA cipher_compatibility = 4` via `SqlitePoolOptions::after_connect`.
- Fails closed: `CAIRN_ENCRYPT=1` on a binary built without the feature panics with a clear `rebuild with --features encrypted` message. Keychain is the *only* allowed location for the DB key — we never write the encryption key into the DB it is protecting.
- `cargo check` verified for both default and `--features encrypted` build paths.
- `docs/SETUP.md` documents the opt-in workflow plus the known migration gap (plaintext → encrypted migration tool is filed in `ROADMAP.md`).

### 3c · Signed JSON sync bundles + per-row LWW ✓

- New `bundle/` module + `migrations/0004_sync.sql` (`sync_log` audit table).
- Bundle format `cairn-bundle/v1`: header (format, ulid, device pubkey, counts, SHA-256 checksum) + payload (notes, entities, relations, themes) + Ed25519 signature over `SHA-256(header || payload)`. Themes are included as warm-start; the audit log, embeddings, agent permissions, and signing keys are **deliberately not** in the bundle.
- IPC: `export_bundle(path, device_name)` + `import_bundle(path, verify_signature)`.
- Conflict resolution: per-row LWW keyed by `(id, updated_at)` for entities and themes, by `created_at` for notes, "insert if absent" for relations.
- Tests:
  - `bundle::tests::roundtrip_export_import_preserves_data` — full export from DB-A → import into empty DB-B → assert rows + signature verified.
  - `bundle::tests::lww_keeps_newer_local_entity` — seeds B from A, bumps B `updated_at`, re-imports → all entities lose LWW (conflicts ≥ 2).
  - `bundle::tests::tampered_payload_fails_checksum` — flipping one byte in the payload triggers checksum failure.
  - `bundle::tests::tampered_signature_does_not_verify` — fake-but-well-formed signature gives `signature_ok=false`; data still imports so callers can decide.
- Every export / import writes a `sync_log` row with signature_ok, conflict count, error list.

> **Scope honesty:** real-time CRDT (yrs) + peer-to-peer transport (Iroh/libp2p) deferred to Phase 4. Phase 3c is hand-carry sync (file copy / AirDrop / shared folder). It is genuinely production-grade for that workflow.

### 3d · Personal-embedding fine-tuning — signal collection + offline trainer ✓

- `migrations/0005_retrieval_signals.sql`: one row per query event, JSON arrays for returned / clicked / dismissed / corrected ids.
- `signals.rs`: `Db::log_retrieval` / `Db::record_action` / `Db::list_signals`, with deduped action lists. Test `signals::tests::log_and_record_round_trip` covers the full event lifecycle including idempotent clicks.
- IPC: `log_retrieval` / `record_signal_action` / `list_retrieval_signals` — ready for the UI to wire when a `/search` page lands.
- `tools/distill/src/distill/embed_finetune.py`: reads the local `retrieval_signals` table, builds contrastive (query, positive, negative) triples (clicked / corrected = positives, weighted 2× for corrections; dismissed first then returned-but-unclicked as soft negatives), embeds via the same Vercel Gateway, trains a residual projection adapter (`alpha=0` initial → identity at start → gradual deviation), writes `adapter.safetensors` + `adapter.meta.json`.
- Runtime adapter loading in `embed/` is filed as Phase 3d-v2 in ROADMAP.

> **Scope honesty:** today signals are only created by direct IPC. A `/search` UI that drives them (and shows results so the user can click / dismiss / correct) lands in Phase 4 with the rest of the search-results pipeline.

---

## Phase 5 — Capture surfaces (2026-05-13 ✓ 3 full + 2 scaffold; 1 deferred)

**Why Phase 5 pivoted:** user identified that "open the app to type" is the
real friction. Original Phase 5 (CRDT / P2P / verifiable agent cards) was
re-ranked behind capture-surface work — and **keylogging was explicitly
rejected** as a trust-model regression, not a UX win. Phase 6 picks up the
deferred items.

### 5a · IMAP email gateway ✓
- Migration `0007_capture_sources.sql`: `capture_sources` (generic table for any polled source) + `capture_seen` (dedupe UIDs across cycles).
- `src/capture/email_imap.rs`: TLS via rustls + `async-imap` + `mail-parser`. Polls UNSEEN messages from a configured folder; reads RFC822 → text (HTML stripped); inserts as a `notes` row with source `email:<UID>`.
- IMAP password stored in the OS keychain (account `imap-<ulid>`); the DB row carries only the keychain account name, never the password.
- `src/capture/processor.rs`: shared post-capture pipeline that drives **any** newly-inserted note through extract → save_extracted → embed → audit. Reused by the IMAP poller, the ICS poller, and `commands::capture_note`.
- IPC: `add_imap_source`, `list_capture_sources`, `set_capture_source_enabled`, `delete_capture_source`.
- UI: new `/settings` page with an "Add IMAP mailbox" form + list of current sources (status / last-poll / error / enable toggle / delete).
- Tests: `capture::email_imap::tests::{html_to_plain_strips_tags, truncate_inserts_marker}`.

### 5b · Clipboard-prefilled hotkey ⌘⇧K ✓
- Pragmatic shipping form of "selected-text capture": instead of asking for macOS Accessibility permission (the same scary prompt keyloggers need), bind a second global shortcut that reads the **clipboard** and opens the quick-capture window pre-filled. UX is identical for the 90% case where the user just hit ⌘C.
- `tauri-plugin-clipboard-manager` plugin; `global_shortcut.rs` distinguishes the two hotkeys by key code and opens the popup with `?prefill=<urlencoded>`.
- `/quick-capture` page reads `useSearchParams` and seeds the textarea.
- Configurable via `CAIRN_HOTKEY` (empty) + `CAIRN_CLIP_HOTKEY` (clipboard).
- **True Accessibility-API selection capture is filed as Phase 6** — same OS permission keyloggers need + Swift/Obj-C bridge for `AXUIElementCopyAttributeValue`.

### 5c · Calendar ICS feed polling ✓
- `src/capture/ics.rs`: HTTPS GET the user-supplied ICS URL, parse with `icalendar` crate, iterate `VEVENT`s, dedupe by UID, insert one note per event with source `calendar:<UID>` (title + when + where + description).
- Same `capture_sources` table; `kind="ics"`, config = `{"url": "..."}`.
- IPC: `add_ics_source`. UI form on `/settings` accepts the "secret address in iCal format" (Google) or shared CalDAV URL.
- Default poll interval 30 min.
- No auth (relies on URL secrecy). OAuth providers → Phase 6.

### 5d · macOS Share Sheet — _scaffold only_
- `tools/share-extension/README.md` with full architectural plan + Swift `NSExtension` skeleton + Cairn-side App-Group drain handler.
- Blocked on Xcode post-build packaging step (Tauri doesn't produce `.appex` natively) + developer-team App Group identifier. Pure packaging work → Phase 6.

### 5e · Browser extension — _scaffold only_
- `tools/browser-ext/README.md` with Manifest V3 skeleton + localhost HTTP handshake design + per-domain allowlist UX + Cairn-side endpoint spec.
- Cairn-side `127.0.0.1:7717/capture` listener is a clean ~150-line task → Phase 6.

### 5f · Voice push-to-talk — _deferred to Phase 6_
- `cpal` audio capture + Gateway Whisper is well-trodden but ~200 lines of audio code + recording UX that doesn't compose with the current keyboard-only flow. Pulled into Phase 6 alongside a dedicated audio module.

---

## Phase 6a — Selection popover (PopClip/Doubao-style) (2026-05-13 ✓)

**Why now:** the clipboard-prefilled hotkey (Phase 5b) needs 4 steps —
select → ⌘C → ⌘⇧K → ⌘↵. PopClip / ChatGPT-macOS / Doubao all use a 2-step
flow: select → click a floating button. Same Accessibility permission as
keylogger paths, but **on demand, read-only, per-action** — Cairn never
reads anything the user hasn't already highlighted.

### Module layout
- `src-tauri/src/selection_popover/`
  - `permission.rs` — `AXIsProcessTrustedWithOptions` wrapper + a "open
    System Settings → Privacy & Security → Accessibility" helper. We never
    silently re-prompt at launch.
  - `geometry.rs` — pack `CFRange` → `AXValueRef` for
    `kAXBoundsForRangeParameterizedAttribute`; unpack the returned `CGRect`;
    `anchor_below_or_above` computes popover position (centred horizontally
    on the selection, below by default, flips above if it would overflow the
    bottom edge of the monitor).
  - `detector.rs` — poll loop (220 ms idle / 380 ms while a stable selection
    is showing). Each tick: skip if left mouse button is held
    (`CGEventSourceButtonState`), else walk system-wide AX → focused element
    → `kAXSelectedTextAttribute` + `kAXSelectedTextRangeAttribute` →
    bounds-for-range. Front-most app PID / bundle / localized name via
    `NSWorkspace.frontmostApplication` (objc2-app-kit). The detector calls a
    closure with `Option<SelectionEvent>`; supervisor decides what to do.
  - `panel.rs` — Tauri `WebviewWindow` builder: borderless, non-resizable,
    always-on-top, shadowed, **`focused = false` + `accept_first_mouse =
    true`** so the popover receives clicks without stealing focus from the
    user's source app. Single instance keyed by label `selection-popover` —
    reused across shows (set_position + show, emit `selection_show` event).
  - `mod.rs` — `SelectionPopover` supervisor: enable/disable, persistence
    (`selection_popover.json` in data dir — sidesteps a migration for one
    bool), cached last selection (renderer fetches via IPC on mount). Boots
    on app start if previously enabled AND AX trust is already granted.
- `src/app/selection-popover/page.tsx` — three-button pill: **Save**
  (captureNote with `source: selection:<bundle>`), **+ note** (opens
  quick-capture prefilled, popover dismissed), **×** (dismiss). Listens
  for `selection_show` events to update its state.
- `src/app/settings/page.tsx` — new "Selection popover" section: enable
  toggle + AX permission indicator + "Open System Settings" button.
- IPC: `selection_status`, `selection_set_enabled`,
  `selection_request_permission`, `selection_get_current`,
  `selection_dismiss`, `selection_save_with_note`.
- Capability `selection-popover` added to `capabilities/default.json`.

### Tests
- `geometry::tests` — anchor placement (below/above/clamp), CFRange→AXValue
  round-trip.
- `permission::tests` — `is_trusted(false)` is a non-panicking FFI call.

### Permission model
- AX trust is checked, never assumed. Prompt only fires when the user
  explicitly clicks the enable toggle or "Open System Settings".
- The supervisor stores `enabled = true` even if AX is missing — the
  detector boots automatically the moment trust appears, so the user only
  ever has to grant once.
- Cairn never observes input; only on-demand AX queries for the *currently
  selected* range. No keylogger path, no clipboard observer, no app
  monitor.

### Known limitations
- Apps that don't expose AX text selection (most Electron apps, some web
  views in Safari → 100% works in Chrome/Edge/Firefox, mixed in Slack /
  Discord) won't show the popover. Documented in the settings copy.
- The popover currently briefly steals key-focus on click (the `focused
  = false` + `accept_first_mouse = true` combo gets us most of the way, but
  true non-activating behaviour requires converting the NSWindow to an
  NSPanel with `NSWindowStyleMaskNonactivatingPanel`). Filed as Phase 6a
  follow-up.

---

## Verification snapshot (2026-05-13 post-Phase-5)

- **Rust:** `cargo check` ✓ · `cargo check --features encrypted` ✓ · `cargo test --lib` **38/38** ✓ (+5 since Phase 5: 4 geometry + 1 permission, all under `selection_popover::*`).
- **Release binary:** `cargo build --release --bin cairn-mcp` ✓
- **TypeScript / UI:** `pnpm exec tsc --noEmit` ✓ · `pnpm build` ✓ — **11 static routes** (added `/selection-popover`).

## Phase 5 open follow-ups (Phase 6 inbox)

1. **5d** → macOS Share Sheet extension via Xcode post-build + App Group code-signing.
2. **5e** → Browser extension package + Cairn local HTTP listener.
3. **5f** → Voice push-to-talk via `cpal` + Gateway Whisper.
4. **True Accessibility-API selection capture** ("highlight + ⌘⇧K reads the selection without round-tripping the clipboard").
5. **Calendar OAuth** (Google + Outlook) for users without an ICS secret link.
6. **/settings → "Poll now" manual trigger** so the user doesn't wait for the next tick after adding a source.
7. **Carry-overs**: Verifiable Agent Cards, BBS+ selective disclosure, real-time CRDT (yrs + Iroh), runtime adapter eval harness.

> Update (2026-06-12): item 2 is done — shipped as a split surface: the
> always-on REST API on `127.0.0.1:7716` serves the extension, while
> MCP-over-SSE lives separately on `7717`.

---

## Phase 4 — Make Cairn usable day-to-day (2026-05-13 ✓ 5 of 6 shipped)

### 4a · `/search` UI + signal collection wiring ✓
- New page `src/app/search/page.tsx` that calls `search()` IPC, renders hybrid-retrieval results, and surfaces the per-result score breakdown (BM25 / vector / RRF / importance / recency / final).
- `Diagnostics` row shows `bm25_hits / vector_hits / fused_hits` and any `vector_skipped` reason.
- Clicks on a result fire `record_signal_action("click")`; the × button fires `dismiss`. Both are wired through IPC into the Phase 3d `retrieval_signals` table — the signal-collection loop is now closed end-to-end.

### 4b · Global hotkey capture overlay ✓
- `tauri-plugin-global-shortcut` registered at startup; default ⌘⇧M on macOS, Ctrl+Shift+M elsewhere. Override with `CAIRN_HOTKEY` env (Tauri shortcut syntax).
- On press: creates an always-on-top frameless 520×200 window at `tauri://localhost/quick-capture` if not already open; otherwise focuses it.
- The page is a single textarea + `⌘+Enter` to save + Esc to close. Auto-closes ~220 ms after save so the user gets a saved flash without an abrupt vanish.

### 4c · Cost / latency telemetry ✓
- New table `usage_log` (`migrations/0006_usage_log.sql`) with `operation`, `provider`, `model`, `prompt_tokens`, `completion_tokens`, `cost_micro_usd`, `latency_ms`, `success`, `error`, `occurred_at`. Cost is stored in integer micro-USD so daily sums are exact.
- `src/usage.rs` exposes a `OnceCell<Db>` sink + `record(event)`; the recording write happens in a detached `tokio::spawn` so the user-visible call path is never slowed by telemetry.
- Hooked into:
  - `extract/openai_compat::call_once_op(...)` — pulls `prompt_tokens`, `completion_tokens`, `cost` from the gateway response.
  - `consolidation/summarize.rs` — tags `operation="consolidate"`.
  - `embed/gateway.rs` — tags `operation="embed"`.
- New IPC: `list_usage`, `usage_summary` (per-day × operation × provider rollup over N days).
- `/usage` UI page: stat cards (total cost, calls, prompt + completion tokens) + daily breakdown table + recent-calls log.

### 4d · Runtime personal-embedding adapter loading ✓
- `embed/adapter.rs`: loads `adapter.safetensors` from the Cairn data dir if present. Reads three tensors:
  - `linear.weight` — `dim×dim` row-major
  - `linear.bias` — `dim`
  - `alpha` — scalar
- Applies `out = normalize(base + alpha · tanh(W · base + b))` — the same math the Python trainer at `tools/distill/embed_finetune.py` was built against.
- `AdaptedEmbeddingProvider` wraps any base `EmbeddingProvider` and degrades cleanly to pass-through when no adapter is on disk.
- `AppState::init` now calls `embed::build_default_with_adapter_dir(Some(&data_dir))` so the adapter activates as soon as the trainer writes its first checkpoint.
- Tests: `embed::adapter::tests::{missing_file_returns_none, identity_adapter_normalises_input, dim_mismatch_rejected}`.

### 4e · Verifiable Agent Cards — _deferred to Phase 5_
A slim implementation (table column + registry JSON + signature verification on `initialize`) is straightforward Rust, but the *operationalisation* — curating the registry, distributing pubkey updates, deciding how to surface unverified-vs-verified in the agent UX without false alarms — is its own design exercise that deserves its own phase. Filed in the open follow-ups and pulled into Phase 5.

### 4f · Plaintext → encrypted DB migration CLI ✓
- New binary `cairn-migrate` (gated by `--features encrypted` so it only exists in builds that ship SQLCipher).
- Steps:
  1. Open destination as a fresh SQLCipher DB with the key from the OS keychain (account `db-key`, service `so.cairn.app`).
  2. Run all `sqlx::migrate!` migrations so the schema is byte-identical to a freshly-installed Cairn.
  3. `ATTACH DATABASE '<plaintext>' AS plaintext KEY ''` + `INSERT INTO main.<t> SELECT * FROM plaintext.<t>` for every concrete data table.
  4. Verify per-table row counts.
  5. Leaves `entity_vecs` empty (vec0 is virtual); the app rebuilds embeddings on next run.
- `docs/SETUP.md` updated with the end-to-end recipe including the file swap.

---

## Verification snapshot (2026-05-13 post-Phase-4)

- **Rust:** `cargo check` ✓ · `cargo check --features encrypted` ✓ · `cargo test --lib` **31/31** ✓ · `cargo build --release --bin cairn-mcp` ✓ · `cargo check --features encrypted --bin cairn-migrate` ✓
- **TypeScript:** `pnpm exec tsc --noEmit` ✓ · `pnpm build` ✓ — **9 static routes** (added `/search`, `/quick-capture`, `/usage` since Phase 3).
- **Hand-written code total:** ~11,300 lines (Phase 3 closed at ~9,800; +1,500 in Phase 4).

---

## Phase 4 open follow-ups (carried into Phase 5 — none silently dropped)

1. **Real-time CRDT (yrs) + peer-to-peer transport (Iroh / libp2p)** — replace hand-carry JSON bundles with continuous sync.
2. **Verifiable Agent Cards** — registry distribution + UI affordances. Slim signature verification on `initialize` is straightforward Rust; the curation / refresh / UX is the part that wants a design pass.
3. **BBS+ selective-disclosure proofs** for the audit log — prove "Agent X only read entity-type Y" without revealing entity ids.
4. **Browser extension + mobile (Tauri iOS/Android)** — deliberately deferred per user direction (2026-05-13). Worth their own phases.
5. **Adapter eval harness** — once enough `retrieval_signals` rows exist, an offline harness comparing per-query results pre- vs post-adapter to gate the rollout.
6. **macOS keychain re-prompt mitigation** — first-run keychain prompt currently takes ~18 s (user-attention bound). Investigate `kSecAttrAccessibleAfterFirstUnlock` semantics so subsequent launches don't re-prompt.

---

## Verification snapshot (2026-05-13 post-Phase-3)

- **Rust:** `cargo check` ✓ · `cargo check --features encrypted` ✓ · `cargo test --lib` **28/28** ✓ (legacy 17 + Phase 2 (4) + Phase 3 (7))
- **Release binary:** `cargo build --release --bin cairn-mcp` ✓
- **TypeScript:** `pnpm exec tsc --noEmit` ✓ · `pnpm build` ✓ (6 static routes)
- **MCP smoke** with `cairn-mcp` + AI Gateway: `initialize` returns `{name: 'cairn', version: '0.1.0'}`; `tools/list` returns 5 tools; startup logs show audit key now created in **keychain**, embedding ready against the gateway, encrypted-flag honest.
- **Python distill toolkit:** `pytest` 12/12 ✓ (the new `embed_finetune.py` is end-to-end runnable but its no-real-signals path errors cleanly with the documented `--min-pairs` floor).

---

## Open follow-ups (filed, not punted)

1. Phase 3c real-time CRDT (yrs) + peer-to-peer transport (Iroh).
2. Phase 3d-v2: load the trained `adapter.safetensors` at runtime in `embed/`. Compose: `normalize(base_emb + alpha * tanh(W @ base_emb + b))`.
3. `/search` UI page so signal collection runs from real user behaviour, not just IPC.
4. Plaintext → encrypted DB migration tool (`tools/migrate/` CLI).
5. Selective-disclosure proofs (BBS+) for the audit log so a user can prove "Agent X only read entity-type Y" without revealing entity ids. Originally on the Phase 4 list; bumped because users with multiple devices will want this once 3c sync ships.

---

## Rebrand to **Cairn** (2026-05-13 ✓)

Working title `cairn-memory` retired. Product is now **Cairn** —
"mark where you've been; read by every agent."

Touched in a single commit pass (cleanly, with verification):

- Cargo: package `cairn`, lib `cairn_lib`, bins `cairn` + `cairn-mcp`.
- Tauri: `productName: "Cairn"`, `identifier: so.cairn.app`, window title.
- MCP `serverInfo.name`: `cairn`. `instructions` rewritten with brand voice.
- ProjectDirs → `~/Library/Application Support/Cairn/` (Linux `~/.local/share/Cairn`, Windows `%APPDATA%\Cairn`). Existing DB migrated by copy.
- Env vars: all `NEXTAGENT_*` → `CAIRN_*` across Rust + `.env`. `AI_GATEWAY_API_KEY` unchanged (Vercel-owned).
- UI: title + header + metadata switched. `/themes` etc. unchanged.
- Icons: regenerated as a stacked-stones glyph (Cairn semantic) at 32/64/128/256/512/1024 + `.icns` (via `iconutil`) + multi-size `.ico`.
- Docs: `README.md`, `docs/{ARCHITECTURE,SETUP,MCP_INTEGRATION,ROADMAP,PROGRESS}.md`, `tools/distill/README.md` — sed-renamed, manually proof-skimmed.
- Claude Desktop config: `mcpServers.cairn-memory` → `mcpServers.cairn`, binary path → `cairn-mcp`, env keys renamed. Backup retained.
- Old `cairn-mcp` binary deleted.

**Verification after rename**
- `cargo check` ✓
- `cargo test --lib` **21/21** ✓
- `cargo build --release --bin cairn-mcp` ✓
- Dev launch under new name ✓ — log shows `cairn v0.1.0`, sqlite-vec, gateway extraction + embedding ready, consolidation worker spawned, `cairn_lib::*` log targets.
- MCP smoke with exact Claude Desktop env block ✓ — `initialize` returns `{name: 'cairn', version: '0.1.0'}`; `tools/list` returns all 5 tools.

Followed-up: no leftover string `nextagent-memory|nextagent_memory_lib|Nextagent Memory|NEXTAGENT_|ai.nextagent.memory` anywhere in source / config / docs.

---

## Verification snapshot (2026-05-13 post-Phase-2)

- **Rust:** `cargo check` ✓ · `cargo test --lib` **21/21** ✓ · `cargo build --release --bin cairn-mcp` ✓
- **TypeScript:** `pnpm exec tsc --noEmit` ✓ · `pnpm build` ✓ (6 static routes incl. `/themes`)
- **Python (distill toolkit):** `pytest` **12/12** ✓
- **MCP smoke (real binary, real DB, real protocol):** `initialize` → `tools/list` (5 tools incl. `get_themes`) → `tools/call` for `get_themes` (correctly denied for un-permissioned agent) and `search_memory` (returns the new diagnostics block with `bm25_hits`/`vector_hits`/`fused_hits`) ✓

---

## Open follow-ups (filed, not punted)

1. **Consolidation eval harness.** Topic-discovery tests cover the deterministic SQL part; the LLM-summary leg is currently unit-tested only against schema validity. We need a recorded-response harness so summary quality regressions are caught automatically.
2. **Vector path A/B telemetry.** Today the retriever returns `vector_skipped` reasons but we don't aggregate them. A small `retrieval_runs` table would let us see how often embedding fails over time.
3. **Audit key → OS keychain.** Phase 2 keeps the Ed25519 private key in `audit_key` SQL table. Move to macOS Keychain / Win DPAPI / libsecret in Phase 3 (filed under Phase 3 in `ROADMAP.md`).
4. **Cost / latency counters per provider.** Currently we log; we don't aggregate. Plug into the consolidation_runs schema once Phase 3 starts.
5. **Distillation: dataset-card.** Add `tools/distill/DATASET.md` documenting seed distributions, teacher model versions, reject-reason histogram. Required before any trained model is shared.
