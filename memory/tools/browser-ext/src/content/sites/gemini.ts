// gemini.google.com adapter — Angular-based, uses custom elements.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // Gemini wraps the editor in a custom `<rich-textarea>` with a
  // contenteditable inside.
  const inner = root.querySelector(
    'rich-textarea div.ql-editor[contenteditable="true"], div.ql-editor[contenteditable="true"]',
  );
  return inner instanceof HTMLElement ? inner : null;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  // Gemini renders each turn inside `model-response`.
  const turns = root.querySelectorAll("model-response, message-content.model-response");
  turns.forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const text = (node.innerText ?? "").trim();
    if (!text) return;
    const key = node.id || `gemini:${text.slice(0, 32)}`;
    out.push({ key, container: node, text, messageId: key });
  });
  return out;
}

function conversationId(): string | undefined {
  const m = location.pathname.match(/\/app\/([\w-]+)/);
  return m ? m[1] : undefined;
}

function roleFor(node: Element): "user" | "assistant" | "system" | null {
  // Gemini renders each turn as either a `user-query` block or a
  // `model-response` block (custom elements + matching class names).
  if (node.closest("user-query, .user-query-container, .user-query, [class*='user-query']"))
    return "user";
  if (node.closest("model-response, .model-response, message-content.model-response, [class*='model-response']"))
    return "assistant";
  return null;
}

export const geminiAdapter: SiteAdapter = {
  agent: "gemini",
  source_host: "gemini.google.com",
  findComposer,
  findAssistantMessages,
  conversationId,
  roleFor,
  keySelectors: [
    { label: "composer", selector: 'rich-textarea div.ql-editor[contenteditable="true"], div.ql-editor[contenteditable="true"]' },
    { label: "assistant-message", selector: "model-response, message-content.model-response", optional: true },
  ],
};
