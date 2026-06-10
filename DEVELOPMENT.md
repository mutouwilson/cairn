# Development

How to set up Cairn locally, run it, and contribute changes.

For the system-level overview see [ARCHITECTURE.md](./ARCHITECTURE.md).

## Prerequisites

| Tool | Version | Why |
|---|---|---|
| **Node.js** | ≥ 20 LTS | Next.js 15, Tauri tooling |
| **pnpm** | ≥ 9 | Workspace + lockfile (pin from `package.json:packageManager`) |
| **Rust** | ≥ 1.77 (stable) | Tauri 2 + Rust core |
| **Xcode CLT** (macOS) | — | Tauri build, codesign |
| **WebKit2GTK 4.1 / libsoup-3 / librsvg2** (Linux) | — | Tauri webview |
| **WebView2** (Windows) | runtime | Tauri webview |

Install once:

```bash
# macOS
xcode-select --install
brew install pnpm rustup-init
rustup-init -y --default-toolchain stable
```

```bash
# Ubuntu 22.04 / 24.04
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  librsvg2-dev libssl-dev pkg-config build-essential curl
curl -fsSL https://get.pnpm.io/install.sh | sh -
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

## First-time setup

```bash
git clone https://github.com/mutouwilson/cairn.git
cd cairn/memory
pnpm install
cp .env.example .env       # fill in AI_GATEWAY_API_KEY or ANTHROPIC_API_KEY
pnpm tauri:dev             # launches Tauri shell with hot-reload
```

The first `cargo build` takes a few minutes (Tauri pulls a lot). Subsequent builds are incremental and fast.

## Repo layout

```
cairn/
├── README.md                    Project entry point
├── ARCHITECTURE.md              System design
├── DEVELOPMENT.md               This file
├── .gitignore
├── memory/                      Main application
│   ├── package.json             Node deps (Next.js 15, React 19, Zustand, Tailwind)
│   ├── next.config.mjs          Static export config
│   ├── tailwind.config.ts
│   ├── src/                     Next.js UI (TypeScript)
│   │   ├── app/(main)/          Main UI: /, /entities, /agents, /audit, /import, /search, /settings
│   │   ├── app/(popup)/         Popover surfaces: quick-capture, selection-popover
│   │   ├── components/          Capture, Timeline, EntityChip
│   │   └── lib/                 tauri.ts (typed invoke), types.ts
│   ├── src-tauri/               Rust core
│   │   ├── Cargo.toml           Crate manifest — three binaries: cairn, cairn-mcp, cairn-migrate
│   │   ├── src/                 See ARCHITECTURE.md for module roles
│   │   ├── migrations/          sqlx migrations
│   │   ├── icons/               App icons
│   │   └── tauri.conf.json
│   ├── docs/                    Per-app docs (SETUP, MCP_INTEGRATION, etc.)
│   └── .env.example
├── tools/
│   ├── distill/                 Python — on-device extraction model distillation (Phase 2)
│   ├── share-extension/         macOS share-sheet integration
│   └── browser-ext/             Browser extension (planned)
└── untitled.pen                 Pencil UI design source
```

## Common commands

All run from `memory/` unless noted.

### Frontend (Next.js + TypeScript)

```bash
pnpm dev                          # Next.js dev server (no Tauri shell)
pnpm typecheck                    # tsc --noEmit
pnpm lint                         # next lint + eslint
pnpm build                        # static export → memory/out/
```

### Rust core (Tauri + binaries)

Run from `memory/src-tauri/`:

```bash
cargo fmt --check                 # formatting
cargo clippy --all-targets -- -D warnings
cargo check                       # type-level only, fast
cargo test                        # unit + integration tests
cargo build --release             # release binary
cargo build --release --bin cairn-mcp   # MCP server only
```

### Full app

```bash
# from memory/
pnpm tauri:dev                    # dev mode, hot-reload
pnpm tauri:build                  # production .app / .dmg / .msi / .AppImage
```

### Encrypted SQLite (optional)

```bash
# from memory/src-tauri/
cargo build --features encrypted --bin cairn          # vendors SQLCipher — adds ~10 min first build
cargo run    --features encrypted --bin cairn-migrate # one-shot plaintext → encrypted DB migration
```

`cairn-mcp` and the GUI must be built with matching feature flags.

## MCP integration (for end-users + testing)

Add to your MCP host config (e.g. `~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "cairn": {
      "command": "/Applications/Cairn.app/Contents/MacOS/cairn-mcp",
      "args": []
    }
  }
}
```

For dev mode, point at your `cargo run --bin cairn-mcp` output binary instead.

After restarting your MCP host:

1. Grant `cairn` permission on at least one entity type in the Cairn UI (`/agents`).
2. Ask the host "What does the user prefer for coffee?" — it should call `search_memory` and answer.
3. Open `/audit` and run **Verify chain** to see the access logged, hashed, and signed.

## Commit conventions

Conventional commits (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`, `perf:`).

- Keep commits small and atomic.
- Reference an issue (`Closes #123`) when relevant.
- Don't commit `.env`, `node_modules/`, `target/`, `.next/`, `out/` — `.gitignore` should already catch them.
- Pencil design changes (`untitled.pen`) are binary but small enough to track directly; large design assets will move to git-lfs when needed.

## CI

GitHub Actions in `.github/workflows/ci.yml` runs on every push and PR:

- `frontend` job — `pnpm install` → `pnpm typecheck` → `pnpm lint`
- `rust` job — installs Tauri system deps → `cargo fmt --check` → `cargo check` → `cargo clippy` → `cargo test`

