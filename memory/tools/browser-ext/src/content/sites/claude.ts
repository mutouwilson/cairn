// claude.ai adapter — Lexical-based composer + role-tagged messages.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // Claude.ai migrated its composer between several editor frameworks
  // (ProseMirror → role=textbox div → Lexical), so we try each in order
  // of specificity. The last-resort `div[contenteditable="true"]` filter
  // picks the bottom-most visible editable on the page — composer is
  // always anchored at the bottom of the viewport.
  const selectors = [
    'div[contenteditable="true"][data-lexical-editor="true"]',
    'div[contenteditable="true"][role="textbox"]',
    'div.ProseMirror[contenteditable="true"]',
    'fieldset div[contenteditable="true"]',
    'div[contenteditable="true"]',
  ];
  for (const sel of selectors) {
    const candidates = root.querySelectorAll(sel);
    let best: HTMLElement | null = null;
    let bestTop = -Infinity;
    for (const el of candidates) {
      if (!(el instanceof HTMLElement)) continue;
      if (el.offsetParent === null) continue;
      const r = el.getBoundingClientRect();
      if (r.width < 80 || r.height < 16) continue; // skip tiny inputs (search boxes)
      // Prefer the bottom-most visible editable — that's the composer.
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
  // Claude tags messages with `data-test-render-count` and a role wrapper.
  // We look for the assistant-role wrapper, then descend for its visible text.
  const groups = root.querySelectorAll(
    'div[data-testid="message-wrapper"], div[role="presentation"] div.font-claude-message, div.font-claude-message',
  );
  groups.forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const text = (node.innerText ?? "").trim();
    if (!text) return;
    const key =
      node.getAttribute("data-message-id") ??
      (node.id || `claude:${text.slice(0, 32)}`);
    out.push({ key, container: node, text, messageId: key });
  });
  return out;
}

function conversationId(): string | undefined {
  const m = location.pathname.match(/\/chat\/([0-9a-f-]+)/i);
  return m ? m[1] : undefined;
}

function roleFor(node: Element): "user" | "assistant" | "system" | null {
  // Claude wraps each turn in an element with `data-testid="user-message"`
  // or `data-testid="assistant-message"`. Some recent builds also expose a
  // `font-claude-message` wrapper without the testid — treat that as the
  // assistant fallback.
  if (node.closest('[data-testid="user-message"]')) return "user";
  if (node.closest('[data-testid="assistant-message"]')) return "assistant";
  if (node.closest('.font-claude-message')) return "assistant";
  return null;
}

export const claudeAdapter: SiteAdapter = {
  agent: "claude",
  source_host: "claude.ai",
  findComposer,
  findAssistantMessages,
  conversationId,
  roleFor,
  keySelectors: [
    { label: "composer", selector: 'div[contenteditable="true"][data-lexical-editor="true"], div[contenteditable="true"][role="textbox"], div.ProseMirror[contenteditable="true"], fieldset div[contenteditable="true"]' },
    { label: "assistant-message", selector: 'div[data-testid="message-wrapper"], div[data-testid="assistant-message"], div.font-claude-message', optional: true },
  ],
};
