// DeepSeek — chat.deepseek.com adapter.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  const ta = root.querySelector('textarea, div[contenteditable="true"]');
  return ta instanceof HTMLElement ? ta : null;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  const turns = root.querySelectorAll(
    'div[class*="_ai_"], div[class*="-assistant"], div[data-role="assistant"]',
  );
  turns.forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const text = (node.innerText ?? "").trim();
    if (!text || text.length < 4) return;
    const key = node.id || `ds:${text.slice(0, 32)}`;
    out.push({ key, container: node, text, messageId: key });
  });
  return out;
}

export const deepseekAdapter: SiteAdapter = {
  agent: "deepseek",
  source_host: "chat.deepseek.com",
  findComposer,
  findAssistantMessages,
  keySelectors: [
    { label: "composer", selector: 'textarea, div[contenteditable="true"]' },
    {
      label: "assistant-message",
      selector: 'div[class*="_ai_"], div[class*="-assistant"], div[data-role="assistant"]',
      optional: true,
    },
  ],
};
