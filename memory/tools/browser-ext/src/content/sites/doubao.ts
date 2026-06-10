// 字节豆包 — www.doubao.com adapter.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // Doubao uses a contenteditable div for the composer; selectors are not
  // stable across deploys, so we go broad: any visible contenteditable
  // attached to the bottom of the viewport.
  const candidates = root.querySelectorAll('div[contenteditable="true"], textarea');
  for (const el of candidates) {
    if (!(el instanceof HTMLElement)) continue;
    if (el.offsetParent === null) continue;
    const rect = el.getBoundingClientRect();
    if (rect.bottom > window.innerHeight - 320 && rect.width > 100) {
      return el;
    }
  }
  return null;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  // Doubao tags assistant turns with various role attributes / class names
  // depending on release. Try a few likely candidates.
  const turns = root.querySelectorAll(
    "[data-testid='receive_message'], [data-testid='message-content'], div.receive-message, div[data-message-author-role='assistant']",
  );
  turns.forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const text = (node.innerText ?? "").trim();
    if (!text) return;
    const key = node.id || node.getAttribute("data-message-id") || `doubao:${text.slice(0, 32)}`;
    out.push({ key, container: node, text, messageId: key });
  });
  return out;
}

function roleFor(node: Element): "user" | "assistant" | "system" | null {
  // Doubao's DOM markers have shifted across recent builds; we match on
  // the union of attributes we've seen. This is intentionally generous —
  // null is the safe fallback when nothing matches and tagging is skipped.
  if (
    node.closest(
      "[data-testid='send_message'], [data-testid='message_send'], div.send-message, div[data-message-author-role='user']",
    )
  )
    return "user";
  if (
    node.closest(
      "[data-testid='receive_message'], [data-testid='message_receive'], div.receive-message, div[data-message-author-role='assistant']",
    )
  )
    return "assistant";
  return null;
}

export const doubaoAdapter: SiteAdapter = {
  agent: "doubao",
  source_host: "doubao.com",
  findComposer,
  findAssistantMessages,
  roleFor,
  keySelectors: [
    {
      label: "assistant-message",
      selector:
        "[data-testid='receive_message'], [data-testid='message-content'], div.receive-message, div[data-message-author-role='assistant']",
      optional: true,
    },
  ],
};
