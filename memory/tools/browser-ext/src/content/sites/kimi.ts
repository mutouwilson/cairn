// Moonshot Kimi — kimi.com adapter.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // Kimi composer is a textarea wrapped in a chat input bar at the bottom.
  const ta = root.querySelector('textarea, div[contenteditable="true"]');
  if (ta instanceof HTMLTextAreaElement || ta instanceof HTMLElement) return ta as HTMLElement;
  return null;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  const turns = root.querySelectorAll(
    "div.message-assistant, div.kimi-message-assistant, div[data-role='assistant']",
  );
  turns.forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const text = (node.innerText ?? "").trim();
    if (!text) return;
    const key = node.id || `kimi:${text.slice(0, 32)}`;
    out.push({ key, container: node, text, messageId: key });
  });
  return out;
}

export const kimiAdapter: SiteAdapter = {
  agent: "kimi",
  source_host: "kimi.com",
  findComposer,
  findAssistantMessages,
  keySelectors: [
    { label: "composer", selector: 'textarea, div[contenteditable="true"]' },
    {
      label: "assistant-message",
      selector: "div.message-assistant, div.kimi-message-assistant, div[data-role='assistant']",
      optional: true,
    },
  ],
};