Tauri full builds are intentionally **not** in CI (too slow for every PR). Release builds happen on tagged commits via a separate workflow (TODO).

## Adding a new module

When you add a new module under `src-tauri/src/`:

1. Mirror domain types in `src/lib/types.ts` (UI side).
2. If the module is callable from UI, expose an IPC command in `commands.rs`.
3. If the module touches memory entries, write the corresponding audit chain entry (`audit::record(...)`) — **no exceptions**.
4. Add unit tests in the module file (Rust convention) or `#[cfg(test)] mod tests`.
5. Update [ARCHITECTURE.md](./ARCHITECTURE.md) module table.

## Database migrations

```bash
# from memory/src-tauri/
sqlx migrate add -r <slug>        # creates migrations/NNNN_<slug>.up.sql + .down.sql
```

Migrations run automatically on app start.

> [!CAUTION]
> **Migrations are immutable once a release ships.** `sqlx` stores a hash of
> every applied migration file in the user's DB; if a single byte changes —
> including a comment, whitespace, or a renamed identifier — the next
> startup detects the checksum mismatch and refuses to open the DB.
>
> Rules:
>
> - **Never edit** a `migrations/*.sql` file after the first tagged release
>   that included it. This includes touch-ups to SQL comments.
> - **Don't run global string replacements** across the repo without
>   excluding `memory/src-tauri/migrations/`. The product rename
>   (`Nextagent → Cairn`) hit this trap and forced every v0.1.0-alpha.1
>   user to wipe their DB.
> - If you must change the schema, **write a new migration** —
>   `sqlx migrate add -r <slug>` — that idempotently brings old DBs to
>   the new state.

### Automated guardrails

These four mechanisms — three of them automated — defend "lossless upgrades"
together. Don't rely on any one alone; each catches a different class of
mistake.

1. **CI: migrations-immutable job** (`.github/workflows/ci.yml`). For every
   file present under `migrations/` at the highest-versioned `v*` tag, the
   job asserts HEAD still has the same blob hash at the same path. Modified,
   deleted, renamed, *and* "delete-then-re-add under a new slug" all fail
   the gate. Added migrations (new versions) pass freely. → A bad PR is
   blocked before it can merge.

2. **CI: upgrade smoke test** (`memory/src-tauri/tests/upgrade.rs`, gated on
   `CAIRN_UPGRADE_OLD_MIGRATIONS_DIR`). CI extracts the prev-release tag's
   migrations dir via `git archive`, applies them on a temp DB, seeds a
   sentinel note + entity, then opens the same file with HEAD's
   `Db::open` (running every migration added since). Asserts the sentinel
   survives byte-identical and `_sqlx_migrations` at HEAD is a
   prefix-preserving superset of OLD's. → A data-destroying *new*
   migration is caught before release, even if the file itself is "new"
   and so passes the immutability gate.

3. **Runtime: pre-migration backup** (`Db::open` in `memory/src-tauri/src/db/mod.rs`).
   When the DB file already existed *and* there are pending migrations,
   `Db::open` checkpoints the WAL and copies `memory.db` to
   `<data_dir>/backups/memory-<YYYYMMDD-HHMMSS-mmm>.db` (sidecars copied
   too when the checkpoint can't fully truncate). Keeps the newest 10
   backups, prunes the rest with their sidecars. → If a migration ever
   does get through and destroys data on a user's machine, the prior
   state is one `cp` away.

4. **Runtime: graceful conflict handling** (same file). On a sqlx
   `VersionMismatch`, `Db::open` returns a typed `MigrationConflict`
   error instead of panicking; `lib.rs::run()` catches it and shows a
   native dialog (via `rfd`) with the backup path and recovery
   instructions. → The user sees a clear message, not a Finder-launched
   silent crash.

> Notes:
> - The runtime guard is **best-effort, not bulletproof**: sqlx 0.8 does
>   not validate every applied-migration checksum *before* applying any
>   pending ones — a low-version pending migration commits before a
>   high-version mismatch trips. The pre-migration backup is taken
>   before any apply, so recovery is intact, but the live `memory.db`
>   may be in a half-migrated state until the user reinstalls. The dialog
>   wording acknowledges this; don't soften it.
> - **Cross-process migration races** are out of scope here. Cairn isn't a
>   single-instance app; two launches racing each other through `Db::open`
>   can both apply migrations concurrently. SQLite file locking + sqlx's
>   per-migration transactions usually serialise the writes, but the
>   safer fix is a single-instance lock plugin — tracked separately.

## Common pitfalls

- **`cargo build` hangs on first run** — Tauri bundles a lot. Expect 5-10 minutes the first time; subsequent builds use the cache.
- **macOS Accessibility permission** — selection popover and global hotkey need it. System Settings → Privacy & Security → Accessibility → enable Cairn.
- **MCP host doesn't see `cairn`** — restart the host process after editing its config; the config is only read at startup.
- **`.env` not loaded** — make sure it's at `memory/.env`, not the repo root, and that you ran `pnpm tauri:dev` rather than `cargo run` directly.

## Common pitfalls

- **`cargo build` hangs on first run** — Tauri bundles a lot. Expect 5-10 minutes the first time; subsequent builds use the cache.
- **macOS Accessibility permission** — selection popover and global hotkey need it. System Settings → Privacy & Security → Accessibility → enable Cairn.
- **MCP host doesn't see `cairn`** — restart the host process after editing its config; the config is only read at startup.
- **`.env` not loaded** — make sure it's at `memory/.env`, not the repo root, and that you ran `pnpm tauri:dev` rather than `cargo run` directly.
