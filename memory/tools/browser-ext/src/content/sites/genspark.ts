// genspark.ai — agent / chat adapter.
//
// Composer:    <textarea class="search-input j-search-input ...">
//              (single visible textarea on the home page and inside an
//              /agents?id=<uuid> chat).
// Assistant:   div.conversation-statement.assistant — each agent turn
//              wrapper (the class token is real, not a Tailwind variant).
// User:        div.conversation-statement.user — symmetric.
// Conv id:     /agents?id=<uuid> — the conversation id lives in a query
//              param, not the pathname.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // GenSpark's composer is the unique `search-input` / `j-search-input`
  // textarea — also referenced under that class on the landing page.
  const direct = root.querySelector(
    "textarea.search-input, textarea.j-search-input",
  );
  if (direct instanceof HTMLElement && direct.offsetParent !== null) {
    return direct;
  }
  // Fallback: bottom-most sizeable visible textarea.
  let best: HTMLElement | null = null;
  let bestTop = -Infinity;
  for (const el of root.querySelectorAll("textarea")) {
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
  // Direct class-token match; `.assistant` IS a real class on the outer
  // wrapper here (not a Tailwind responsive variant).
  const nodes = root.querySelectorAll("div.conversation-statement.assistant");
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
  // GenSpark keeps the conv id in `?id=<uuid>`, e.g.
  // /agents?id=77361ac0-a23e-43a1-9fcb-beafb7a9601a
  const params = new URLSearchParams(location.search);
  const id = params.get("id");
  return id || undefined;
}

function roleFor(node: Element): "user" | "assistant" | "system" | null {
  if (node.closest("div.conversation-statement.assistant")) return "assistant";
  if (node.closest("div.conversation-statement.user")) return "user";
  return null;
}

function structuralKey(node: HTMLElement, idx: number, text: string): string {
  const head = text.slice(0, 24).replace(/\s+/g, " ");
  return `genspark:${idx}:${head}`;
}

export const gensparkAdapter: SiteAdapter = {
  agent: "genspark",
  source_host: "genspark.ai",
  findComposer,
  findAssistantMessages,
  conversationId,
  roleFor,
  keySelectors: [
    {
      label: "composer",
      selector: "textarea.search-input, textarea.j-search-input",
    },
    {
      label: "assistant-message",
      selector: "div.conversation-statement.assistant",
      optional: true,
    },
  ],
};
