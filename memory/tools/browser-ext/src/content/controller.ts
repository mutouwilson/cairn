// Site-agnostic controller. Glues a `SiteAdapter` to three pieces of UI:
//
//   * Passive recall — every composer keystroke fires a debounced search
//     against /api/search. When at least one result clears the WEAK gate we
//     mount a small accent badge anchored to the composer's bottom-left
//     (mock `oDjEW`). Clicking it opens the picker.
//   * Picker — a multi-select panel (mock `WNRWi`) with checkboxes, entity
//     pills, score chips, 1/2/3 hotkeys, and an `Inject N → Enter` footer
//     button. The legacy `@cairn <query>` fast-path opens the picker
//     immediately (skipping the debounce) so muscle memory still works.
//   * `MutationObserver` on the conversation tree — for every assistant
//     message we render a "save to Cairn" badge (unchanged).
//
// Passive recall is gated:
//   * STRONG_SCORE = 0.60 → green pill, default-checked, advertised in the
//     indicator badge.
//   * WEAK_SCORE   = 0.30 → amber pill, default-unchecked, only mentioned
//     in the indicator if there are no strong hits.
//   * Below WEAK_SCORE → dropped entirely; we don't mount the indicator.
//
// We deliberately do NOT fire passive recall on every keystroke — only on
// debounced input where the composer text has actually changed and is long
// enough to be worth searching. See `MIN_PASSIVE_CHARS_WEIGHT`.

import * as cairn from "@/lib/cairn";
import type { Settings } from "@/lib/settings";
import type { SearchPayload } from "@/lib/types";
import {
  type SiteAdapter,
  getComposerText,
  setComposerText,
} from "./types";
import {
  mountSlashPicker,
  type PickerItem,
  type SlashPickerHandle,
} from "./slash-picker";
import {
  mountPassiveIndicator,
  type IndicatorHandle,
} from "./passive-indicator";
// Save-to-Cairn badge per assistant message was removed in favour of the
// macOS selection-popover save flow (highlight any text → Cairn pill).
// The badges duplicated the affordance and added visual noise above each
// reply. The MutationObserver below still runs so composer re-hooks stay
// reliable on SPA re-renders.

// ---- Quality gates -------------------------------------------------------
//
// `rrf_score` lives in `[0, 1]` after the retrieval layer normalises (see
// `src-tauri/src/retrieval.rs`). 0.60 / 0.30 are the same thresholds the
// Pencil mockup uses for its green/amber score pills.
const STRONG_SCORE = 0.6;
const WEAK_SCORE = 0.3;
/** Minimum *weighted* length before passive search fires. Each CJK char
 *  counts as 2, ASCII as 1 (see `effectiveLength` below). Set to 4 so:
 *    - 4 ASCII chars hit common single keywords ("work", "jane", "plan")
 *    - 2 CJK chars hit common Chinese entities ("李四", "百秋", "美国")
 *    - mixed input scales naturally ("AI 工程师" = 1+1+1+2+2 = 7 ✓)
 *  The previous flat 8 (then 4) char rule under-served CJK input: 2-char
 *  Chinese entity names are routine but never fired the pill, while 4
 *  CJK chars is already a multi-word phrase. */
const MIN_PASSIVE_CHARS_WEIGHT = 4;
/** Debounce window for passive searches. 200ms hits the "feels
 *  responsive without being jumpy" sweet spot — comparable to GitHub
 *  search-as-you-type (~200ms) and snappier than the old 350ms which
 *  was past the threshold where users notice the lag. Backend search
 *  is ~50ms so the whole loop stays under perceived-instant (300ms). */
const PASSIVE_DEBOUNCE_MS = 200;

/** Per-codepoint weight. CJK (U+3000 onwards covers CJK punctuation,
 *  ideographs, kana, full-width chars) carries roughly 2× the lexical
 *  density of Latin, so we count it that way for the search-trigger
 *  threshold. Cheap; avoids regexes on hot path. */
function effectiveLength(s: string): number {
  let n = 0;
  for (const c of s) {
    const code = c.codePointAt(0)!;
    n += code >= 0x3000 ? 2 : 1;
  }
  return n;
}
/** Limit for the backend search — we pull a few extra so the picker has
 *  room to surface 3 strong + a few weak. */
const PASSIVE_SEARCH_LIMIT = 10;

