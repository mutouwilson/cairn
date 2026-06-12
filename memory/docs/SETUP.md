# Setup

> Just want to *use* Cairn? Grab a prebuilt installer (DMG / MSI / AppImage +
> the Chrome extension zip) from
> [Releases](https://github.com/mutouwilson/cairn/releases). Everything below
> is the **build-from-source / development** path.

## Prerequisites

- macOS 13+ (Linux / Windows also work; instructions below are macOS-focused)
- **Node.js 20+** and **pnpm 9+**
- **Rust 1.77+** (`rustup install stable`)
- **Tauri 2 system deps**: see the [official guide](https://v2.tauri.app/start/prerequisites/). On macOS that's just Xcode CLI tools.
- Optional: an API key for any supported AI provider — configured **in-app**
  after first launch (Settings → AI providers; 10 families: Vercel Gateway,
  OpenRouter, OpenAI, Anthropic, Gemini, 豆包, Minimax, Kimi, GLM, 通义).
  Without one Cairn runs in BM25-only logbook mode.

## First-time install

```bash
cd memory
pnpm install
```

AI providers are configured in the app (Settings → AI providers) — no env
vars required. `cp .env.example .env` + `AI_GATEWAY_API_KEY=…` still works as
a fallback for headless/dev runs, but the Settings page is the primary path.

## Run in dev

```bash
pnpm tauri:dev
```

This:
1. Starts Next.js on `http://localhost:3000`.
2. Builds the Rust app in debug mode.
3. Launches the Tauri webview pointing at the Next.js dev server.
4. Hot-reloads UI on save; rebuilds Rust on save.

The first build may take 5–10 minutes (sqlx + reqwest + tauri) — subsequent builds are seconds.

## Where data lives

By default, on macOS:

```
~/Library/Application Support/Cairn/memory.db
```

Override via env:

```bash
CAIRN_DATA_DIR=/path/to/dir pnpm tauri:dev
```

## Running the standalone MCP server

```bash
cargo run --bin cairn-mcp --release --manifest-path src-tauri/Cargo.toml
```

This is what Claude Desktop / Cursor launch as a subprocess. See [MCP_INTEGRATION.md](./MCP_INTEGRATION.md) for the config snippet. It also supports `--transport sse --port 7717` for a standalone read-only SSE server.

## Local port topology

| Port | What | Lifecycle |
|---|---|---|
| `127.0.0.1:7716` | REST API (`/api/status\|capture\|search\|recent\|themes`) — used by the browser extension and third-party callers | Always on while the GUI runs; auto-bumps up to 4 ports if taken; `CAIRN_API_PORT` overrides |
| `127.0.0.1:7717` | MCP SSE bridge (`/sse`) — for remote MCP clients (ChatGPT connectors via tunnel) | Opt-in toggle in Settings |

Set `CAIRN_API_TOKEN` before launch to require `Authorization: Bearer …` on `/api/*` (`/api/status` stays open); paste the same token into the browser extension's options.

## Tests

```bash
cd src-tauri
cargo test
```

Covers:
- Audit chain verifies after many appends.
- Tampering with a stored row is detected on verify.
- Schema validation rejects bad LLM outputs.
- Ebbinghaus retention / reinforcement bounds.
- RRF fusion + recency scoring.
- FTS query building.
- Consolidation supersede chain + CJK-safe truncation.

## Production build

```bash
pnpm tauri:build
```

Produces an ad-hoc-signed `.dmg` (macOS — not notarized; first launch may need `xattr -dr com.apple.quarantine`), `.msi` (Windows), or `.AppImage` (Linux) in `src-tauri/target/release/bundle/`.

## Encrypted at-rest storage (opt-in, Phase 3b)

Cairn supports SQLCipher-encrypted SQLite databases. The DB key is a 32-byte
random secret held in the OS keychain — Cairn never writes the DB key into the
DB itself. To enable:

```bash
# Build once with the cargo feature (vendors OpenSSL → first build is slow):
(cd src-tauri && cargo build --release --features encrypted --bin cairn)
(cd src-tauri && cargo build --release --features encrypted --bin cairn-mcp)

# Then turn encryption on at runtime:
export CAIRN_ENCRYPT=1
pnpm tauri:dev          # or run the binaries directly
```

What you get:
- All SQLite files (`memory.db` + `-wal` + `-shm`) are AES-256 encrypted at rest.
- The `entity_vecs` (sqlite-vec) and `notes_fts` (FTS5) tables work identically — both are stored inside the encrypted page-store.
- A `cairn-mcp` subprocess started by Claude Desktop must inherit `CAIRN_ENCRYPT=1` (set it in the Claude Desktop `env` block).

Caveats:
- Switching `CAIRN_ENCRYPT` on an *existing* plaintext DB will fail to open. Use the `cairn-migrate` helper below.
- If you forget the OS keychain entry, the DB is unrecoverable. Back it up like any other crypto-protected file.

### Migrating an existing plaintext DB → encrypted (Phase 4f)

```bash
# Build the migration binary (also needs --features encrypted)
(cd src-tauri && cargo build --release --features encrypted --bin cairn-migrate)

# The destination key comes from the OS keychain (account "db-key", service
# "so.cairn.app"). Open the app at least once with --features encrypted +
# CAIRN_ENCRYPT=1 so the key exists before running the migration.
./src-tauri/target/release/cairn-migrate \
    --from "$HOME/Library/Application Support/Cairn/memory.db" \
    --to   "$HOME/Library/Application Support/Cairn/encrypted.db"

# Swap the file. The original is kept as `.plaintext.bak` so you can verify.
DATA="$HOME/Library/Application Support/Cairn"
mv "$DATA/memory.db" "$DATA/memory.db.plaintext.bak"
mv "$DATA/encrypted.db" "$DATA/memory.db"
export CAIRN_ENCRYPT=1
```

`entity_vecs` (the sqlite-vec virtual table) is intentionally left empty by the
migration — open the app once with CAIRN_ENCRYPT=1 and it rebuilds them via
`reembed_pending`.

## Common issues

- **Extraction doesn't run / notes stay raw** — no extract provider configured. Open Settings → AI providers and set one (any of the 10 families); env vars are only a fallback.
- **Migration checksum conflict on launch** — Cairn shows a native dialog and has already kept a pre-migration backup under `…/Cairn/backups/`. Install a matching/newer version first; wiping `memory.db` is the last resort.
- **MCP server starts but Claude Desktop doesn't see tools** — quit + relaunch Claude Desktop. It only scans the config at startup.
- **Permission denied on SQLite** — another process has the DB open with an exclusive lock. WAL mode normally avoids this but some tools (DB Browser for SQLite) still grab exclusive.
