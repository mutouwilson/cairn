import { DEFAULTS, loadSettings, saveSettings, type Settings } from "@/lib/settings";
import * as cairn from "@/lib/cairn";

const KNOWN_SITES: { host: string; label: string }[] = [
  { host: "chatgpt.com", label: "ChatGPT (chatgpt.com)" },
  { host: "chat.openai.com", label: "ChatGPT (chat.openai.com)" },
  { host: "claude.ai", label: "Claude (claude.ai)" },
  { host: "gemini.google.com", label: "Gemini" },
  { host: "www.doubao.com", label: "豆包 Doubao" },
  { host: "kimi.com", label: "Kimi" },
  { host: "chat.deepseek.com", label: "DeepSeek" },
  { host: "yiyan.baidu.com", label: "文心一言" },
  { host: "tongyi.aliyun.com", label: "通义千问" },
  { host: "www.mira.day", label: "Mira (mira.day)" },
  { host: "www.manus.im", label: "Manus (manus.im)" },
  { host: "www.genspark.ai", label: "Genspark (genspark.ai)" },
];

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T | null;

async function main() {
  const s = await loadSettings();
  ($("cairn-url") as HTMLInputElement).value = s.cairn_url;
  const tokenInput = $("cairn-token") as HTMLInputElement | null;
  if (tokenInput) tokenInput.value = s.cairn_token ?? "";
  ($("slash-prefix") as HTMLInputElement).value = s.slash_prefix;
  ($("inject-prefix") as HTMLInputElement).checked = s.inject_as_prefix;
  ($("passive-recall") as HTMLInputElement).checked = s.passive_recall;
  renderSiteList(s.enabled_sites);

  $("test-conn")?.addEventListener("click", testConnection);
  $("save")?.addEventListener("click", () => void onSave());
}

function renderSiteList(enabled: Record<string, boolean>) {
  const root = $("site-list");
  if (!root) return;
  root.innerHTML = "";
  for (const site of KNOWN_SITES) {
    const wrap = document.createElement("label");
    wrap.className = "site-row";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = enabled[site.host] ?? DEFAULTS.enabled_sites[site.host] ?? false;
    cb.dataset.site = site.host;
    const span = document.createElement("span");
    span.textContent = site.label;
    wrap.appendChild(cb);
    wrap.appendChild(span);
    root.appendChild(wrap);
  }
}

async function onSave() {
  const status = $("save-status");
  const tokenInput = $("cairn-token") as HTMLInputElement | null;
  const patch: Partial<Settings> = {
    cairn_url: ($("cairn-url") as HTMLInputElement).value.trim() || DEFAULTS.cairn_url,
    cairn_token: tokenInput ? tokenInput.value.trim() : "",
    slash_prefix: ($("slash-prefix") as HTMLInputElement).value.trim() || DEFAULTS.slash_prefix,
    inject_as_prefix: ($("inject-prefix") as HTMLInputElement).checked,
    passive_recall: ($("passive-recall") as HTMLInputElement).checked,
    enabled_sites: collectSiteToggles(),
  };
  await saveSettings(patch);
  if (status) {
    status.textContent = "✓ saved";
    setTimeout(() => (status.textContent = ""), 1500);
  }
}

function collectSiteToggles(): Record<string, boolean> {
  const result: Record<string, boolean> = {};
  const root = $("site-list");
  root?.querySelectorAll<HTMLInputElement>("input[type='checkbox'][data-site]").forEach((cb) => {
    result[cb.dataset.site!] = cb.checked;
  });
  return result;
}

async function testConnection() {
  const status = $("conn-status");
  if (!status) return;
  // Make sure we're testing the *new* URL/token the user typed even if they
  // haven't clicked Save yet.
  const newUrl = ($("cairn-url") as HTMLInputElement).value.trim() || DEFAULTS.cairn_url;
  const tokenInput = $("cairn-token") as HTMLInputElement | null;
  const newToken = tokenInput ? tokenInput.value.trim() : "";
  await saveSettings({ cairn_url: newUrl, cairn_token: newToken });
  try {
    const s = await cairn.status();
    status.textContent = s.ok ? `✓ connected · cairn v${s.version}` : "✗ unreachable";
  } catch (e) {
    status.textContent = `✗ ${(e as Error).message}`;
  }
}

main().catch((e) => console.error(e));
