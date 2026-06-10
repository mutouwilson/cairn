// chatgpt.com / chat.openai.com adapter.
//
// Composer:   #prompt-textarea (contenteditable Lexical) or textarea fallback
// Assistant:  div[data-message-author-role="assistant"]
// Conv id:    /c/<uuid> in pathname

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // ChatGPT's composer DOM has churned a lot (textarea#prompt-textarea →
  // Lexical contenteditable → ProseMirror in some builds → new "+" tool
  // chooser composer mid-2026). We try the historically-known selectors
  // first, then fall back to "the bottom-most visible editable that's
  // wide enough to be the composer" — works across redesigns.
  const direct = root.querySelector(
    '#prompt-textarea, div[contenteditable="true"][data-id="root"], textarea[data-id="prompt-textarea"], div[contenteditable="true"][data-lexical-editor="true"]',
  );
  if (direct instanceof HTMLElement && direct.offsetParent !== null) {
    return direct;
  }
  const selectors = [
    'form textarea',
    'form div[contenteditable="true"]',
    'div[contenteditable="true"]',
  ];
  for (const sel of selectors) {
    let best: HTMLElement | null = null;
    let bestTop = -Infinity;
    for (const el of root.querySelectorAll(sel)) {
      if (!(el instanceof HTMLElement)) continue;
      if (el.offsetParent === null) continue;
      const r = el.getBoundingClientRect();
      if (r.width < 120 || r.height < 16) continue; // skip search/header inputs
      // ChatGPT composer is always anchored near the bottom of the viewport.
      if (r.top > bestTop) {
        best = el;
        bestTop = r.top;
      }
    }
    if (best) return best;
  }
  return null;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  const nodes = root.querySelectorAll('div[data-message-author-role="assistant"]');
  nodes.forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const messageId =
      node.getAttribute("data-message-id") ?? node.id ?? structuralKey(node);
    out.push({
      key: messageId,
      container: node,
      text: extractMarkdown(node),
      messageId,
    });
  });
  return out;
}

function conversationId(): string | undefined {
  const m = location.pathname.match(/\/c\/([0-9a-f-]+)/i);
  return m ? m[1] : undefined;
}

/** Best-effort: grab the inner markdown text the user sees, not the raw
 *  HTML. ChatGPT renders the markdown into `.markdown` blocks. */
function extractMarkdown(node: HTMLElement): string {
  const md = node.querySelector(".markdown");
  if (md instanceof HTMLElement) return (md.innerText ?? "").trim();
  return (node.innerText ?? "").trim();
}

function structuralKey(node: HTMLElement): string {
  // Cheap-but-stable key when no id is exposed: index among siblings +
  // first 24 chars of text. Re-renders that change the text will get a
  // new key (which is fine — we'd want a fresh decoration anyway).
  const parent = node.parentElement;
  const idx = parent ? Array.from(parent.children).indexOf(node) : 0;
  const head = (node.textContent ?? "").slice(0, 24);
  return `chatgpt:${idx}:${head}`;
}

function roleFor(node: Element): "user" | "assistant" | "system" | null {
  // ChatGPT exposes `data-message-author-role="user"|"assistant"|"system"`
  // on each turn's wrapper. Walk up until we find one.
  const hit = node.closest('[data-message-author-role]');
  if (!hit) return null;
  const role = hit.getAttribute("data-message-author-role");
  if (role === "user" || role === "assistant" || role === "system") return role;
  return null;
}

export const chatgptAdapter: SiteAdapter = {
  agent: "chatgpt",
  source_host: "chatgpt.com",
  findComposer,
  findAssistantMessages,
  conversationId,
  roleFor,
  keySelectors: [
    { label: "composer", selector: '#prompt-textarea, div[contenteditable="true"][data-id="root"], textarea[data-id="prompt-textarea"], div[contenteditable="true"][data-lexical-editor="true"], form textarea, form div[contenteditable="true"]' },
    { label: "assistant-message", selector: 'div[data-message-author-role="assistant"]', optional: true },
  ],
};
