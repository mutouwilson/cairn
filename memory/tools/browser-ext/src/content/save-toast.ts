// Transient bottom-right pill announcing a successful save. Used by the
// background context-menu and selection paths where the user can't see the
// inline `save` badge. The inline badge on assistant messages handles its
// own feedback in `save-badge.ts`.
//
// Stateless: each call mounts a fresh element. Stacks downward when many
// arrive in quick succession.

import { CAIRN_NS, ensureStyles } from "./styles";

interface ToastOpts {
  noteId?: string;
  ttlMs?: number;
  /// "saved" (accent green pill) or "error" (red).
  intent?: "saved" | "error";
}

export function showSaveToast(text: string, opts: ToastOpts = {}): void {
  ensureStyles();
  const host = ensureHost();
  const wrap = document.createElement("div");
  wrap.className = `${CAIRN_NS}-toast`;
  wrap.dataset.intent = opts.intent ?? "saved";

  const icon = opts.intent === "error" ? ICON_X : ICON_CHECK;
  wrap.innerHTML = `<span class="${CAIRN_NS}-toast-icon">${icon}</span><span class="${CAIRN_NS}-toast-msg"></span>`;
  (wrap.querySelector(`.${CAIRN_NS}-toast-msg`) as HTMLElement).textContent = text;

  // Optional `view` action — when the host registers a `cairn://` URL
  // scheme this opens the entity page directly. Until then we just copy the
  // note id to clipboard so the user can paste it into the Cairn search bar.
  if (opts.noteId) {
    const btn = document.createElement("button");
    btn.className = `${CAIRN_NS}-toast-btn`;
    btn.type = "button";
    btn.textContent = "copy id";
    btn.addEventListener("click", () => {
      void navigator.clipboard?.writeText(opts.noteId!);
      btn.textContent = "copied";
      setTimeout(() => (btn.textContent = "copy id"), 1200);
    });
    wrap.appendChild(btn);
  }

  host.appendChild(wrap);
  // Trigger CSS enter animation on next frame.
  requestAnimationFrame(() => wrap.classList.add(`${CAIRN_NS}-toast-in`));

  const ttl = opts.ttlMs ?? 2400;
  setTimeout(() => {
    wrap.classList.remove(`${CAIRN_NS}-toast-in`);
    setTimeout(() => wrap.remove(), 220);
  }, ttl);
}

function ensureHost(): HTMLDivElement {
  const id = `${CAIRN_NS}-toast-host`;
  let host = document.getElementById(id) as HTMLDivElement | null;
  if (host) return host;
  host = document.createElement("div");
  host.id = id;
  host.className = `${CAIRN_NS}-toast-host`;
  document.documentElement.appendChild(host);
  return host;
}

const ICON_CHECK = `<svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3.5 8.5l3 3 6-6.5"/></svg>`;
const ICON_X = `<svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8"/></svg>`;
