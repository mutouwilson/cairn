import * as cairn from "@/lib/cairn";
import { loadSettings } from "@/lib/settings";
import { diagnoseError, type Diagnosis } from "@/lib/diagnostics";

const $ = <T extends Element>(id: string) =>
  document.getElementById(id) as T | null;

async function main() {
  const settings = await loadSettings();
  hookCapture();
  hookFooter(settings.cairn_url);
  pingStatus();
  void renderCanaryWarnings();
}

function hookCapture() {
  const ta = $("capture-text") as HTMLTextAreaElement | null;
  const btn = $("capture-btn") as HTMLButtonElement | null;
  const hint = $("capture-hint");
  if (!ta || !btn || !hint) return;

  async function submit() {
    if (!ta) return;
    const text = ta.value.trim();
    if (!text) return;
    btn!.disabled = true;
    hint!.textContent = "saving…";
    try {
      const tab = await activeTab();
      await cairn.capture(text, "browser-popup", {
        url: tab?.url,
        title: tab?.title,
        agent: "browser",
      });
      ta!.value = "";
      hint!.textContent = "✓ saved";
    } catch (e) {
      hint!.textContent = `error: ${(e as Error).message}`;
    } finally {
      btn!.disabled = false;
      setTimeout(() => {
        if (hint && hint.textContent && hint.textContent.startsWith("✓")) {
          hint.textContent = "⌘+Enter to save · stored locally";
        }
      }, 1500);
    }
  }

  btn.addEventListener("click", submit);
  ta.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
      ev.preventDefault();
      void submit();
    }
  });
  ta.focus();
}

function hookFooter(_baseUrl: string) {
  $("open-options")?.addEventListener("click", () => {
    void chrome.runtime.openOptionsPage();
  });
  $("open-cairn")?.addEventListener("click", (ev) => {
    ev.preventDefault();
    // Cairn.app doesn't expose a deep-link URL scheme yet; open the local
    // dev server / static export so the user can browse their memory.
    void chrome.tabs.create({ url: "http://127.0.0.1:7717/health" });
  });
}

async function pingStatus() {
  const el = $("conn-status");
  if (!el) return;
  const start = performance.now();
  try {
    const s = await cairn.status();
    el.textContent = s.ok ? `connected · v${s.version}` : "unreachable";
    clearDiagnosis();
  } catch (e) {
    const elapsed = performance.now() - start;
    const diagnosis = diagnoseError(e as Error, elapsed >= 3000);
    el.textContent = `cairn unreachable · ${diagnosis.kind}`;
    renderDiagnosis(diagnosis);
  }
}

function renderDiagnosis(d: Diagnosis) {
  // Mount a single block under the footer with the message + an action
  // button. Re-uses the popup's existing .foot link styling for visual
  // consistency — design refresh is tracked separately.
  let host = $("diagnosis-host") as HTMLDivElement | null;
  if (!host) {
    host = document.createElement("div");
    host.id = "diagnosis-host";
    host.className = "diagnosis";
    const foot = $("conn-status")?.closest("footer");
    foot?.parentElement?.insertBefore(host, foot);
  }
  host.innerHTML = "";

  const msg = document.createElement("div");
  msg.className = "diagnosis-msg";
  msg.textContent = d.message;
  host.appendChild(msg);

  if (d.action.intent !== "none") {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "diagnosis-action";
    btn.textContent = d.action.label;
    btn.addEventListener("click", () => runActionIntent(d.action.intent));
    host.appendChild(btn);
  }
}

function clearDiagnosis() {
  const host = $("diagnosis-host");
  if (host && host.parentElement) host.parentElement.removeChild(host);
}

function runActionIntent(intent: Diagnosis["action"]["intent"]) {
  switch (intent) {
    case "open-options":
      void chrome.runtime.openOptionsPage();
      return;
    case "open-cairn":
      // Open the local health endpoint — if Cairn is actually running but
      // unreachable for some other reason, this confirms it; if it isn't,
      // the browser's connection-refused page is at least an honest tell.
      // (A `cairn://` deep link would be cleaner, but the app doesn't
      // register one yet.)
      void chrome.tabs.create({ url: "http://127.0.0.1:7716/health" });
      return;
    case "upgrade-cairn":
      void chrome.tabs.create({ url: "https://github.com/anthropics/cairn/releases" });
      return;
    case "open-help":
      void chrome.tabs.create({
        url: "https://github.com/anthropics/cairn#troubleshooting",
      });
      return;
    case "none":
      return;
  }
}

interface CanaryEntry {
  missing?: boolean;
  at?: number;
  selector?: string;
  label?: string;
  host?: string;
}

// Show a canary warning only if it was recorded in the last 7 days (older
// entries are likely stale — host moved DOM around and we never revisited).
const CANARY_STALE_MS = 7 * 24 * 60 * 60 * 1000;

async function renderCanaryWarnings() {
  const all = await chrome.storage.local.get(null);
  const now = Date.now();
  // Only surface warnings for the host of the currently-active tab — a
  // stale canary from a site the user hasn't opened recently shouldn't
  // bleed into a different chat's popup.
  let activeHost: string | null = null;
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (tab?.url) activeHost = new URL(tab.url).hostname;
  } catch {
    /* ignore — non-http URLs throw on URL parsing */
  }
  const fresh: Array<{ key: string; entry: CanaryEntry }> = [];
  for (const [key, value] of Object.entries(all)) {
    if (!key.startsWith("canary:")) continue;
    const entry = value as CanaryEntry;
    if (!entry?.missing) continue;
    if (typeof entry.at !== "number") continue;
    if (now - entry.at > CANARY_STALE_MS) continue;
    if (activeHost && entry.host && entry.host !== activeHost) continue;
    fresh.push({ key, entry });
  }
  if (fresh.length === 0) return;

  // Mount under the footer (or at the bottom of body if footer's absent).
  let host = $("canary-host") as HTMLDivElement | null;
  if (!host) {
    host = document.createElement("div");
    host.id = "canary-host";
    host.className = "canary";
    document.body.appendChild(host);
  }
  host.innerHTML = "";
  // Aggregate by host so we don't repeat the same site label multiple times.
  const byHost = new Map<string, CanaryEntry[]>();
  for (const { entry } of fresh) {
    if (!entry.host) continue;
    const list = byHost.get(entry.host) ?? [];
    list.push(entry);
    byHost.set(entry.host, list);
  }
  for (const [hostname, entries] of byHost) {
    const line = document.createElement("div");
    line.className = "canary-line";
    const labels = entries.map((e) => e.label ?? "selector").join(", ");
    line.textContent = `! ${hostname} ${labels} missing — slash command may not work. Please report.`;
    host.appendChild(line);
  }
  const report = document.createElement("button");
  report.type = "button";
  report.className = "canary-report";
  report.textContent = "copy report";
  report.addEventListener("click", () => {
    const blob = JSON.stringify(
      { generated_at: now, entries: fresh.map((f) => ({ key: f.key, ...f.entry })) },
      null,
      2,
    );
    void navigator.clipboard.writeText(blob).then(() => {
      report.textContent = "copied";
      setTimeout(() => (report.textContent = "copy report"), 1500);
    });
  });
  host.appendChild(report);
}

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab ?? null;
}

main().catch((e) => console.error(e));