export function mountController(adapter: SiteAdapter, settings: Settings) {
  console.info(`[cairn] mounted on ${adapter.agent}`);

  // ---- 1. Passive recall on the composer -------------------------------
  let hookedEl: HTMLElement | null = null;
  let indicator: IndicatorHandle | null = null;
  let picker: SlashPickerHandle | null = null;

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let searchAbort: AbortController | null = null;
  /** Snapshot of the last query string we actually fired. Cheap dedup:
   *  if the user re-types the same string (whitespace-trimmed) we don't
   *  re-hit the backend. */
  let lastFiredQuery = "";
  /** The most recent successful search payload — the picker reads this when
   *  the user clicks the indicator badge so we don't have to re-search. */
  let lastResults: SearchPayload | null = null;
  /** Suppress the indicator when the user explicitly dismissed it for the
   *  current composer text. Resets when the user types something new
   *  (different query → different intent). */
  let dismissedFor = "";
  /** Same idea but for the picker auto-open: once the user closes the
   *  picker for a query, we don't re-open it until the composer text
   *  changes — otherwise Esc would immediately re-summon the panel they
   *  just dismissed. The indicator pill still stays available as a manual
   *  re-entry point. */
  let pickerDismissedFor = "";

  function ensureComposerHook() {
    const el = adapter.findComposer(document);
    if (!el) return;
    if (hookedEl && !document.contains(hookedEl)) {
      console.info(`[cairn] composer detached on ${adapter.agent}, re-hooking`);
      hookedEl = null;
      // The indicator's anchor is gone — kill it so the new composer can
      // mount a fresh one.
      teardownIndicator();
      teardownPicker();
    }
    if (hookedEl === el) return;
    attachComposerListeners(el);
    hookedEl = el;
    console.info(
      `[cairn] composer hooked on ${adapter.agent} (${el.tagName.toLowerCase()}#${el.id || "—"})`,
    );
  }

  function attachComposerListeners(composer: HTMLElement) {
    // `input` covers normal typing (works for both textarea and
    // contenteditable on every host we ship). `compositionupdate` keeps
    // IME-driven typing in sync — important for Chinese / Japanese chats.
    composer.addEventListener("input", () => scheduleSearch(composer));
    composer.addEventListener("compositionupdate", () => scheduleSearch(composer));
    // Some hosts re-mount the composer's child contenteditable when the
    // user clicks "+" or switches modes; the listener still fires through
    // bubbling, so we don't need to chase the inner node specifically.
  }

  function scheduleSearch(composer: HTMLElement) {
    const text = getComposerText(composer).trim();

    // Fast-path: legacy `@cairn <query>` summon. Strip the prefix and fire
    // the search immediately, skipping the debounce — that's the muscle
    // memory we promised to keep.
    const slash = matchSlashCommand(text, settings.slash_prefix);
    if (slash) {
      const q = slash.query || text.replace(escapeRegex(settings.slash_prefix), "").trim();
      // The slash command's intent is "open the picker now", so we mount
      // it eagerly even if results haven't landed yet.
      if (debounceTimer) clearTimeout(debounceTimer);
      void runSearch(composer, q, { openPicker: true });
      return;
    }

    // Passive path. Four gates:
    if (!settings.passive_recall) {
      // User opted out — only the slash fast-path above mounts the picker.
      teardownIndicator();
      return;
    }
    if (effectiveLength(text) < MIN_PASSIVE_CHARS_WEIGHT) {
      // Too short — clear any in-flight state and dismiss the indicator.
      if (debounceTimer) clearTimeout(debounceTimer);
      searchAbort?.abort();
      lastFiredQuery = "";
      lastResults = null;
      teardownIndicator();
      // Don't tear down the picker if it's open — the user might still be
      // reviewing; they can Esc out.
      return;
    }
    if (text === lastFiredQuery) {
      // Same query as last fire (the user is just moving the cursor or
      // pressing modifier keys). Skip.
      return;
    }
    if (text === dismissedFor) {
      // User dismissed the indicator for this exact text. Don't pop it
      // back up — they'll re-summon by typing something new or @cairn.
      return;
    }

    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      void runSearch(composer, text, { openPicker: false });
    }, PASSIVE_DEBOUNCE_MS);
  }

  async function runSearch(
    composer: HTMLElement,
    query: string,
    { openPicker }: { openPicker: boolean },
  ) {
    // Cancel any in-flight request — a fast typist shouldn't see stale
    // results pop in.
    searchAbort?.abort();
    searchAbort = new AbortController();
    lastFiredQuery = query;
    // Different query → user re-engaged. Reset the dismiss flag.
    dismissedFor = "";

    try {
      const res = await cairn.search(query, PASSIVE_SEARCH_LIMIT);
      // Stale check: another search may have started while we were awaiting.
      if (query !== lastFiredQuery) return;
      lastResults = res;

      const counts = countMatches(res);
      if (counts.strong === 0 && counts.weak === 0) {
        teardownIndicator();
        // If the picker is open and now has nothing, close it.
        if (picker && !openPicker) teardownPicker();
        return;
      }

      mountIndicatorIfNeeded(composer);
      indicator?.setCounts(counts.strong, counts.weak);

      // Open the picker eagerly for the legacy slash path. For the passive
      // path, also auto-open when there's at least one strong (>= 0.6)
      // match — "expand by default" — but respect `pickerDismissedFor` so
      // Esc isn't immediately undone, and only when the picker isn't already
      // up. Weak-only matches stay collapsed behind the indicator pill to
      // avoid hijacking attention when relevance is borderline.
      const shouldAutoOpen =
        !picker &&
        (openPicker ||
          (counts.strong > 0 && query !== pickerDismissedFor));
      if (shouldAutoOpen) openPickerNow(composer);
      if (picker) picker.setResults(res);
    } catch (e) {
      // Stale check before surfacing the error.
      if (query !== lastFiredQuery) return;
      const msg = (e as Error).message || "search failed";
      console.warn(`[cairn] passive search error: ${msg}`);
      // Show the error pill on the indicator only when one was already
      // visible — silent failures for purely-passive searches when no
      // indicator existed prior keep the UX quiet.
      indicator?.setError("backend unreachable");
      if (picker) picker.setError(msg);
    }
  }

  function countMatches(res: SearchPayload): { strong: number; weak: number } {
    let strong = 0;
    let weak = 0;
    for (const re of res.entities) {
      const s = scoreOfEntity(re);
      if (s >= STRONG_SCORE) strong++;
      else if (s >= WEAK_SCORE) weak++;
    }
    for (const rn of res.notes) {
      const s = scoreOfNote(rn);
      if (s >= STRONG_SCORE) strong++;
      else if (s >= WEAK_SCORE) weak++;
    }
    return { strong, weak };
  }

  function mountIndicatorIfNeeded(composer: HTMLElement) {
    if (indicator) return;
    indicator = mountPassiveIndicator(composer, {
      onClick: () => {
        if (picker) {
          teardownPicker();
          return;
        }
        openPickerNow(composer);
      },
      onDismiss: () => {
        // Soft-dismiss: hide for the current composer text only. The user
        // is signalling "not relevant right now"; re-summon by typing more.
        dismissedFor = getComposerText(composer).trim();
        teardownIndicator();
      },
    });
  }

  function teardownIndicator() {
    indicator?.dispose();
    indicator = null;
  }

  function openPickerNow(composer: HTMLElement) {
    if (picker) return;
    picker = mountSlashPicker(composer, {
      query: getComposerText(composer).trim(),
      strongScore: STRONG_SCORE,
      weakScore: WEAK_SCORE,
      onInject: (items) => injectAndClose(composer, items),
      onClose: () => {
        // Remember the query the user closed the picker for, so the
        // auto-open in runSearch doesn't immediately re-summon it on the
        // next debounced search of the same text. Cleared by typing
        // anything different.
        pickerDismissedFor = getComposerText(composer).trim();
        teardownPicker();
      },
    });
    // Populate from cache; if no cache (rare: indicator clicked before any
    // search landed) just leave the loading state and let the in-flight
    // search fill it in.
    if (lastResults) picker.setResults(lastResults);
  }

  function teardownPicker() {
    picker?.dispose();
    picker = null;
  }

  function injectAndClose(composer: HTMLElement, items: PickerItem[]) {
    const original = stripContextBlock(getComposerText(composer));
    const block = formatContextBlock(items);
    const final = `${block}\n\n${original}`.trim();
    setComposerText(composer, final);
    teardownPicker();
    // After injection, the composer text has changed and now starts with
    // `<cairn-context>` — that doesn't carry useful retrieval signal, so
    // we suppress passive recall until the user types more meaningful
    // text. `lastFiredQuery` set to the new text covers this: the
    // dedup check in `scheduleSearch` skips identical strings.
    lastFiredQuery = getComposerText(composer).trim();
    // Hide the indicator since we just spent its credit.
    teardownIndicator();
  }

  // ---- Document-level key swallow while picker is open ----
  //
  // Capture phase ⟹ runs before the host's React/Lexical handlers, so the
  // host's "Enter = submit" never fires.
  function onDocKey(ev: KeyboardEvent) {
    if (!picker) return;
    // IME guard: while an input method is composing (e.g. picking a CJK
    // candidate), hand EVERY key — Enter, digits, arrows, Esc — to the IME.
    // Otherwise the Enter that confirms a pinyin candidate gets swallowed as
    // "Inject", and the digit keys that pick a candidate toggle picker rows.
    // `keyCode === 229` covers browsers that flag composition that way even
    // when `isComposing` is momentarily false on the confirming keystroke.
    if (ev.isComposing || ev.keyCode === 229) return;
    if (ev.key === "Escape") {
      ev.preventDefault();
      ev.stopPropagation();
      ev.stopImmediatePropagation();
      teardownPicker();
      return;
    }
    if (ev.key === "Enter" && !ev.shiftKey && !ev.metaKey && !ev.ctrlKey) {
      ev.preventDefault();
      ev.stopPropagation();
      ev.stopImmediatePropagation();
      picker.confirm();
      return;
    }
    if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      ev.preventDefault();
      ev.stopPropagation();
      ev.stopImmediatePropagation();
      picker.move(ev.key === "ArrowDown" ? 1 : -1);
      return;
    }
    // Number hotkeys 1/2/3 toggle the corresponding row.
    if (ev.key === "1" || ev.key === "2" || ev.key === "3") {
      // Don't steal numerics that look like text content. We only want
      // bare digit presses (no modifier). When the user is typing into
      // the composer they may legitimately want a "1" — but the picker is
      // a focused panel, and the contract in the design is that 1/2/3
      // are picker hotkeys. We respect modifier-free, non-IME presses.
      if (ev.metaKey || ev.ctrlKey || ev.altKey) return;
      ev.preventDefault();
      ev.stopPropagation();
      ev.stopImmediatePropagation();
      picker.toggleAt(Number(ev.key));
    }
  }
  document.addEventListener("keydown", onDocKey, true);

  // ---- 2. Composer re-hook on SPA navigation ---------------------------
  // Per-assistant-message save badge was removed (Nov 2026). The
  // observer still has work to do: chat SPAs re-render the composer's
  // host element on conversation switch / mode toggle, so we re-run
  // `ensureComposerHook()` to pick up the new node.
  const observer = new MutationObserver(() => {
    ensureComposerHook();
  });
  observer.observe(document.body, { childList: true, subtree: true });

  // First pass.
  ensureComposerHook();

  // Adapter health canary: probe the selectors the adapter says it depends
  // on after the SPA has had a beat to render. Persist a stale-friendly
  // flag the popup reads — silent breakage is the failure mode we're
  // optimising for, not flake on a tab that opened mid-navigation.
  if (adapter.keySelectors && adapter.keySelectors.length > 0) {
    void scheduleCanary(adapter);
  }
}

