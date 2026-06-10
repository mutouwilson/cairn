#!/usr/bin/env bash
# One-shot launcher: boots cairn-mcp in SSE mode behind a public Cloudflare
# Tunnel and prints the URL you paste into ChatGPT's "新应用 → MCP 服务器 URL"
# dialog. Ctrl-C tears both processes down cleanly.
#
# Prereqs:
#   - cairn-mcp built (release recommended):  cargo build --release --bin cairn-mcp
#   - cloudflared:                            brew install cloudflared
#
# Env overrides:
#   CAIRN_MCP_PORT       (default 7717)
#   CAIRN_MCP_BIN        (default: ./src-tauri/target/release/cairn-mcp falling back to debug)
#   CAIRN_DATA_DIR       (passed through to cairn-mcp)
#   CAIRN_LOG            (passed through; default `info`)

set -euo pipefail

PORT="${CAIRN_MCP_PORT:-7717}"

# --- locate binaries -----------------------------------------------------
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
mcp_bin="${CAIRN_MCP_BIN:-}"
if [[ -z "$mcp_bin" ]]; then
  for candidate in \
    "$repo_root/src-tauri/target/release/cairn-mcp" \
    "$repo_root/src-tauri/target/debug/cairn-mcp"; do
    if [[ -x "$candidate" ]]; then mcp_bin="$candidate"; break; fi
  done
fi
if [[ -z "$mcp_bin" || ! -x "$mcp_bin" ]]; then
  echo "✘ cairn-mcp not found. Build with:"
  echo "    cargo build --release --bin cairn-mcp"
  exit 1
fi
if ! command -v cloudflared >/dev/null 2>&1; then
  echo "✘ cloudflared not installed. Install with:"
  echo "    brew install cloudflared"
  exit 1
fi

mcp_log="$(mktemp -t cairn-mcp.XXXXXX.log)"
tunnel_log="$(mktemp -t cairn-tunnel.XXXXXX.log)"

cleanup() {
  trap - INT TERM EXIT
  if [[ -n "${tunnel_pid:-}" ]] && kill -0 "$tunnel_pid" 2>/dev/null; then
    kill "$tunnel_pid" 2>/dev/null || true
  fi
  if [[ -n "${mcp_pid:-}" ]] && kill -0 "$mcp_pid" 2>/dev/null; then
    kill "$mcp_pid" 2>/dev/null || true
  fi
  wait 2>/dev/null || true
  echo
  echo "stopped (logs left at $mcp_log, $tunnel_log)"
}
trap cleanup INT TERM EXIT

# --- start cairn-mcp -----------------------------------------------------
echo "▸ starting cairn-mcp on :$PORT  (log: $mcp_log)"
"$mcp_bin" --transport sse --port "$PORT" >"$mcp_log" 2>&1 &
mcp_pid=$!

# Wait for /health to answer.
for i in $(seq 1 40); do
  if curl -fs --max-time 1 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$mcp_pid" 2>/dev/null; then
    echo "✘ cairn-mcp exited before binding. Tail of log:" >&2
    tail -20 "$mcp_log" >&2
    exit 1
  fi
  sleep 0.25
  if [[ "$i" == 40 ]]; then
    echo "✘ cairn-mcp did not start in 10s. Tail of log:" >&2
    tail -20 "$mcp_log" >&2
    exit 1
  fi
done
echo "  ✓ /health OK"

# --- start cloudflared ---------------------------------------------------
echo "▸ starting cloudflared tunnel  (log: $tunnel_log)"
cloudflared tunnel --no-autoupdate --url "http://localhost:$PORT" \
  >"$tunnel_log" 2>&1 &
tunnel_pid=$!

# Parse the trycloudflare URL out of cloudflared's startup chatter.
public_url=""
for i in $(seq 1 60); do
  if grep -qE 'https://[a-z0-9-]+\.trycloudflare\.com' "$tunnel_log"; then
    public_url="$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$tunnel_log" | head -1)"
    break
  fi
  if ! kill -0 "$tunnel_pid" 2>/dev/null; then
    echo "✘ cloudflared exited before publishing a URL. Tail of log:" >&2
    tail -20 "$tunnel_log" >&2
    exit 1
  fi
  sleep 0.5
done
if [[ -z "$public_url" ]]; then
  echo "✘ cloudflared did not emit a trycloudflare URL within 30s." >&2
  tail -20 "$tunnel_log" >&2
  exit 1
fi

# Copy to clipboard if possible (macOS pbcopy / Wayland wl-copy / xclip).
sse_url="$public_url/sse"
copy_status=""
if command -v pbcopy >/dev/null 2>&1; then
  printf "%s" "$sse_url" | pbcopy && copy_status=" (copied to clipboard)"
elif command -v wl-copy >/dev/null 2>&1; then
  printf "%s" "$sse_url" | wl-copy && copy_status=" (copied to clipboard)"
elif command -v xclip >/dev/null 2>&1; then
  printf "%s" "$sse_url" | xclip -selection clipboard && copy_status=" (copied to clipboard)"
fi

# --- present ------------------------------------------------------------
cat <<EOF

  ╭─ Cairn MCP bridge is live ─────────────────────────────────────────────╮
  │                                                                        │
  │   SSE URL (paste into ChatGPT):                                        │
  │     $sse_url${copy_status}
  │                                                                        │
  │   Auth: 未授权 / None — the URL itself is the secret. Treat as a       │
  │   password. Anyone who learns it can read your full memory.            │
  │                                                                        │
  │   Local:  http://127.0.0.1:$PORT/sse                                     │
  │   Health: http://127.0.0.1:$PORT/health                                  │
  │                                                                        │
  │   Press Ctrl-C to stop.                                                │
  ╰────────────────────────────────────────────────────────────────────────╯

EOF

wait "$tunnel_pid" "$mcp_pid"
