// 通义千问 — tongyi.aliyun.com adapter.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  const ta = root.querySelector('textarea, div[contenteditable="true"]');
  return ta instanceof HTMLElement ? ta : null;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  const turns = root.querySelectorAll(
    'div[class*="answer-content"], div[class*="bot-content"], div[data-role="assistant"]',
  );
  turns.forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const text = (node.innerText ?? "").trim();
    if (!text || text.length < 4) return;
    const key = node.id || `tongyi:${text.slice(0, 32)}`;
    out.push({ key, container: node, text, messageId: key });
  });
  return out;
}

export const tongyiAdapter: SiteAdapter = {
  agent: "tongyi",
  source_host: "tongyi.aliyun.com",
  findComposer,
  findAssistantMessages,
  keySelectors: [
    { label: "composer", selector: 'textarea, div[contenteditable="true"]' },
    {
      label: "assistant-message",
      selector: 'div[class*="answer-content"], div[class*="bot-content"], div[data-role="assistant"]',
      optional: true,
    },
  ],
};
