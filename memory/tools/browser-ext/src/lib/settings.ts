// Thin wrapper over `chrome.storage.sync` for the few user-facing knobs we
// expose: the Cairn URL, per-site enable flags, and the slash-command
// prefix character (defaults to `@cairn`).

export interface Settings {
  cairn_url: string;
  /// Bearer token for `Authorization` header. Empty = no auth (localhost
  /// default). When Cairn is started with `CAIRN_API_TOKEN=…` the user must
  /// paste it here for the extension to talk to /api/*.
  cairn_token: string;
  enabled_sites: Record<string, boolean>;
  slash_prefix: string;
  default_search_limit: number;
  /// When `true`, the extension prepends matched memories as a `<context>`
  /// block above the user's actual message before submit. When `false`,
  /// the slash command just inserts an inline reference and lets the user
  /// keep editing.
  inject_as_prefix: boolean;
  /// When `true`, the extension runs a debounced search on every composer
  /// keystroke and shows a small indicator badge when relevant memories
  /// are found. When `false`, only the legacy `@cairn <query>` summon
  /// path opens the picker. Default ON.
  passive_recall: boolean;
}

// Cairn moved /api/* to its own port (7716) in 0.2; the legacy MCP-bundled
// port (7717) is tried as a fallback so users on a stale Cairn build still
// get a working extension out of the box.
export const FALLBACK_URLS = ["http://127.0.0.1:7716", "http://127.0.0.1:7717"];

export const DEFAULTS: Settings = {
  cairn_url: "http://127.0.0.1:7716",
  cairn_token: "",
  enabled_sites: {
    "chatgpt.com": true,
    "chat.openai.com": true,
    "claude.ai": true,
    "gemini.google.com": true,
    "www.doubao.com": true,
    "kimi.com": true,
    "chat.deepseek.com": true,
    "yiyan.baidu.com": true,
    "tongyi.aliyun.com": true,
    "mira.day": true,
    "www.mira.day": true,
    "manus.im": true,
    "www.manus.im": true,
    "genspark.ai": true,
    "www.genspark.ai": true,
  },
  slash_prefix: "@cairn",
  default_search_limit: 5,
  inject_as_prefix: true,
  passive_recall: true,
};

// Maps legacy URLs the user might have saved before /api/* moved to its own
// port. Returns the migrated URL, or the input unchanged when no migration
// is needed. Keep the table additive — never silently move a user from a
// port we recognise to another.
const LEGACY_URL_MIGRATIONS: Record<string, string> = {
  "http://127.0.0.1:7717": "http://127.0.0.1:7716",
  "http://localhost:7717": "http://localhost:7716",
};

export async function loadSettings(): Promise<Settings> {
  const raw = await chrome.storage.sync.get(DEFAULTS);
  const merged = {
    ...DEFAULTS,
    ...raw,
    enabled_sites: { ...DEFAULTS.enabled_sites, ...(raw.enabled_sites ?? {}) },
  } as Settings;
  const url = merged.cairn_url?.replace(/\/$/, "");
  const migrated = LEGACY_URL_MIGRATIONS[url];
  if (migrated) {
    // Silently update the saved value so we don't keep hitting the dead
    // port every load. One-shot; subsequent loads see the migrated URL.
    merged.cairn_url = migrated;
    void chrome.storage.sync.set({ cairn_url: migrated });
  }
  return merged;
}

export async function saveSettings(patch: Partial<Settings>): Promise<void> {
  await chrome.storage.sync.set(patch);
}

export function siteEnabled(settings: Settings, hostname: string): boolean {
  // Treat unknown hosts as disabled — content script will simply skip
  // injecting the UI.
  return Boolean(settings.enabled_sites[hostname]);
}
