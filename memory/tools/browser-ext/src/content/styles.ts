// One stylesheet for the slash picker, passive indicator, and save badge —
// injected once per page. We use a unique namespace prefix on every class
// so we don't clash with the host site's CSS.
//
// Colours mirror the Pencil design tokens from `untitled.pen`
// (`get_variables`). Entity colours match `entity-*` tokens 1:1 so the tag
// pills look identical to mocks like `WNRWi`.

export const CAIRN_NS = "cairn-ext";

let injected = false;

export function ensureStyles() {
  if (injected) return;
  injected = true;
  const style = document.createElement("style");
  style.dataset.cairnExt = "1";
  style.textContent = `
    /* === Passive recall indicator (oDjEW) =============================== */
    .${CAIRN_NS}-indicator {
      position: fixed;
      z-index: 2147483646;
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 10px;
      border-radius: 999px;
      background: #E6F4EE; /* accent-dim */
      color: #00603F;      /* accent-fg */
      border: 1px solid rgba(0, 135, 90, 0.35);
      font-family: -apple-system, BlinkMacSystemFont, "Inter", sans-serif;
      font-size: 11.5px;
      font-weight: 500;
      cursor: pointer;
      box-shadow: 0 4px 12px -4px rgba(0, 135, 90, 0.18);
      transition: transform 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
      user-select: none;
    }
    .${CAIRN_NS}-indicator:hover {
      background: #D2EBDF;
      transform: translateY(-1px);
      box-shadow: 0 8px 16px -6px rgba(0, 135, 90, 0.28);
    }
    .${CAIRN_NS}-indicator[data-tier="weak"] {
      background: #F5F5F4; /* bg-sunken */
      color: #737373;      /* ink-secondary */
      border-color: rgba(0, 0, 0, 0.10);
      box-shadow: none;
    }
    .${CAIRN_NS}-indicator[data-tier="error"] {
      background: #FEE2E2; /* error-dim */
      color: #DC2626;      /* error */
      border-color: rgba(220, 38, 38, 0.30);
    }
    .${CAIRN_NS}-indicator-dot {
      display: inline-block;
      width: 6px;
      height: 6px;
      border-radius: 999px;
      background: #00875A; /* accent */
    }
    .${CAIRN_NS}-indicator[data-tier="weak"] .${CAIRN_NS}-indicator-dot {
      background: #A3A3A3; /* ink-tertiary */
    }
    .${CAIRN_NS}-indicator[data-tier="error"] .${CAIRN_NS}-indicator-dot {
      background: #DC2626;
    }
    .${CAIRN_NS}-indicator-dismiss {
      margin-left: 4px;
      color: rgba(0, 96, 63, 0.55);
      font-size: 12px;
      line-height: 1;
      cursor: pointer;
      padding: 0 2px;
      border-radius: 4px;
    }
    .${CAIRN_NS}-indicator-dismiss:hover { color: #00603F; background: rgba(0, 135, 90, 0.10); }
    .${CAIRN_NS}-indicator[data-tier="weak"] .${CAIRN_NS}-indicator-dismiss { color: rgba(115, 115, 115, 0.55); }
    .${CAIRN_NS}-indicator[data-tier="weak"] .${CAIRN_NS}-indicator-dismiss:hover { color: #0A0A0A; background: rgba(0, 0, 0, 0.06); }

    /* === Picker panel (WNRWi) ========================================= */
    .${CAIRN_NS}-slash-panel {
      position: fixed;
      z-index: 2147483646;
      background: #FFFFFF; /* bg-surface */
      color: #0A0A0A;      /* ink-primary */
      border: 1px solid #E5E5E5; /* border */
      border-radius: 12px; /* r-xl */
      box-shadow: 0 24px 48px -12px rgba(0, 0, 0, 0.18), 0 4px 12px -2px rgba(0, 0, 0, 0.08);
      font-family: -apple-system, BlinkMacSystemFont, "Inter", sans-serif;
      font-size: 13px;
      overflow: hidden;
      max-height: 420px;
      display: flex;
      flex-direction: column;
    }
    @media (prefers-color-scheme: dark) {
      .${CAIRN_NS}-slash-panel {
        background: #1A1A18;
        color: #F4F4F3;
        border-color: rgba(255, 255, 255, 0.08);
      }
    }

    .${CAIRN_NS}-slash-head {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 10px 12px;
      border-bottom: 1px solid #E5E5E5;
      font-size: 12px;
      color: #737373; /* ink-secondary */
    }
    @media (prefers-color-scheme: dark) {
      .${CAIRN_NS}-slash-head { border-bottom-color: rgba(255, 255, 255, 0.06); }
    }
    .${CAIRN_NS}-tag {
      display: inline-flex;
      align-items: center;
      padding: 2px 8px;
      border-radius: 999px;
      background: #00875A; /* accent */
      color: #FFFFFF;
      font-weight: 600;
      letter-spacing: 0.06em; /* tracking-caps */
      text-transform: uppercase;
      font-size: 10px;
    }
    .${CAIRN_NS}-head-title { color: #0A0A0A; font-weight: 500; }
    @media (prefers-color-scheme: dark) { .${CAIRN_NS}-head-title { color: #F4F4F3; } }
    .${CAIRN_NS}-spacer { flex: 1; }
    .${CAIRN_NS}-hint {
      color: #A3A3A3; /* ink-tertiary */
      font-size: 11px;
    }

    /* Rows */
    .${CAIRN_NS}-slash-body {
      overflow-y: auto;
      max-height: 320px;
      padding: 4px 0;
    }
    .${CAIRN_NS}-loading,
    .${CAIRN_NS}-empty,
    .${CAIRN_NS}-error {
      padding: 16px;
      color: #737373;
      font-size: 12.5px;
    }
    .${CAIRN_NS}-error { color: #DC2626; }
    .${CAIRN_NS}-row {
      display: grid;
      grid-template-columns: auto 1fr auto auto;
      align-items: center;
      column-gap: 10px;
      padding: 8px 12px;
      cursor: pointer;
      border-left: 2px solid transparent;
      transition: background 0.10s ease, border-color 0.10s ease;
    }
    .${CAIRN_NS}-row:hover { background: rgba(0, 0, 0, 0.03); }
    .${CAIRN_NS}-row.active {
      background: rgba(0, 135, 90, 0.06);
      border-left-color: #00875A;
    }
    @media (prefers-color-scheme: dark) {
      .${CAIRN_NS}-row:hover { background: rgba(255, 255, 255, 0.04); }
      .${CAIRN_NS}-row.active { background: rgba(0, 135, 90, 0.10); }
    }

    .${CAIRN_NS}-checkbox {
      appearance: none;
      -webkit-appearance: none;
      width: 16px;
      height: 16px;
      border: 1.5px solid #D4D4D4; /* border-strong */
      border-radius: 4px; /* r-sm */
      background: #FFFFFF;
      cursor: pointer;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      transition: border-color 0.10s ease, background 0.10s ease;
      flex: 0 0 auto;
      margin: 0;
    }
    .${CAIRN_NS}-checkbox:checked {
      background: #00875A;
      border-color: #00875A;
    }
    .${CAIRN_NS}-checkbox:checked::after {
      content: "";
      width: 4px;
      height: 8px;
      border: solid #FFFFFF;
      border-width: 0 1.5px 1.5px 0;
      transform: rotate(45deg) translate(-1px, -1px);
    }

    .${CAIRN_NS}-row-main { min-width: 0; }
    .${CAIRN_NS}-row-head {
      display: flex;
      align-items: center;
      gap: 8px;
      min-width: 0;
    }
    .${CAIRN_NS}-row-name {
      font-weight: 600;
      color: #0A0A0A;
      font-size: 13px;
      line-height: 1.3;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      min-width: 0;
    }
    @media (prefers-color-scheme: dark) { .${CAIRN_NS}-row-name { color: #F4F4F3; } }
    .${CAIRN_NS}-row-sub {
      color: #737373;
      font-size: 11.5px;
      line-height: 1.3;
      margin-top: 2px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    /* Entity tag pill (matches entity-* tokens) */
    .${CAIRN_NS}-entity {
      display: inline-flex;
      align-items: center;
      padding: 1px 8px;
      border-radius: 999px;
      font-size: 10px;
      font-weight: 600;
      letter-spacing: 0.04em;
      text-transform: lowercase;
      color: #FFFFFF;
      flex: 0 0 auto;
    }
    .${CAIRN_NS}-entity[data-type="person"]     { background: #D97757; }
    .${CAIRN_NS}-entity[data-type="belief"]     { background: #9A6CB2; }
    .${CAIRN_NS}-entity[data-type="goal"]       { background: #D4A017; }
    .${CAIRN_NS}-entity[data-type="event"]      { background: #5B8DEF; }
    .${CAIRN_NS}-entity[data-type="location"]   { background: #5A8A3A; }
    .${CAIRN_NS}-entity[data-type="asset"]      { background: #0D6E6E; }
    .${CAIRN_NS}-entity[data-type="preference"] { background: #6B8E23; }
    .${CAIRN_NS}-entity[data-type="skill"]      { background: #B85450; }
    .${CAIRN_NS}-entity[data-type="note"]       { background: #737373; }
    .${CAIRN_NS}-entity[data-type="other"]      { background: #737373; }

    /* Score pill */
    .${CAIRN_NS}-score {
      display: inline-flex;
      align-items: center;
      padding: 2px 8px;
      border-radius: 999px;
      font-family: "Geist Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 11px;
      font-weight: 600;
      color: #FFFFFF;
      flex: 0 0 auto;
    }
    .${CAIRN_NS}-score[data-tier="strong"] { background: #00875A; }
    .${CAIRN_NS}-score[data-tier="weak"]   { background: #D97706; }

    .${CAIRN_NS}-kbd {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 20px;
      height: 20px;
      padding: 0 5px;
      border-radius: 4px;
      background: #F5F5F4; /* bg-sunken */
      color: #737373;
      font-family: "Geist Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 10.5px;
      border: 1px solid #E5E5E5;
      flex: 0 0 auto;
    }
    .${CAIRN_NS}-kbd-blank {
      width: 20px;
      flex: 0 0 auto;
    }
    @media (prefers-color-scheme: dark) {
      .${CAIRN_NS}-kbd { background: rgba(255,255,255,0.04); color: #A8A8A4; border-color: rgba(255,255,255,0.08); }
    }

    /* Footer */
    .${CAIRN_NS}-slash-foot {
      display: flex;
      align-items: center;
      gap: 10px;
      padding: 8px 12px;
      border-top: 1px solid #E5E5E5;
      font-size: 12px;
      color: #737373;
    }
    @media (prefers-color-scheme: dark) {
      .${CAIRN_NS}-slash-foot { border-top-color: rgba(255, 255, 255, 0.06); }
    }
    .${CAIRN_NS}-foot-summary {
      flex: 1;
      font-family: "Geist Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 11px;
    }
    .${CAIRN_NS}-inject {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 6px 12px;
      border-radius: 999px;
      background: #0A0A0A; /* bg-inverse */
      color: #FAFAFA;      /* ink-on-inverse */
      border: 0;
      font: inherit;
      font-weight: 500;
      font-size: 12px;
      cursor: pointer;
      transition: opacity 0.15s ease, transform 0.10s ease;
    }
    .${CAIRN_NS}-inject:hover:not(:disabled) {
      transform: translateY(-1px);
    }
    .${CAIRN_NS}-inject:disabled {
      opacity: 0.40;
      cursor: not-allowed;
    }
    .${CAIRN_NS}-inject-arrow {
      font-family: "Geist Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
      opacity: 0.70;
    }

    /* === Save badge (unchanged) ====================================== */
    .${CAIRN_NS}-save-btn {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      margin: 6px 6px 0 0;
      padding: 3px 8px;
      border-radius: 999px;
      border: 1px solid rgba(0, 0, 0, 0.10);
      background: rgba(255, 255, 255, 0.82);
      color: #3c3c3a;
      font-family: inherit;
      font-size: 11px;
      font-weight: 500;
      cursor: pointer;
      vertical-align: middle;
      line-height: 1;
      transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
    }
    .${CAIRN_NS}-save-btn:hover {
      border-color: rgba(0, 0, 0, 0.18);
      background: #fff;
    }
    .${CAIRN_NS}-save-btn[data-state="saved"] {
      background: #ecfdf5;
      border-color: #34d399;
      color: #047857;
    }
    .${CAIRN_NS}-save-btn[data-state="saving"] { opacity: 0.7; }
    .${CAIRN_NS}-save-btn[data-state="error"] {
      background: #fef2f2;
      border-color: #f87171;
      color: #b91c1c;
    }
    @media (prefers-color-scheme: dark) {
      .${CAIRN_NS}-save-btn {
        background: rgba(255, 255, 255, 0.06);
        border-color: rgba(255, 255, 255, 0.12);
        color: #e7e7e5;
      }
      .${CAIRN_NS}-save-btn:hover {
        background: rgba(255, 255, 255, 0.10);
        border-color: rgba(255, 255, 255, 0.22);
      }
    }
    .${CAIRN_NS}-spin { animation: cairn-spin 0.8s linear infinite; transform-origin: 50% 50%; }
    @keyframes cairn-spin { to { transform: rotate(360deg); } }

    /* Save toast — bottom-right transient pill for async saves (context
       menu, slash-picker save). Inline message-level save-badge handles
       its own feedback separately. */
    .${CAIRN_NS}-toast-host {
      position: fixed;
      right: 16px;
      bottom: 16px;
      z-index: 2147483647;
      display: flex;
      flex-direction: column;
      gap: 8px;
      pointer-events: none;
    }
    .${CAIRN_NS}-toast {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      border-radius: 999px;
      background: #ffffff;
      color: #1a1a18;
      box-shadow: 0 10px 28px -8px rgba(0,0,0,0.22), 0 2px 6px -1px rgba(0,0,0,0.10);
      border: 1px solid rgba(0,0,0,0.06);
      font-family: -apple-system, BlinkMacSystemFont, "Inter", sans-serif;
      font-size: 12.5px;
      pointer-events: auto;
      transform: translateY(8px);
      opacity: 0;
      transition: transform 0.18s ease, opacity 0.18s ease;
    }
    .${CAIRN_NS}-toast.${CAIRN_NS}-toast-in {
      transform: translateY(0);
      opacity: 1;
    }
    .${CAIRN_NS}-toast[data-intent="saved"] .${CAIRN_NS}-toast-icon { color: #00875A; }
    .${CAIRN_NS}-toast[data-intent="error"] .${CAIRN_NS}-toast-icon { color: #DC2626; }
    .${CAIRN_NS}-toast-icon {
      display: inline-flex;
      align-items: center;
    }
    .${CAIRN_NS}-toast-msg { line-height: 1.2; }
    .${CAIRN_NS}-toast-btn {
      margin-left: 4px;
      background: none;
      border: 1px solid rgba(0,0,0,0.10);
      border-radius: 999px;
      color: #71716e;
      font: inherit;
      font-size: 11px;
      padding: 2px 8px;
      cursor: pointer;
    }
    .${CAIRN_NS}-toast-btn:hover { color: #1a1a18; border-color: rgba(0,0,0,0.20); }
    @media (prefers-color-scheme: dark) {
      .${CAIRN_NS}-toast {
        background: #232320;
        color: #f4f4f3;
        border-color: rgba(255,255,255,0.08);
      }
      .${CAIRN_NS}-toast-btn {
        border-color: rgba(255,255,255,0.14);
        color: #b8b8b4;
      }
      .${CAIRN_NS}-toast-btn:hover { color: #f4f4f3; border-color: rgba(255,255,255,0.28); }
    }
  `;
  document.documentElement.appendChild(style);
}
