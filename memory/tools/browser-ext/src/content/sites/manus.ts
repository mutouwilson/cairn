// manus.im — agent workspace adapter.
//
// Composer:    <div class="tiptap ProseMirror" contenteditable="true">
//              (single visible TipTap/ProseMirror editor on the task view).
// Assistant:   div.manus-markdown — Manus' rendered-markdown container on
//              each final assistant reply. Manus is an agent platform: a
//              single user turn often triggers several intermediate "step"
//              cards (tool calls, status updates) followed by one or more
//              `.manus-markdown` blocks containing the user-facing answer.
//              We only attach save badges to the final-text turns, because
//              the step cards are agent state, not memorable output.
// User turn:   No semantic role class; identified by *not* being inside
//              a `.manus-markdown` wrapper. `roleFor` returns "assistant"
//              for nodes inside `.manus-markdown` and "user" otherwise.
// Conv id:     /app/<id> in pathname.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // Manus' composer is the only TipTap editor on the page; the
  // `tiptap ProseMirror` class combo is the stable signature.
  const direct = root.querySelector(
    'div.tiptap.ProseMirror[contenteditable="true"]',
  );
  if (direct instanceof HTMLElement && direct.offsetParent !== null) {
    return direct;
  }
  // Fallback for class-name churn: the bottom-most visible contenteditable
  // wider than ~200px is the composer (sidebars and inline editors are
  // smaller / higher in the viewport).
  let best: HTMLElement | null = null;
  let bestTop = -Infinity;
  for (const el of root.querySelectorAll('div[contenteditable="true"]')) {
    if (!(el instanceof HTMLElement) || el.offsetParent === null) continue;
    const r = el.getBoundingClientRect();
    if (r.width < 200) continue;
    if (r.top > bestTop) {
      best = el;
      bestTop = r.top;
    }
  }
  return best;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  // `.manus-markdown` is the rendered-markdown container on each final
  // assistant reply. Skipping intermediate step cards is the whole point
  // of using this selector instead of a generic turn wrapper.
  const nodes = root.querySelectorAll("div.manus-markdown");
  nodes.forEach((node, idx) => {
    if (!(node instanceof HTMLElement)) return;
    const text = (node.innerText ?? "").trim();
    if (!text || text.length < 4) return;
    const key = structuralKey(node, idx, text);
    out.push({ key, container: node, text, messageId: key });
  });
  return out;
}

function conversationId(): string | undefined {
  const m = location.pathname.match(/\/app\/([A-Za-z0-9_-]+)/);
  return m ? m[1] : undefined;
}

function roleFor(node: Element): "user" | "assistant" | "system" | null {
  // No role class anywhere — `.manus-markdown` IS the assistant signal.
  // Everything else inside the conversation column is either the original
  // user prompt or an intermediate step card; we lump the latter under
  // "system" so downstream filtering can drop them if needed.
  if (node.closest("div.manus-markdown")) return "assistant";
  return "user";
}

function structuralKey(node: HTMLElement, idx: number, text: string): string {
  const head = text.slice(0, 24).replace(/\s+/g, " ");
  return `manus:${idx}:${head}`;
}

export const manusAdapter: SiteAdapter = {
  agent: "manus",
  source_host: "manus.im",
  findComposer,
  findAssistantMessages,
  conversationId,
  roleFor,
  keySelectors: [
    {
      label: "composer",
      selector: 'div.tiptap.ProseMirror[contenteditable="true"]',
    },
    {
      label: "assistant-message",
      selector: "div.manus-markdown",
      optional: true,
    },
  ],
};
