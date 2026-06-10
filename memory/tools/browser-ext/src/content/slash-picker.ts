// Picker panel (mock `WNRWi`).
//
// Previously this was a single-select dropdown summoned by `@cairn <query>`.
// We now drive it from passive recall — the controller hands us the most
// recent `SearchPayload` plus the live composer text — and let the user pick
// any subset of memories to inject as a compact markdown block.
//
// Keyboard navigation is owned partly by the controller (Enter / Esc / arrows
// are swallowed in document capture so the host's React handlers never see
// them) and partly here (1/2/3 keys + checkbox toggling). The controller
// dispatches to `move()` / `confirm()` / `toggleAt()`.
//
// Pure DOM — no framework — so the panel can be injected into any host
// without leaking React state. Anchored with `position: fixed` so it stays
// in lockstep with the indicator badge and survives composer reflows.

import type { RetrievedEntity, RetrievedNote, SearchPayload } from "@/lib/types";
import { CAIRN_NS, ensureStyles } from "./styles";

export interface PickerItem {
  /** Stable id per row — entity id, or `note:<note_id>`. */
  id: string;
  /** Entity type (`person`, `belief`, `goal`, …) or `note` / `other`. */
  type: string;
  /** Display name (entity name, or a truncated note prefix). */
  name: string;
  /** Source + age subline (`selection from claude.ai · 12 days ago`). */
  subline: string;
  /** RRF score in [0, 1]. Drives the colour tier of the score pill. */
  score: number;
  /** Markdown line we paste into the composer when injected. */
  injectLine: string;
  /** Default selection state for the checkbox. Top-K strong rows default
   *  to checked; weak rows default unchecked. */
  selected: boolean;
}

export interface SlashPickerHandle {
  dispose: () => void;
  /** Replace the rows. The controller calls this when a fresh search lands
   *  while the picker is open. */
  setResults: (res: SearchPayload) => void;
  /** Show an error banner in place of the row list. */
  setError: (msg: string) => void;
  /** Move highlight (visual ring) by +1 / -1. */
  move: (delta: number) => void;
  /** Inject the currently selected rows (or the highlighted row if no
   *  checkboxes are ticked). */
  confirm: () => void;
  /** Toggle the checkbox of the row at `index` (1-based for hotkeys). */
  toggleAt: (index: number) => void;
  /** Re-position relative to the anchor. The controller calls this when
   *  the indicator badge moves or after the composer is re-hooked. */
  reposition: () => void;
}

export interface SlashPickerOptions {
  /** Live query text — used to drive the cache invalidation hint in the
   *  panel header. Optional; falls back to "" when unknown. */
  query?: string;
  /** Score threshold at and above which a row is "strong". */
  strongScore: number;
  /** Score threshold at and above which a row is shown (but not strong). */
  weakScore: number;
  /** Inject the chosen rows into the composer. The controller is
   *  responsible for actually doing the text replacement. */
  onInject: (items: PickerItem[]) => void;
  /** Close without injecting (Esc / outside-click). */
  onClose: () => void;
}

