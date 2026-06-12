# Roadmap

Months-long path from working prototype → production-grade personal Memory /
Context OS. Listed in priority order, not strict chronological order — items
within a phase can run in parallel.

> **A note on phase numbering:** numbers in this file follow the engineering
> log (`PROGRESS.md` + code comments — e.g. the code's "Phase 7" is dev-tool
> memory sync/import-export). `README.md` uses its own coarser Phase 1–4
> product framing; the two are different scales. Sections marked "Future"
> below are deliberately unnumbered to avoid colliding with either sequence.

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
- Ship a first distilled checkpoint and bake F1 floors into CI before any stable release.

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
### 5e. Browser extension ✓ shipped (load-unpacked zip distributed with each Release)
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

### Shipped since (2026-06 snapshot)
- **6b · In-app MCP SSE bridge ✓** — Settings toggle boots an in-process SSE
  server on `127.0.0.1:7717`; standalone `cairn-mcp --transport sse` kept.
- **6d · Provider settings page ✓** — 10 provider families configurable
  in-app, hot-reloaded without restart.
- **Local REST API server ✓** — always-on `127.0.0.1:7716` with optional
  `CAIRN_API_TOKEN` bearer auth; powers the browser extension.
- **i18n ✓** — full `en` + `zh-CN` dictionaries.
- **Phase 7 · V6 dev-tool memory import/export ✓** — CLAUDE.md / Cursor /
  Codex import, managed-block export, file watcher, skip / unlinked handling.
- **Release + CI infrastructure ✓** — 3-platform release builds + extension
  zip (see Phase 8 below); migrations-immutable gate + upgrade smoke test.
- **Relicensed to FSL-1.1-ALv2 ✓.**

### Still on the Phase 6 list
- 5d / 5f from Phase 5 promoted to full implementation (5e shipped).
- Calendar OAuth (Google + Outlook) for non-ICS users.
- Verifiable Agent Cards (Phase 4e carry-over).
- BBS+ selective-disclosure proofs for audit log.
- Real-time CRDT (yrs) + peer-to-peer transport (Iroh / libp2p).
- Adapter eval harness once retrieval signals accumulate.
- True non-activating popover (NSWindow → NSPanel conversion so the
  popover never blips focus on click). Filed by Phase 6a.
- iOS Shortcuts / Android intent: native share-sheet target.
- Wear OS / watchOS dictation forwarding.

> Two earlier draft sections that used to live here ("Phase 5 — Personal
> embedding model", "Phase 6 — Capture surfaces beyond the app") have been
> removed as duplicates: the embedding-model work shipped as 3d + 4d, the
> capture-surface items shipped under Phases 4–5 or are tracked in the
> Phase 6 list above (iOS/Android share-sheet, watch dictation).

## Future — Family Context (shared context, separate data)

- A "shared schema" can be subscribed across users (e.g. household preferences, kid schedules).
- Each subscriber stores their own copy; updates flow via CRDT through a trusted relay.
- Per-field ACLs so only some fields are shared.

## Phase 8 — Distribution

Shipped:

- Tag-triggered release builds (`.github/workflows/release.yml`): `v*.*.*`
  tags produce macos-arm64 / linux-x64 / windows-x64 bundles plus the
  chrome-extension zip; tags containing `-` are auto-marked pre-release.
  Artifacts are **unsigned**.

Still open:

- Code signing + notarization (`.dmg` / `.msi` / `.AppImage` via Tauri's
  official signers).
- Auto-update (Sparkle / Tauri updater plugin — `tauri-plugin-updater` is not
  wired yet).
- Cloud companion (optional): user-controlled, single-tenant, end-to-end-encrypted backup.

---

## Cross-cutting tracks (always-on)

- **Observability**: structured logs, OpenTelemetry exporter, anonymous opt-in usage metrics.
- **Performance**: capture-to-extracted P99 < 4 s with gateway, < 1 s with on-device.
- **Security**: quarterly threat-model review, dependency audit (`cargo audit` + `pnpm audit` in CI — planned; CI has no audit job yet).
- **Documentation**: every public Rust function has a doc comment; every page has an inline help affordance.
