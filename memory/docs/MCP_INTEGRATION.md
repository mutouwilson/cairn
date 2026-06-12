# MCP integration

How to wire **Cairn** into MCP-capable agents so they can read
your context (after you grant them permission).

> Two local surfaces, don't confuse them:
> **`127.0.0.1:7717`** is MCP over SSE (this doc), opt-in via Settings.
> **`127.0.0.1:7716`** is the always-on REST API (`/api/*`) used by the
> browser extension and non-MCP callers — it is not MCP.
>
> The Settings → **Hosted MCP Connectors** section shows ready-to-paste
> snippets for Claude Desktop / Cursor / ChatGPT, plus each agent's last-seen
> time.

## 1. Claude Desktop (macOS)

Edit `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "cairn": {
      "command": "/absolute/path/to/memory/src-tauri/target/release/cairn-mcp",
      "args": [],
      "env": {
        "CAIRN_AGENT_ID": "claude-desktop",
        "CAIRN_LOG": "info"
        // optional: "CAIRN_DATA_DIR": "/custom/dir",
        //           "CAIRN_ENCRYPT": "1"  (if your DB is encrypted)
      }
    }
  }
}
```

For development you can point at the debug build:

```
/absolute/path/to/memory/src-tauri/target/debug/cairn-mcp
```

After saving, **fully quit and relaunch Claude Desktop**. Open the
🔌 plug icon in the chat composer; you should see `cairn`
with 5 tools: `search_memory`, `get_preferences`, `list_recent_notes`,
`get_themes`, `record_observation`.

> Prefer SSE instead of spawning a subprocess? Enable **Settings → Remote
> MCP bridge** in Cairn and point Claude Desktop at
> `{"url": "http://127.0.0.1:7717/sse"}` — then Cairn.app stays the only
> SQLite writer.

## 2. Cursor

Settings → MCP → "Add MCP server":

- Name: `cairn`
- Command: `/absolute/path/to/memory/src-tauri/target/release/cairn-mcp`
- Env: `CAIRN_AGENT_ID=cursor`

Then in the Cairn UI (`/agents`), add a `cursor` agent and grant
it the entity-type permissions you want.

## 3. Remote / SSE clients (ChatGPT connectors, etc.)

Enable **Settings → Remote MCP bridge** → Cairn serves MCP over SSE at
`http://127.0.0.1:7717/sse` (no restart needed). Remote services like the
ChatGPT connector need a public HTTPS URL — front it with a tunnel
(`cloudflared tunnel --protocol http2 --url http://localhost:7717`) and use
`https://<name>.trycloudflare.com/sse`. A standalone read-only variant also
exists: `cairn-mcp --transport sse --port 7717`.

## 4. Any other MCP client

The binary speaks plain JSON-RPC 2.0 over stdio. Spawn it as a subprocess
and write `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n`
to stdin. The server responds, then accepts `tools/list` and `tools/call`.

## Permission model

New agents are **not** auto-registered: add the agent (by its
`CAIRN_AGENT_ID`) in `/agents` and grant permissions there. Until then,
every tool call returns `denied: true` (a normal result, not an error —
models handle it gracefully).

Recommended starting policy:

| Agent | Scope |
|---|---|
| `claude-desktop` | `Preference: read` (seeded) |
| `cursor` (coding) | `Preference: read`, `Skill: read`, `Goal: read` |
| anything else | `none` until you decide |

Never give `'*': write` to a third-party agent — that lets it append
arbitrary notes to your memory, which then influence every future
retrieval.

## What gets audited

Every `tools/list` and every `tools/call` (success or failure) is
appended to the hash-chained, Ed25519-signed `audit_log`. Visit
`/audit` to view entries or run "verify chain". The public key on
that page is yours — share it and a third party can independently
verify your audit history was not tampered with.

## Useful first prompt to test

```
You have a tool called search_memory. Use it to answer:
"What does the user prefer for coffee?"

After answering, also tell me which entity types you read and how
many entities the search returned.
```

If everything is wired correctly you should see Claude call
`search_memory({query: "coffee", types: ["Preference"]})`, get back
your preferences, and answer. Then in `/audit` you'll see a row with
agent_id `claude-desktop`, action `tools/call`, tool_name
`search_memory`, with the cryptographic chain anchored to the
previous entry.