export function mountSlashPicker(
  anchor: HTMLElement,
  opts: SlashPickerOptions,
): SlashPickerHandle {
  ensureStyles();
  const panel = document.createElement("div");
  panel.className = `${CAIRN_NS}-slash-panel`;
  panel.setAttribute("role", "listbox");
  panel.tabIndex = -1;

  panel.innerHTML = `
    <div class="${CAIRN_NS}-slash-head">
      <span class="${CAIRN_NS}-tag">cairn</span>
      <span class="${CAIRN_NS}-head-title"></span>
      <span class="${CAIRN_NS}-spacer"></span>
      <span class="${CAIRN_NS}-hint">esc to dismiss</span>
    </div>
    <div class="${CAIRN_NS}-slash-body">
      <div class="${CAIRN_NS}-loading">searching…</div>
    </div>
    <div class="${CAIRN_NS}-slash-foot">
      <span class="${CAIRN_NS}-foot-summary">0 selected</span>
      <button class="${CAIRN_NS}-inject" disabled>
        Inject 0 <span class="${CAIRN_NS}-inject-arrow">→ Enter</span>
      </button>
    </div>
  `;

  const headTitle = panel.querySelector(`.${CAIRN_NS}-head-title`) as HTMLSpanElement;
  const body = panel.querySelector(`.${CAIRN_NS}-slash-body`) as HTMLDivElement;
  const footSummary = panel.querySelector(`.${CAIRN_NS}-foot-summary`) as HTMLSpanElement;
  const injectBtn = panel.querySelector(`.${CAIRN_NS}-inject`) as HTMLButtonElement;

  document.body.appendChild(panel);

  function reposition() {
    if (!document.contains(anchor)) return;
    const editable = anchor.getBoundingClientRect();
    if (editable.height === 0 || editable.width === 0) return;
    // ChatGPT and Claude both place a toolbar (attach / web-search / send)
    // *below* the editable. We walk up to find the sticky/fixed composer
    // wrapper and use ITS top edge as the ceiling — that way "flip above"
    // doesn't tuck under the toolbar.
    const block = composerBlockRect(anchor) ?? editable;
    const width = Math.min(560, Math.max(360, editable.width));
    const GAP = 8;
    const EDGE = 8;
    const vh = window.innerHeight;
    // How much usable airspace is above the composer block, and below?
    const spaceAbove = Math.max(0, block.top - GAP - EDGE);
    const spaceBelow = Math.max(0, vh - block.bottom - GAP - EDGE);
    // Cap the panel height to whichever side has room. We always pick the
    // larger side (it's where we render). 420 is the design ceiling; 120
    // is the floor below which the picker is too short to be usable —
    // anything tighter than that, we still render at 120 and accept a
    // small overlap rather than going invisible.
    const goBelow = spaceBelow >= spaceAbove;
    const available = goBelow ? spaceBelow : spaceAbove;
    const maxH = Math.min(420, Math.max(120, available));
    panel.style.maxHeight = `${maxH}px`;
    const actualHeight = Math.min(panel.scrollHeight || maxH, maxH);
    const top = goBelow
      ? block.bottom + GAP
      : Math.max(EDGE, block.top - GAP - actualHeight);
    panel.style.left = `${Math.max(8, editable.left)}px`;
    panel.style.top = `${top}px`;
    panel.style.width = `${width}px`;
  }

  function composerBlockRect(el: HTMLElement): DOMRect | null {
    // Walk up from the editable, keeping the largest ancestor that's still
    // anchored at the viewport bottom. We stop at the first sticky/fixed
    // ancestor (the composer chrome layer) — that's where ChatGPT/Claude
    // mount their toolbar + send-button cluster. Falling back to body
    // would be wrong (covers the entire page).
    //
    // We deliberately do NOT cap by height-fraction of viewport: on small
    // windows (DevTools open) the composer chrome itself can be > 50% of
    // viewport, which the previous version rejected and left the picker
    // anchored to the bare editable.
    const vh = window.innerHeight;
    let best: DOMRect | null = null;
    let cur: HTMLElement | null = el;
    while (cur && cur !== document.body) {
      const r = cur.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) break;
      if (r.bottom > vh + 12) break;       // not anchored at bottom anymore
      if (r.bottom < vh - 200) break;      // composer is way above viewport bottom
      best = r;
      const style = getComputedStyle(cur);
      if (style.position === "fixed" || style.position === "sticky") {
        // Reached the chrome layer — that's our ceiling.
        break;
      }
      cur = cur.parentElement;
    }
    return best;
  }
  reposition();

  const onReposition = () => reposition();
  window.addEventListener("scroll", onReposition, true);
  window.addEventListener("resize", onReposition);

  // Clicking outside the picker (and outside the composer) closes it.
  const outsideClick = (ev: MouseEvent) => {
    if (!(ev.target instanceof Node)) return;
    if (panel.contains(ev.target)) return;
    if (anchor.contains(ev.target)) return;
    opts.onClose();
  };
  window.addEventListener("mousedown", outsideClick, true);

  let items: PickerItem[] = [];
  let activeIndex = 0;
  let strongCount = 0;
  let weakCount = 0;

  function setHeader() {
    if (items.length === 0) {
      headTitle.textContent = "no matches yet";
      return;
    }
    if (weakCount > 0 && strongCount > 0) {
      headTitle.textContent = `${strongCount} strong ${strongCount === 1 ? "match" : "matches"} · ${weakCount} weak (hidden)`;
    } else if (strongCount > 0) {
      headTitle.textContent = `${strongCount} strong ${strongCount === 1 ? "match" : "matches"}`;
    } else if (weakCount > 0) {
      headTitle.textContent = `${weakCount} weak ${weakCount === 1 ? "match" : "matches"}`;
    } else {
      headTitle.textContent = `${items.length} ${items.length === 1 ? "match" : "matches"}`;
    }
  }

  function setFooter() {
    const selected = items.filter((i) => i.selected);
    const n = selected.length;
    if (n === 0) {
      footSummary.textContent = "0 selected";
      injectBtn.disabled = true;
      injectBtn.innerHTML = `Inject 0 <span class="${CAIRN_NS}-inject-arrow">→ Enter</span>`;
      return;
    }
    const avg = selected.reduce((a, b) => a + b.score, 0) / n;
    const scoreStr = selected.map((i) => i.score.toFixed(2)).join(" + ");
    footSummary.textContent = `${n} selected · ${scoreStr} (avg ${avg.toFixed(2)})`;
    injectBtn.disabled = false;
    injectBtn.innerHTML = `Inject ${n} <span class="${CAIRN_NS}-inject-arrow">→ Enter</span>`;
  }

  function renderRows() {
    body.innerHTML = "";
    if (items.length === 0) {
      body.innerHTML = `<div class="${CAIRN_NS}-empty">No matches in your memory yet — keep typing.</div>`;
      setHeader();
      setFooter();
      return;
    }
    items.forEach((it, idx) => {
      const row = document.createElement("div");
      row.className = `${CAIRN_NS}-row`;
      row.dataset.index = String(idx);
      if (idx === activeIndex) row.classList.add("active");

      const tier = it.score >= opts.strongScore ? "strong" : "weak";
      const kbd =
        idx < 3
          ? `<span class="${CAIRN_NS}-kbd">${idx + 1}</span>`
          : `<span class="${CAIRN_NS}-kbd-blank"></span>`;

      row.innerHTML = `
        <input type="checkbox" class="${CAIRN_NS}-checkbox" ${it.selected ? "checked" : ""} />
        <div class="${CAIRN_NS}-row-main">
          <div class="${CAIRN_NS}-row-head">
            <span class="${CAIRN_NS}-entity" data-type="${escapeAttr(it.type)}">${escapeText(it.type)}</span>
            <span class="${CAIRN_NS}-row-name"></span>
          </div>
          <div class="${CAIRN_NS}-row-sub"></div>
        </div>
        <span class="${CAIRN_NS}-score" data-tier="${tier}">${it.score.toFixed(2)}</span>
        ${kbd}
      `;
      // Plain-text setters so user-controlled strings can't inject HTML.
      (row.querySelector(`.${CAIRN_NS}-row-name`) as HTMLSpanElement).textContent = it.name;
      (row.querySelector(`.${CAIRN_NS}-row-sub`) as HTMLSpanElement).textContent = it.subline;

      const cb = row.querySelector(`.${CAIRN_NS}-checkbox`) as HTMLInputElement;
      cb.addEventListener("mousedown", (ev) => ev.stopPropagation());
      cb.addEventListener("click", (ev) => {
        ev.stopPropagation();
        it.selected = cb.checked;
        activeIndex = idx;
        markActive();
        setFooter();
      });

      row.addEventListener("mousedown", (ev) => {
        // Don't steal focus from the host composer — that breaks IME.
        ev.preventDefault();
      });
      row.addEventListener("click", (ev) => {
        // Click on the body (not checkbox): switch to single-select mode and
        // mark this row alone. The quick path — matches Notion/Slack pickers.
        if (ev.target instanceof Element && ev.target.closest(`.${CAIRN_NS}-checkbox`)) return;
        for (const x of items) x.selected = false;
        it.selected = true;
        activeIndex = idx;
        // Refresh checkboxes in place.
        body.querySelectorAll<HTMLInputElement>(`.${CAIRN_NS}-checkbox`).forEach((box, i) => {
          box.checked = items[i]?.selected ?? false;
        });
        markActive();
        setFooter();
      });

      body.appendChild(row);
    });
    setHeader();
    setFooter();
  }

  function markActive() {
    const rows = body.querySelectorAll(`.${CAIRN_NS}-row`);
    rows.forEach((r, i) => r.classList.toggle("active", i === activeIndex));
    rows[activeIndex]?.scrollIntoView({ block: "nearest" });
  }

  injectBtn.addEventListener("click", (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    confirm();
  });
  injectBtn.addEventListener("mousedown", (ev) => ev.preventDefault());

  function confirm() {
    const selected = items.filter((i) => i.selected);
    if (selected.length === 0) {
      // Fall back to highlighted row when nothing is ticked.
      const fallback = items[activeIndex];
      if (fallback) {
        opts.onInject([fallback]);
      } else {
        opts.onClose();
      }
      return;
    }
    opts.onInject(selected);
  }

  function setResults(res: SearchPayload) {
    const packed = packResults(res, opts.strongScore, opts.weakScore);
    items = packed.items;
    strongCount = packed.strong;
    weakCount = packed.weak;
    activeIndex = 0;
    renderRows();
    // After rendering, reposition because the panel's intrinsic height may
    // have changed (empty → 5 rows etc.).
    reposition();
  }

  return {
    dispose: () => {
      window.removeEventListener("mousedown", outsideClick, true);
      window.removeEventListener("scroll", onReposition, true);
      window.removeEventListener("resize", onReposition);
      panel.remove();
    },
    setResults,
    setError: (msg) => {
      body.innerHTML = "";
      const e = document.createElement("div");
      e.className = `${CAIRN_NS}-error`;
      e.textContent = `Cairn error: ${msg}`;
      body.appendChild(e);
      headTitle.textContent = "error";
      footSummary.textContent = "—";
      injectBtn.disabled = true;
    },
    move: (delta) => {
      if (items.length === 0) return;
      activeIndex = (activeIndex + delta + items.length) % items.length;
      markActive();
    },
    confirm,
    toggleAt: (oneBased) => {
      const idx = oneBased - 1;
      const it = items[idx];
      if (!it) return;
      it.selected = !it.selected;
      activeIndex = idx;
      // Reflect in DOM without a full re-render.
      const cb = body
        .querySelectorAll<HTMLInputElement>(`.${CAIRN_NS}-checkbox`)
        .item(idx);
      if (cb) cb.checked = it.selected;
      markActive();
      setFooter();
    },
    reposition,
  };
}