// ---- Scoring helpers (kept here so the indicator/picker can stay dumb) ---

function scoreOfEntity(re: SearchPayload["entities"][number]): number {
  if (typeof re.rrf_score === "number" && Number.isFinite(re.rrf_score)) return re.rrf_score;
  if (typeof re.final_score === "number" && Number.isFinite(re.final_score)) return re.final_score;
  if (typeof re.score === "number" && Number.isFinite(re.score)) return re.score;
  return 0;
}

function scoreOfNote(rn: SearchPayload["notes"][number]): number {
  if (typeof rn.rrf_score === "number" && Number.isFinite(rn.rrf_score)) return rn.rrf_score;
  if (typeof rn.final_score === "number" && Number.isFinite(rn.final_score)) return rn.final_score;
  if (typeof rn.score === "number" && Number.isFinite(rn.score)) return rn.score;
  return 0;
}

// ---- Canary --------------------------------------------------------------

const CANARY_PROBE_DELAY_MS = 1500;

async function scheduleCanary(adapter: SiteAdapter) {
  // Wait for DOMContentLoaded (cheap if already past) then add ~1 s on top
  // so React/Angular/Vue mount the first render.
  if (document.readyState === "loading") {
    await new Promise<void>((resolve) =>
      document.addEventListener("DOMContentLoaded", () => resolve(), { once: true }),
    );
  }
  await new Promise<void>((resolve) => setTimeout(resolve, CANARY_PROBE_DELAY_MS));
  await runCanary(adapter);
}

