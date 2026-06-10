// Background service worker. Owns:
//   - Right-click context menu items ("Save selection to Cairn", "Search Cairn").
//   - The Ctrl/Cmd+Shift+L hotkey that opens the search popup as the side panel.
//   - The cross-origin fetch proxy used by content scripts (chat sites'
//     CSP usually blocks direct localhost calls).
//
// Keepalive (belt & suspenders): MV3 terminates the service worker after
// ~30 s idle, and the first call after a cold start drops 100–300 ms of
// latency on the floor — long enough for `@cairn` slash typing to feel
// laggy. Two complementary mechanisms keep it warm:
//
//   1) `chrome.alarms` fires every ~24 s; the listener does nothing, but
//      the dispatch itself resets the idle timer.
//   2) Each content script opens a long-lived `chrome.runtime.connect()`
//      port named "cairn-keepalive". An open port also pins the worker
//      alive, and we get a chance to react if the page is closed.
//
// Either alone is enough; together they cover the edge cases where one
// is suppressed (alarms can be throttled in low-power mode; the port
// only exists when a chat tab is open).
import { capture, executeFetch, search, type FetchMessage } from "@/lib/cairn";

// ---------- keepalive ----------

const KEEPALIVE_ALARM = "cairn-keepalive";
// 0.4 minutes = 24 s, safely under the ~30 s idle kill threshold.
const KEEPALIVE_PERIOD_MIN = 0.4;

function ensureKeepaliveAlarm() {
  chrome.alarms.get(KEEPALIVE_ALARM, (existing) => {
    if (existing) return;
    chrome.alarms.create(KEEPALIVE_ALARM, { periodInMinutes: KEEPALIVE_PERIOD_MIN });
  });
}

// Register at module load (covers cold start) and at install/startup so the
// alarm survives an upgrade.
ensureKeepaliveAlarm();
chrome.runtime.onStartup.addListener(() => ensureKeepaliveAlarm());

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name !== KEEPALIVE_ALARM) return;
  // The dispatch itself is what resets the idle timer; no work needed.
});

// Long-lived port from content scripts. The port presence pins the worker
// alive for as long as any chat tab is open, and we no-op the pings.
chrome.runtime.onConnect.addListener((port) => {
  if (port.name !== "cairn-keepalive") return;
  // Drop messages on the floor — the channel staying open is all we need.
  port.onMessage.addListener(() => {});
  port.onDisconnect.addListener(() => {
    // Reading `chrome.runtime.lastError` swallows the implicit warning when
    // a tab navigates away mid-port.
    void chrome.runtime.lastError;
  });
});

// ---------- fetch proxy for content scripts ----------

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type !== "cairn-fetch") return false;
  executeFetch(msg as FetchMessage)
    .then((res) => sendResponse(res))
    .catch((e) =>
      sendResponse({ ok: false, status: 0, error: (e as Error).message }),
    );
  // Returning `true` keeps the message channel open for the async response.
  return true;
});

// ---------- context menu ----------

chrome.runtime.onInstalled.addListener(() => {
  ensureKeepaliveAlarm();
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "cairn-save-selection",
      title: "Save selection to Cairn",
      contexts: ["selection"],
    });
    chrome.contextMenus.create({
      id: "cairn-search-selection",
      title: "Search Cairn for: %s",
      contexts: ["selection"],
    });
    chrome.contextMenus.create({
      id: "cairn-save-page",
      title: "Save this page to Cairn",
      contexts: ["page"],
    });
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (!info.menuItemId) return;
  try {
    if (info.menuItemId === "cairn-save-selection" && info.selectionText) {
      const res = await capture(info.selectionText, "browser-selection", {
        url: tab?.url,
        title: tab?.title,
        agent: "browser",
      });
      await celebrateSave(tab, "Saved to Cairn", res.note_id);
    } else if (info.menuItemId === "cairn-save-page" && tab?.id) {
      const summary = await summarisePage(tab.id, tab);
      const res = await capture(summary.text, "browser-page", {
        url: summary.url,
        title: summary.title,
        agent: "browser",
      });
      await celebrateSave(tab, "Saved page to Cairn", res.note_id);
    } else if (info.menuItemId === "cairn-search-selection" && info.selectionText) {
      const res = await search(info.selectionText, 5);
      await notify(
        `Cairn found ${res.entities.length} entities, ${res.notes.length} notes for "${truncate(info.selectionText, 40)}"`,
      );
    }
  } catch (e) {
    await notify(`Cairn error: ${(e as Error).message}`);
    if (tab?.id) await dispatchToast(tab.id, `Cairn error: ${(e as Error).message}`, { intent: "error" });
  }
});

/** Prefer the in-page toast (fast, contextual). Fall back to the system
 *  notification if the content script isn't loaded on the active tab. */
async function celebrateSave(tab: chrome.tabs.Tab | undefined, label: string, noteId?: string) {
  if (tab?.id) {
    const delivered = await dispatchToast(tab.id, label, { noteId });
    if (delivered) return;
  }
  await notify(label);
}

async function dispatchToast(
  tabId: number,
  text: string,
  opts: { noteId?: string; intent?: "saved" | "error" } = {},
): Promise<boolean> {
  try {
    await chrome.tabs.sendMessage(tabId, {
      type: "cairn-saved-toast",
      text,
      noteId: opts.noteId,
      intent: opts.intent ?? "saved",
    });
    return true;
  } catch {
    // Content script isn't installed on this tab (e.g. chrome://, PDF viewer).
    return false;
  }
}

