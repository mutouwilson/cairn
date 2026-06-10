// Maps a failed `FetchResponse` (or a thrown Error wrapping one) to a
// human-friendly diagnosis. Centralising this here keeps `cairn.ts`'s
// URL-fallback / token plumbing untouched while letting the popup, options
// page, and content scripts share one ruleset.
//
// The shape returned is intentionally minimal: a `kind` enum for telemetry /
// branching, a one-line `message` for the user, and an `action` describing
// the most useful remediation. The caller decides how to render `action`
// (button, link, plain text), so the helper itself has zero UI assumptions.

import type { FetchResponse } from "./cairn";

export type DiagnosisKind =
  /** All candidate URLs returned status 0 — process likely not running. */
  | "cairn-not-running"
  /** Repeated status-0 with hung requests (> 3 s) — firewall / proxy. */
  | "firewall"
  /** HTTP 401 — bearer token missing or wrong. */
  | "unauthorized"
  /** HTTP 403 — server actively rejecting this caller. */
  | "forbidden"
  /** HTTP 404 on `/api/status` — older Cairn that predates the endpoint. */
  | "version-mismatch"
  /** Anything we couldn't classify. */
  | "unknown";

export interface DiagnosisAction {
  /** Short label suitable for a button or anchor. */
  label: string;
  /**
   * Hint for the renderer on what to wire the action to. Renderers map
   * these to concrete handlers — the helper itself never reaches into the
   * runtime.
   */
  intent:
    | "open-cairn"
    | "open-options"
    | "upgrade-cairn"
    | "open-help"
    | "none";
}

export interface Diagnosis {
  kind: DiagnosisKind;
  /** One-sentence explanation in plain English. */
  message: string;
  /** Suggested remediation; renderer decides how to surface it. */
  action: DiagnosisAction;
}

/**
 * Input the helper accepts:
 *   - A `FetchResponse` from `executeFetch` (preferred — has status + error).
 *   - A thrown `Error` whose `message` includes "→ 0 …" / "→ 401 …" etc.
 *   - A raw `{ status, error }` shape (e.g. from a manual `fetch`).
 */
export type DiagnoseInput =
  | FetchResponse
  | { status: number; error?: string; message?: string }
  | Error;

interface Normalised {
  status: number;
  error: string;
  hung?: boolean;
}

/** Convert any of the accepted inputs into the simple `{status, error}` we
 *  branch on. Tolerates the `Error` form thrown by `cairn.http()` whose
 *  message looks like `cairn /api/status → 0 Failed to fetch`. */
function normalise(input: DiagnoseInput, hung?: boolean): Normalised {
  if (input instanceof Error) {
    // Match the "→ <status> <rest>" pattern produced by `http()`.
    const m = input.message.match(/→\s*(\d+)\s+(.+)$/);
    if (m) return { status: Number(m[1]), error: m[2], hung };
    return { status: 0, error: input.message, hung };
  }
  if ("ok" in input) {
    // FetchResponse — only the error variant has useful info.
    if (input.ok) return { status: input.status, error: "", hung };
    return { status: input.status, error: input.error, hung };
  }
  return {
    status: input.status,
    error: input.error ?? input.message ?? "",
    hung,
  };
}

/**
 * Diagnose a single failure. `hung` is an optional hint from the caller —
 * pass `true` when the request was observed to hang > 3 s before status 0
 * came back, so the helper can pick `firewall` over `cairn-not-running`.
 */
export function diagnoseError(input: DiagnoseInput, hung?: boolean): Diagnosis {
  const n = normalise(input, hung);

  if (n.status === 401) {
    return {
      kind: "unauthorized",
      message:
        "Cairn rejected the request (401). The bearer token is missing or wrong.",
      action: { label: "Set token in Options", intent: "open-options" },
    };
  }
  if (n.status === 403) {
    return {
      kind: "forbidden",
      message:
        "Cairn refused the request (403). Check that this origin is allowed.",
      action: { label: "Open Options", intent: "open-options" },
    };
  }
  if (n.status === 404) {
    // The HTTP client always hits `/api/status` first from the popup; a 404
    // there means the local server is running but is too old to expose
    // the endpoint. Distinct from a generic 404 (which we'd still tag as
    // version mismatch because the extension only hits known paths).
    return {
      kind: "version-mismatch",
      message: "Cairn responded but doesn't expose the API the extension expects. Upgrade Cairn.",
      action: { label: "Upgrade Cairn", intent: "upgrade-cairn" },
    };
  }
  if (n.status === 0) {
    if (n.hung) {
      return {
        kind: "firewall",
        message:
          "Cairn isn't responding within 3 s. A firewall or proxy may be blocking 127.0.0.1.",
        action: { label: "Open help", intent: "open-help" },
      };
    }
    return {
      kind: "cairn-not-running",
      message:
        "Can't reach Cairn on 127.0.0.1. Make sure the desktop app is running.",
      action: { label: "Open Cairn", intent: "open-cairn" },
    };
  }
  return {
    kind: "unknown",
    message: n.error || `Cairn returned an unexpected error (${n.status}).`,
    action: { label: "Open Options", intent: "open-options" },
  };
}

/**
 * Convenience wrapper around a fetch attempt with timing — returns the
 * diagnosis directly. Callers that already have a `FetchResponse` can keep
 * using `diagnoseError(res)`; this one is for paths that want the hung-
 * timeout heuristic without writing it themselves.
 */
export async function diagnose<T>(
  attempt: () => Promise<T>,
  hangMs = 3000,
): Promise<{ ok: true; value: T } | { ok: false; diagnosis: Diagnosis }> {
  const start = performance.now();
  try {
    const value = await attempt();
    return { ok: true, value };
  } catch (err) {
    const elapsed = performance.now() - start;
    return {
      ok: false,
      diagnosis: diagnoseError(err as Error, elapsed >= hangMs),
    };
  }
}