async function runCanary(adapter: SiteAdapter) {
  const host = adapter.source_host;
  const selectors = adapter.keySelectors ?? [];
  // Batch storage I/O: read the existing flags once, compute the new state,
  // then issue a single set/remove pair.
  const keys = selectors.map((s) => `canary:${host}:${s.label}`);
  const existing = await chrome.storage.local.get(keys);
  const toSet: Record<string, unknown> = {};
  const toRemove: string[] = [];
  for (const s of selectors) {
    const k = `canary:${host}:${s.label}`;
    const hit = document.querySelector(s.selector);
    if (hit) {
      // Selector matched — clear any stale "missing" record.
      if (existing[k]) toRemove.push(k);
    } else if (s.optional) {
      // Optional selector legitimately absent right now (e.g. assistant-
      // message on a fresh chat). Don't warn — just make sure no stale
      // missing record persists from a previous load.
      if (existing[k]) toRemove.push(k);
    } else {
      // Record a new (or refresh existing) "missing" entry. Storing `at`
      // lets the popup distinguish a fresh miss from one we recorded
      // months ago and stopped caring about.
      toSet[k] = { missing: true, at: Date.now(), selector: s.selector, label: s.label, host };
    }
  }
  if (Object.keys(toSet).length > 0) await chrome.storage.local.set(toSet);
  if (toRemove.length > 0) await chrome.storage.local.remove(toRemove);
}