// ---------- omnibox (URL bar: `cairn <query>`) ----------
//
// Skip opening the popup for "search memory" flows. As the user types, we
// hit the local /api/search endpoint and render up-to-5 suggestions inline
// in the URL bar. Enter on a suggestion opens the Cairn app's entity page;
// Enter on raw text just runs the search and opens results in Cairn.

chrome.omnibox?.setDefaultSuggestion?.({
  description: "Search Cairn memory — start typing…",
});

let omniboxDebounce: ReturnType<typeof setTimeout> | null = null;
let omniboxLastQuery = "";

chrome.omnibox?.onInputChanged.addListener((text, suggest) => {
  const q = text.trim();
  if (omniboxDebounce) clearTimeout(omniboxDebounce);
  if (!q) {
    suggest([]);
    return;
  }
  omniboxDebounce = setTimeout(async () => {
    omniboxLastQuery = q;
    try {
      const res = await search(q, 5);
      const out: chrome.omnibox.SuggestResult[] = [];
      for (const re of res.entities) {
        out.push({
          content: `entity:${re.entity.id}`,
          description: escapeXml(`${re.entity.type}: ${re.entity.name}`),
        });
      }
      for (const rn of res.notes) {
        const preview = truncate(rn.note.text.replace(/\s+/g, " "), 90);
        out.push({
          content: `note:${rn.note.id}`,
          description: escapeXml(`note · ${preview}`),
        });
      }
      suggest(out.slice(0, 5));
    } catch (e) {
      suggest([
        {
          content: `error:${q}`,
          description: escapeXml(`Cairn unreachable — ${(e as Error).message}`),
        },
      ]);
    }
  }, 180);
});

chrome.omnibox?.onInputEntered.addListener(async (text) => {
  // Cairn's UI lives in a Tauri window, not over HTTP — there's no web URL
  // to deep-link to yet. Best we can do for v0.1: re-run the search via the
  // API, surface a notification with the top hit, and copy its name to the
  // clipboard so the user can paste it into the Cairn search bar with one
  // keystroke. Once Cairn registers a `cairn://` URL scheme this collapses
  // to a single navigation.
  if (text.startsWith("entity:") || text.startsWith("note:")) {
    const summary = await fetchHitSummary(text);
    if (summary) {
      await notify(`Cairn → ${summary}`);
      try {
        // Service workers can use clipboard via OffscreenDocument; for
        // simplicity we route through the active tab.
        const tab = await getActiveTab();
        if (tab?.id)
          await chrome.scripting.executeScript({
            target: { tabId: tab.id },
            args: [summary],
            func: (s: string) => navigator.clipboard?.writeText(s),
          });
      } catch {
        /* ignore */
      }
    }
    return;
  }
  // Free text Enter → re-run search and notify the top match count.
  const q = text.startsWith("error:") ? text.slice("error:".length) : text;
  try {
    const res = await search(q, 5);
    await notify(`Cairn: ${res.entities.length} entities, ${res.notes.length} notes for "${truncate(q, 40)}"`);
  } catch (e) {
    await notify(`Cairn unreachable — ${(e as Error).message}`);
  }
});

async function fetchHitSummary(raw: string): Promise<string | null> {
  if (!raw.startsWith("entity:") && !raw.startsWith("note:")) return null;
  // Re-run the last query so we get fresh data; pull the matching hit by id.
  try {
    const res = await search(omniboxLastQuery, 10);
    const id = raw.slice(raw.indexOf(":") + 1);
    if (raw.startsWith("entity:")) {
      const hit = res.entities.find((e) => e.entity.id === id);
      return hit ? `${hit.entity.type}: ${hit.entity.name}` : null;
    }
    const hit = res.notes.find((n) => n.note.id === id);
    return hit ? `note · ${truncate(hit.note.text, 80)}` : null;
  } catch {
    return null;
  }
}

function escapeXml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

// ---------- keyboard command ----------

chrome.commands.onCommand.addListener(async (command) => {
  if (command === "open-search") {
    const tab = await getActiveTab();
    if (!tab?.id) return;
    // Open the side panel showing the search popup for the current window.
    try {
      await chrome.sidePanel.open({ tabId: tab.id });
    } catch (e) {
      // Some browsers route this to the toolbar popup instead.
      console.warn("[cairn] sidePanel.open failed:", e);
    }
  }
});

// ---------- helpers ----------

async function notify(message: string) {
  try {
    await chrome.notifications.create({
      type: "basic",
      iconUrl: chrome.runtime.getURL("icons/icon-128.png"),
      title: "Cairn",
      message,
    });
  } catch {
    // The `notifications` permission is optional — fall back silently.
    console.info("[cairn]", message);
  }
}

async function getActiveTab(): Promise<chrome.tabs.Tab | null> {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab ?? null;
}

interface PageSummary {
  text: string;
  url?: string;
  title?: string;
}

async function summarisePage(
  tabId: number,
  tab: chrome.tabs.Tab,
): Promise<PageSummary> {
  // Pull the first ~2 KB of visible text from the page; that's enough for
  // Cairn's extractor to grasp what the page is about. Heavier summarisation
  // happens server-side via Haiku.
  try {
    const [hit] = await chrome.scripting.executeScript({
      target: { tabId },
      func: () => {
        const article =
          document.querySelector("article, main") ??
          document.body;
        return (article as HTMLElement).innerText.slice(0, 2000);
      },
    });
    return {
      text: hit?.result ?? document.title,
      url: tab.url,
      title: tab.title,
    };
  } catch {
    return { text: tab.title ?? tab.url ?? "(empty)", url: tab.url, title: tab.title };
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}