// ----- Helpers ------------------------------------------------------------

interface PackedResults {
  items: PickerItem[];
  /** Rows kept and rendered with the "strong" score pill colour. */
  strong: number;
  /** Rows below `strongScore` but at or above `weakScore`. We still render
   *  these in the picker so the user can opt-in; the indicator badge,
   *  however, advertises them differently. */
  weak: number;
}

function packResults(
  res: SearchPayload,
  strongScore: number,
  weakScore: number,
): PackedResults {
  const all: PickerItem[] = [];
  for (const re of res.entities) {
    const s = entityScore(re);
    if (s < weakScore) continue;
    all.push({
      id: re.entity.id,
      type: (re.entity.type ?? "other").toLowerCase(),
      name: re.entity.name,
      subline: entitySubline(re),
      score: s,
      injectLine: entityInjectLine(re),
      selected: false,
    });
  }
  for (const rn of res.notes) {
    const s = noteScore(rn);
    if (s < weakScore) continue;
    all.push({
      id: `note:${rn.note.id}`,
      type: "note",
      name: truncate(rn.note.text.replace(/\s+/g, " "), 80),
      subline: noteSubline(rn),
      score: s,
      injectLine: noteInjectLine(rn),
      selected: false,
    });
  }
  // Highest score first. The picker's top-3 hotkey slots go to the best
  // matches, which is what users want — `1` is your strongest.
  all.sort((a, b) => b.score - a.score);

  // Defaults: top 3 strong rows auto-selected; weak rows default unselected.
  let strongPicked = 0;
  for (const it of all) {
    if (it.score >= strongScore && strongPicked < 3) {
      it.selected = true;
      strongPicked++;
    }
  }
  const strong = all.filter((i) => i.score >= strongScore).length;
  const weak = all.length - strong;
  return { items: all, strong, weak };
}

