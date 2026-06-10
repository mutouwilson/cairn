import { defineManifest } from "@crxjs/vite-plugin";

// Sites we know how to enhance with the slash-command + save-reply UI.
// Each entry maps a content script bundle to its host pattern.
const CHAT_HOSTS = [
  "https://chatgpt.com/*",
  "https://www.chatgpt.com/*",
  "https://chat.openai.com/*",
  "https://claude.ai/*",
  "https://gemini.google.com/*",
  "https://www.doubao.com/*",
  "https://kimi.com/*",
  "https://www.kimi.com/*",
  "https://chat.deepseek.com/*",
  "https://yiyan.baidu.com/*",
  "https://tongyi.aliyun.com/*",
  "https://mira.day/*",
  "https://www.mira.day/*",
  "https://manus.im/*",
  "https://www.manus.im/*",
  "https://genspark.ai/*",
  "https://www.genspark.ai/*",
];

export default defineManifest({
  manifest_version: 3,
  name: "Cairn — Personal Memory",
  description:
    "Read & write your personal memory from any web AI chat. Backed by your local Cairn app.",
  version: "0.1.0",

  permissions: ["storage", "contextMenus", "activeTab", "scripting", "tabs", "alarms"],
  host_permissions: [
    "http://127.0.0.1:7716/*",
    "http://localhost:7716/*",
    "http://127.0.0.1:7717/*",
    "http://localhost:7717/*",
    ...CHAT_HOSTS,
  ],

  // Reachable from the URL bar — type `cairn <query>` to search memory
  // without opening the popup. Suggestions hit the local Cairn HTTP API.
  omnibox: { keyword: "cairn" },

  background: {
    service_worker: "src/background/index.ts",
    type: "module",
  },

  action: {
    default_popup: "src/popup/index.html",
    default_title: "Cairn",
  },

  options_ui: {
    page: "src/options/index.html",
    open_in_tab: true,
  },

  side_panel: {
    default_path: "src/popup/index.html",
  },

  content_scripts: [
    {
      matches: CHAT_HOSTS,
      js: ["src/content/index.ts"],
      run_at: "document_idle",
      all_frames: false,
    },
  ],

  icons: {
    "16": "icons/icon-16.png",
    "32": "icons/icon-32.png",
    "48": "icons/icon-48.png",
    "128": "icons/icon-128.png",
  },

  commands: {
    "open-search": {
      suggested_key: { default: "Ctrl+Shift+L", mac: "Command+Shift+L" },
      description: "Open Cairn search popup",
    },
  },
});