// ---- Slash command parsing ----------------------------------------------

function matchSlashCommand(
  text: string,
  prefix: string,
): { query: string } | null {
  // Tolerant match: prefix at the start, optionally followed by a space and
  // any rest. Just typing `@cairn` (no trailing space) is enough to open the
  // picker in empty-query state — same affordance as Notion/Slack slash
  // commands. The picker then renders a hint until the user types more.
  const re = new RegExp(`^\\s*${escapeRegex(prefix)}(?:\\s+(.*))?$`, "is");
  const m = text.match(re);
  if (!m) return null;
  return { query: (m[1] ?? "").trim() };
}

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// ---- Context block format ------------------------------------------------

/** Build the compact `<cairn-context>` markdown block from the selected
 *  picker rows. Matches mock `U7uUKA`:
 *
 *  ```
 *  <cairn-context memories=2>
 *  - event: 上海美国大使馆面签 — date: 2026-06-25 · location: 上海美国大使馆 (from note 01KSPP · 2h ago)
 *  - goal: 找投资人优先聊 mira (manual, 5d ago)
 *  </cairn-context>
 *  ```
 *  Per-line cap raised from 120 → 200 chars so the entity's key properties
 *  (Event.date, Event.location, Person.role, Asset.url, …) fit in the same
 *  line as the name. The earlier 120-char limit was set when each line
 *  carried a single hand-picked "summary" field; now that we dump all
 *  leaf-typed properties (see `entityInjectLine`), 120 silently chopped
 *  decision-critical fields like dates. */
function formatContextBlock(items: PickerItem[]): string {
  const head = `<cairn-context memories=${items.length}>`;
  // Per-line cap of 800 matches noteInjectLine — entity lines are bounded
  // by their own 320 cap, so they fit comfortably inside this envelope.
  const lines = items.map((it) => `- ${truncate(it.injectLine, 800)}`);
  return [head, ...lines, "</cairn-context>"].join("\n");
}

/** Remove any pre-existing `<cairn-context>...</cairn-context>` block from
 *  the composer text. We always replace rather than accumulate — repeated
 *  injections produced a wall of nested blocks. */
function stripContextBlock(text: string): string {
  return text.replace(/<cairn-context[\s\S]*?<\/cairn-context>\n*/gi, "").trim();
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, Math.max(0, max - 1)) + "…";
}

