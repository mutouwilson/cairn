// Cairn HTTP client. Targets the local API exposed by Cairn.app on
// http://127.0.0.1:7717.
//
// In Manifest V3 a content script that fetches a cross-origin URL is, in
// principle, allowed by host_permissions — but the host site's CSP can
// still poison the request (chatgpt.com, claude.ai, and gemini.google.com
// all set strict `connect-src` directives that drop the call before it
// leaves the page). The reliable pattern is to route every request through
// the background service worker, which runs in the extension's own origin
// and isn't bound by the host page's CSP.
//
// `popup.ts` / `options.ts` live in extension pages and *could* fetch
// directly, but we send them through the same path for consistency and so
// errors come back through one channel.

import { loadSettings } from "./settings";
import type {
  CairnStatus,
  CaptureMetadata,
  CaptureResponse,
  Note,
  SearchPayload,
} from "./types";

export type FetchMessage = {
  type: "cairn-fetch";
  path: string;
  method: "GET" | "POST";
  query?: Record<string, string>;
  body?: unknown;
};

export type FetchResponse =
  | { ok: true; status: number; body: unknown }
  | { ok: false; status: number; error: string };

/** Run inside the background service worker — actually performs the HTTP
 *  request. Importable from the worker so the listener can use it.
 *
 *  Tries the user's configured `cairn_url` first; on connection errors
 *  (status 0 / network "Failed to fetch") cycles through `FALLBACK_URLS`
 *  so the extension keeps working when Cairn was upgraded and moved ports.
 *  Sends `Authorization: Bearer <token>` when `cairn_token` is set. */
export async function executeFetch(msg: FetchMessage): Promise<FetchResponse> {
  const s = await loadSettings();
  const { FALLBACK_URLS } = await import("./settings");
  // Build candidate list: user-configured first, then any fallback we haven't
  // already tried. Preserves user override while keeping resilience.
  const candidates = [s.cairn_url, ...FALLBACK_URLS.filter((u) => u !== s.cairn_url)];

  let lastError: FetchResponse | null = null;
  for (const base of candidates) {
    const out = await tryOne(base, s.cairn_token, msg);
    // Network-level failure (status 0) → try the next candidate.
    // Also fall back on 404 — that's the canonical "this URL hosts SOME
    // server but not Cairn's API" signal (e.g. MCP-SSE-only port from
    // earlier builds where /api/* was bundled with /sse). Without this,
    // a stale `cairn_url` returning 404 misleads the diagnoser into
    // "version mismatch" instead of "try the right port".
    if (!out.ok && (out.status === 0 || out.status === 404)) {
      lastError = out;
      continue;
    }
    return out;
  }
  return lastError ?? { ok: false, status: 0, error: "no candidate URLs" };
}

async function tryOne(base: string, token: string, msg: FetchMessage): Promise<FetchResponse> {
  try {
    const url = new URL(`${base.replace(/\/$/, "")}${msg.path}`);
    if (msg.query) {
      for (const [k, v] of Object.entries(msg.query)) url.searchParams.set(k, v);
    }
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (token) headers["authorization"] = `Bearer ${token}`;
    const init: RequestInit = {
      method: msg.method,
      headers,
      body: msg.body !== undefined ? JSON.stringify(msg.body) : undefined,
    };
    const res = await fetch(url.toString(), init);
    const text = await res.text();
    let parsed: unknown = null;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      parsed = text;
    }
    if (!res.ok) {
      const detail =
        (parsed && typeof parsed === "object" && "error" in parsed
          ? String((parsed as { error: unknown }).error)
          : null) ?? text;
      return { ok: false, status: res.status, error: detail || `HTTP ${res.status}` };
    }
    return { ok: true, status: res.status, body: parsed };
  } catch (e) {
    return { ok: false, status: 0, error: (e as Error).message };
  }
}

async function http<T>(
  path: string,
  method: "GET" | "POST",
  opts: { query?: Record<string, string | number | undefined>; body?: unknown } = {},
): Promise<T> {
  const query: Record<string, string> = {};
  if (opts.query) {
    for (const [k, v] of Object.entries(opts.query)) {
      if (v !== undefined) query[k] = String(v);
    }
  }
  const msg: FetchMessage = {
    type: "cairn-fetch",
    path,
    method,
    query: Object.keys(query).length ? query : undefined,
    body: opts.body,
  };
  const res = (await chrome.runtime.sendMessage(msg)) as FetchResponse | undefined;
  if (!res) throw new Error(`cairn ${path} → background not reachable`);
  if (!res.ok) throw new Error(`cairn ${path} → ${res.status} ${res.error}`);
  return res.body as T;
}

export async function status(): Promise<CairnStatus> {
  return http<CairnStatus>("/api/status", "GET");
}

export async function capture(
  text: string,
  source: string,
  metadata?: CaptureMetadata,
): Promise<CaptureResponse> {
  return http<CaptureResponse>("/api/capture", "POST", {
    body: { text, source, metadata },
  });
}

export async function search(q: string, limit = 5): Promise<SearchPayload> {
  return http<SearchPayload>("/api/search", "GET", {
    query: { q, limit },
  });
}

export async function recent(limit = 20): Promise<Note[]> {
  return http<Note[]>("/api/recent", "GET", { query: { limit } });
}
