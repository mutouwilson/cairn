# Cairn Browser Extension

A Manifest V3 extension that surfaces your local Cairn memory inside any
web AI chat — and lets you save AI replies back into Cairn in one click.

## Why this exists

Cairn talks MCP, which is a great protocol but only Anthropic + a handful of
allied tools (Cursor, Windsurf) speak it natively. ChatGPT / Gemini / Doubao /
Kimi / DeepSeek / 通义 / 文心 — none of them will speak MCP this year. The
extension is the bridge. It does two things:

1. **Read** — type `@cairn <query>` in the chat composer; results from your
   memory get injected as a `<cairn-context>` block above your actual
   message before you press Send.
2. **Write** — each assistant reply gets a small "Save" pill; one click pipes
   the reply into Cairn with the agent, conversation id, and URL captured as
   metadata.

Plus a right-click "Save selection to Cairn" / "Save this page" menu that
works on any page (not just chats).

Everything talks to your local Cairn at `http://127.0.0.1:7717`. Nothing
leaves your machine.

## Sites with explicit support

| Site | Slash `@cairn` | Save AI reply | Notes |
| --- | --- | --- | --- |
| `chatgpt.com` / `chat.openai.com` | ✓ | ✓ | Conversation id pulled from `/c/<uuid>` |
| `claude.ai` | ✓ | ✓ | Lexical composer; some message decorations may need adjustment as Claude updates the DOM |
| `gemini.google.com` | ✓ | ✓ | `model-response` web component |
| `www.doubao.com` | ✓ | ✓ | Generic contenteditable detection |
| `kimi.com` | ✓ | ✓ | |
| `chat.deepseek.com` | ✓ | ✓ | |
| `yiyan.baidu.com` (文心一言) | best-effort | best-effort | Uses the Doubao adapter as a stand-in |
| `tongyi.aliyun.com` | ✓ | ✓ | |

If a site you use isn't listed, the right-click menu still works; only the
in-chat injection needs a per-site adapter.

## Install (developer mode)

Requirements: Node 18+, pnpm.

```bash
cd tools/browser-ext
pnpm install
pnpm build
```

That produces `dist/`. Then in your Chromium browser:

1. Open `chrome://extensions/`
2. Toggle **Developer mode** on (top right)
3. Click **Load unpacked**
4. Choose the `tools/browser-ext/dist/` folder

The Cairn icon appears in the toolbar; open it once to verify the connection
to `http://127.0.0.1:7717` is green. If it isn't, open Cairn → **Settings →
Remote MCP bridge** and enable it.

## Configure

Right-click the toolbar icon → **Options** to change:

- **Cairn URL** — default `http://127.0.0.1:7717`. Change if you run Cairn on
  a different port or you've tunneled it.
- **Slash prefix** — defaults to `@cairn`.
- **Inject as prefix** — when on, results from your search become a
  `<cairn-context>` block prepended to your message; the agent sees them as
  context. Off: the picked entity's summary is appended after your message
  for you to edit.
- **Enabled sites** — toggle each chat host individually.

## Hotkey

`Ctrl+Shift+L` (`⌘⇧L` on macOS) opens the Cairn side panel from any tab.

## Architecture

```
tools/browser-ext/
├── manifest.config.ts          ← Manifest V3 declaration
├── vite.config.ts              ← @crxjs/vite-plugin build
├── src/
│   ├── background/index.ts     ← Service worker (context menu, hotkey)
│   ├── content/
│   │   ├── index.ts            ← Dispatcher: per-host adapter
│   │   ├── controller.ts       ← Slash command + save badge orchestration
│   │   ├── slash-picker.ts     ← Floating search result panel
│   │   ├── save-badge.ts       ← "Save to Cairn" button on AI messages
│   │   ├── styles.ts           ← Namespaced CSS injected once per page
│   │   ├── types.ts            ← SiteAdapter contract
│   │   └── sites/
│   │       ├── chatgpt.ts
│   │       ├── claude.ts
│   │       ├── gemini.ts
│   │       ├── doubao.ts
│   │       ├── kimi.ts
│   │       ├── deepseek.ts
│   │       └── tongyi.ts
│   ├── lib/
│   │   ├── cairn.ts            ← HTTP client for Cairn local API
│   │   ├── settings.ts         ← chrome.storage.sync wrapper
│   │   └── types.ts            ← Mirrors src-tauri/src/api.rs shapes
│   ├── popup/                  ← Toolbar popup (search + quick capture)
│   └── options/                ← Options page
└── public/icons/               ← 16/32/48/128 PNGs
```

## Adding a new site adapter

Create `src/content/sites/<host>.ts` exporting a `SiteAdapter`, then add it
to `src/content/sites/index.ts` and to `host_permissions` in
`manifest.config.ts`. The contract is:

```ts
export interface SiteAdapter {
  agent: string;                                 // "chatgpt" | "claude" | …
  source_host: string;                           // "chatgpt.com"
  findComposer(root: ParentNode): HTMLElement | null;
  findAssistantMessages(root: ParentNode): AssistantMessage[];
  conversationId?(): string | undefined;
}
```

Selectors are the only fragile bit — most chat SPAs change their DOM every
few weeks. Adapters return *fresh* references each call so a `MutationObserver`
in the controller keeps re-decorating after every re-render.

## Security model

The extension only talks to `127.0.0.1:7717` by default. The Cairn local API
itself has **no authentication** (the host being `localhost` is the
boundary). Don't tunnel it publicly unless you also add auth — that's a
separate hardening task tracked in the main repo.