function entityScore(re: RetrievedEntity): number {
  if (typeof re.rrf_score === "number" && Number.isFinite(re.rrf_score)) return re.rrf_score;
  if (typeof re.final_score === "number" && Number.isFinite(re.final_score)) return re.final_score;
  if (typeof re.score === "number" && Number.isFinite(re.score)) return re.score;
  return 0;
}

function noteScore(rn: RetrievedNote): number {
  if (typeof rn.rrf_score === "number" && Number.isFinite(rn.rrf_score)) return rn.rrf_score;
  if (typeof rn.final_score === "number" && Number.isFinite(rn.final_score)) return rn.final_score;
  if (typeof rn.score === "number" && Number.isFinite(rn.score)) return rn.score;
  return 0;
}

function entitySubline(re: RetrievedEntity): string {
  const parts: string[] = [];
  const src = entitySource(re);
  if (src) parts.push(src);
  const age = relativeTime(Number(re.entity.last_accessed || re.entity.updated_at || re.entity.created_at || 0));
  if (age) parts.push(age);
  const ac = Number(re.entity.access_count || 0);
  if (ac > 0) parts.push(`used ${ac}×`);
  return parts.join(" · ");
}

function noteSubline(rn: RetrievedNote): string {
  const parts: string[] = [];
  if (rn.note.source) parts.push(rn.note.source);
  const age = relativeTime(Number(rn.note.created_at || 0));
  if (age) parts.push(age);
  return parts.join(" · ");
}

