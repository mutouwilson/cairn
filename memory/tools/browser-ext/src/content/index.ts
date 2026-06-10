// Top-level content-script dispatcher.
//
// Each chat site has its own quirky DOM. We keep selectors / behaviour in
// per-site adapters under `./sites/*.ts`. This file picks the right one
// for `location.hostname`, then asks the adapter to mount the slash-command
// + save-reply UI.

import { loadSettings, siteEnabled } from "@/lib/settings";
import { adapterForHost } from "./sites";
import { mountController } from "./controller";
import { showSaveToast } from "./save-toast";

interface ToastMessage {
  type: "cairn-saved-toast";
  text: string;
  noteId?: string;
  intent?: "saved" | "error";
}

// Background sends `cairn-saved-toast` after the context-menu save / page
// save resolves. Each open tab listens — the toast lands wherever the user
// is currently looking.
chrome.runtime.onMessage.addListener((msg: ToastMessage) => {
  if (msg?.type !== "cairn-saved-toast") return false;
  showSaveToast(msg.text, { noteId: msg.noteId, intent: msg.intent });
  return false;
});

/** Keep the MV3 service worker warm while a chat tab is open. The port
 *  presence is what matters; periodic pings cover the rare case where the
 *  browser disconnects long-idle ports. Reconnects automatically on drop. */
function openKeepalivePort() {
  let port: chrome.runtime.Port | null = null;
  let pingTimer: ReturnType<typeof setInterval> | null = null;

  const connect = () => {
    try {
      port = chrome.runtime.connect({ name: "cairn-keepalive" });
    } catch {
      // Worker mid-restart — back off then retry.
      setTimeout(connect, 1000);
      return;
    }
    port.onDisconnect.addListener(() => {
      // Reading lastError suppresses Chrome's noisy console warning.
      void chrome.runtime.lastError;
      port = null;
      if (pingTimer) {
        clearInterval(pingTimer);
        pingTimer = null;
      }
      // Reconnect on the next animation frame — the worker will spin back up.
      setTimeout(connect, 1000);
    });
    if (pingTimer) clearInterval(pingTimer);
    pingTimer = setInterval(() => {
      try {
        port?.postMessage({ t: "ping" });
      } catch {
        // Channel torn down between checks — onDisconnect will fire and the
        // reconnect path kicks in.
      }
    }, 20_000);
  };
  connect();
}

async function init() {
  const settings = await loadSettings();
  const host = location.hostname;
  if (!siteEnabled(settings, host)) return;

  const adapter = adapterForHost(host);
  if (!adapter) return;

  // Pin the service worker for the lifetime of this tab so cold starts
  // don't bite the first `@cairn` keystroke after an idle period.
  openKeepalivePort();

  // Defer to give the chat app's SPA time to render its initial composer.
  if (document.readyState === "complete") {
    mountController(adapter, settings);
  } else {
    window.addEventListener("load", () => mountController(adapter, settings));
  }
}

init().catch((e) => console.warn("[cairn] init failed:", e));
