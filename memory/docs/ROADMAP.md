# Roadmap

Months-long path from working prototype → production-grade personal Memory /
Context OS. Listed in priority order, not strict chronological order — items
within a phase can run in parallel.

## Quality bar (applies to every item)

- No commit lands without a test or an explicit "untested" note in the PR.
- No `unwrap()`/`panic!()` on user-input paths.
- No `TODO:` left dangling more than 14 days; either fixed or filed as an issue.
- No silent failures; everything logs with `tracing` at appropriate level.
- All new external deps explained in their PR (why, alternatives considered).

---

## Phase 1 — Foundation ✓ shipped

See `PROGRESS.md`.

## Phase 2 — Real technical depth ✓ shipped (2026-05-13)

### 2a. Provider routing via Vercel AI Gateway ✓
### 2b. On-device extraction model — toolkit ✓ (trained weights TBD when a user runs it end-to-end)
### 2c. Hierarchical memory + sleep consolidation ✓
### 2d. Vector retrieval (sqlite-vec) ✓

See `PROGRESS.md` for full module list. Carry-over items into Phase 3:

- Tune RRF / weights via offline A/B on real captures.
- Build a recorded-response eval harness for the consolidation summary leg.
- Ship a first distilled checkpoint and bake F1 floors into CI before any release.

## Phase 3 — Multi-device + privacy hardening ✓ shipped (2026-05-13)

### 3a. Audit signing key → OS Keychain ✓
### 3b. Encrypted SQLite via SQLCipher ✓ (opt-in `--features encrypted`)
### 3c. Signed JSON sync bundles (hand-carry sync) ✓
### 3d. Personal-embedding signals + offline LoRA trainer ✓

## Phase 4 — Make Cairn usable day-to-day ✓ shipped (2026-05-13)

### 4a. `/search` UI + signal collection wiring ✓
### 4b. Global hotkey capture overlay (⌘⇧M) ✓
### 4c. Cost / latency telemetry + `/usage` page ✓
### 4d. Runtime personal-embedding adapter loading ✓
### 4e. Verifiable Agent Cards — _deferred to Phase 5_
### 4f. Plaintext → encrypted DB migration CLI (`cairn-migrate`) ✓

## Phase 5 — Capture surfaces ✓ (2026-05-13; 3 full + 2 scaffold; pivoted from original scope)

Original Phase 5 (CRDT / P2P / agent identity) was re-ranked behind
capture-surface work per user feedback 2026-05-13. Keylogging explicitly
rejected as a trust-model regression.

### 5a. IMAP email gateway ✓
### 5b. Clipboard-prefilled hotkey ⌘⇧K ✓
### 5c. Calendar ICS feed polling ✓
### 5d. macOS Share Sheet — _scaffold_
### 5e. Browser extension — _scaffold_
### 5f. Voice push-to-talk — _deferred_

## Phase 6 — Polish capture + carry-over privacy hardening (in progress)

### 6a. Selection popover (PopClip / Doubao-style) ✓ shipped (2026-05-13)
- macOS Accessibility-API selection capture as a floating action bar.
- Replaces the 4-step clipboard hotkey flow with a 2-step
  highlight → click. Same OS permission as keyloggers, but on-demand and
  read-only — Cairn never observes anything the user hasn't already
  highlighted.
- See `docs/PROGRESS.md#phase-6a` for module layout, tests, and
  permission model.

### Still on the Phase 6 list
- 5d / 5e / 5f from Phase 5 promoted to full implementation.
- Calendar OAuth (Google + Outlook) for non-ICS users.
- Verifiable Agent Cards (Phase 4e carry-over).
- BBS+ selective-disclosure proofs for audit log.
- Real-time CRDT (yrs) + peer-to-peer transport (Iroh / libp2p).
- Adapter eval harness once retrieval signals accumulate.
- True non-activating popover (NSWindow → NSPanel conversion so the
  popover never blips focus on click). Filed by Phase 6a.

## Phase 5 — Personal embedding model

- Collect implicit feedback signal: which results the user clicked into / corrected / shared with agents.
- Contrastive LoRA head over the base embedding model. Online update every ~500 signals using EWC to avoid catastrophic forgetting.
- A/B-style harness: log retrieval queries with provenance so we can replay a query against old vs new ranker.

## Phase 6 — Capture surfaces beyond the app

- Global hotkey overlay (Tauri global shortcut + minimal floating capture window).
- Email gateway: `me@<user>.inbox.cairn.ai` forwards mail → API → daemon (with retention + DKIM checks).
- Browser extension: read-only "save this page + my note" → daemon.
- iOS Shortcuts / Android intent: native share-sheet target.
- Wear OS / watchOS dictation forwarding.

## Phase 7 — Family Context (shared context, separate data)

- A "shared schema" can be subscribed across users (e.g. household preferences, kid schedules).
- Each subscriber stores their own copy; updates flow via CRDT through a trusted relay.
- Per-field ACLs so only some fields are shared.

## Phase 8 — Distribution

- Signed `.dmg` / `.msi` / `.AppImage` via Tauri's official signers.
- macOS auto-update via Sparkle / Tauri updater plugin.
- Cloud companion (optional): user-controlled, single-tenant, end-to-end-encrypted backup.

---

## Cross-cutting tracks (always-on)

- **Observability**: structured logs, OpenTelemetry exporter, anonymous opt-in usage metrics.
- **Performance**: capture-to-extracted P99 < 4 s with gateway, < 1 s with on-device.
- **Security**: quarterly threat-model review, dependency audit (`cargo audit` + `pnpm audit` in CI).
- **Documentation**: every public Rust function has a doc comment; every page has an inline help affordance.