function entitySource(re: RetrievedEntity): string {
  // We don't have a direct source on the entity. Use the note source if
  // available via source_note_id; otherwise hint at "manual capture".
  const sni = re.entity.source_note_id;
  if (sni) return `from note ${sni.slice(0, 6)}`;
  return "manual capture";
}

// Property keys we always lead with (in this order) when present, because
// they're the most decision-critical for the common entity types. Anything
// not in this list is appended after, alphabetically, so the model still
// sees the long tail.
const HEADLINE_PROP_KEYS = [
  "summary",
  "description",
  "date",
  "time",
  "location",
  "address",
  "value",
  "target",
  "position",
  "role",
  "company",
  "kind",
  "url",
];

function entityInjectLine(re: RetrievedEntity): string {
  const props = parseProps(re.entity.properties);

  // Fold *every* leaf-typed property into the inject line — previously this
  // hardcoded 5 keys (summary/description/value/target/position) and silently
  // dropped everything else, which lost decision-critical fields like
  // Event.date / Event.location / Person.role / Asset.url. The model has no
  // way to ask follow-up questions on a memory it can't see, so a complete
  // dump beats an opinionated abstract.
  const parts: string[] = [];
  const seen = new Set<string>();
  const emit = (k: string) => {
    if (seen.has(k)) return;
    const v = props[k];
    if (v == null) return;
    if (typeof v === "object") return; // skip nested objects/arrays
    const s = String(v).trim();
    if (!s) return;
    seen.add(k);
    parts.push(`${k}: ${truncate(s, 60)}`);
  };
  for (const k of HEADLINE_PROP_KEYS) emit(k);
  for (const k of Object.keys(props).sort()) emit(k);

  const subline = entitySubline(re);
  const meta = subline ? ` (${truncate(subline, 60)})` : "";
  const head = `${(re.entity.type ?? "memory").toLowerCase()}: ${re.entity.name}`;
  // Per-line cap raised from 200 to 320 because the format-block step caps
  // each line to 200, and we want to fit name + 2–3 key=value pairs before
  // that final clamp truncates.
  const body = parts.length > 0 ? ` — ${parts.join(" · ")}` : "";
  return truncate(`${head}${body}${meta}`, 320);
}

function noteInjectLine(rn: RetrievedNote): string {
  const text = rn.note.text.replace(/\s+/g, " ").trim();
  const sub = noteSubline(rn);
  const meta = sub ? ` (${truncate(sub, 60)})` : "";
  // Notes are the user's narrative memories — their value is the *complete*
  // text, not a snippet. 800 chars holds a paragraph-sized note in full
  // while still bounding pathological pastes. The 200-char cap that lived
  // here previously was inherited from entity inject lines and silently
  // clipped notes mid-sentence.
  return truncate(`note: ${text}${meta}`, 800);
}

function parseProps(raw: string | undefined): Record<string, unknown> {
  if (!raw) return {};
  try {
    return (JSON.parse(raw) as Record<string, unknown>) ?? {};
  } catch {
    return {};
  }
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, Math.max(0, max - 1)) + "…";
}

function escapeText(s: string): string {
  return s.replace(/[&<>"]/g, (c) =>
    c === "&" ? "&amp;" : c === "<" ? "&lt;" : c === ">" ? "&gt;" : "&quot;",
  );
}

function escapeAttr(s: string): string {
  return escapeText(s);
}

/** Short relative-time string ("12d ago", "5m ago", "just now"). Accepts a
 *  unix-seconds OR unix-ms epoch — anything below 2001-01-01 in ms is
 *  treated as seconds. */
export function relativeTime(epoch: number): string {
  if (!epoch) return "";
  // Cairn stores `created_at` as unix-ms in the JSON API; some older notes
  // round-trip through seconds. Heuristic: a value < 1e12 is seconds.
  const ms = epoch < 1e12 ? epoch * 1000 : epoch;
  const diff = Date.now() - ms;
  if (diff < 0) return "just now";
  const sec = Math.floor(diff / 1000);
  if (sec < 30) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d ago`;
  const mo = Math.floor(day / 30);
  if (mo < 12) return `${mo}mo ago`;
  const yr = Math.floor(day / 365);
  return `${yr}y ago`;
}
