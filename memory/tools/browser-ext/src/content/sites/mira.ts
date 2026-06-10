// mira.day — task workspace adapter.
//
// Composer:    <textarea placeholder="请输入..." data-slot="input-group-control">
//              (single visible textarea on /task and /task/<id>).
// Assistant:   div.is-assistant   ← each agent turn wrapper.
// User:        div.is-user        ← each user turn wrapper.
// Conv id:     /task/<id> in pathname.
//
// No data-message-id attribute is exposed today, so we fall back to a
// structural key (turn index within the messages container + first 24 chars
// of innerText) — re-renders with the same content stay stable.

import type { AssistantMessage, SiteAdapter } from "../types";

function findComposer(root: ParentNode): HTMLElement | null {
  // Mira ships exactly one composer textarea on the task view, tagged via
  // shadcn's `data-slot="input-group-control"` with the visible placeholder.
  const direct = root.querySelector(
    'textarea[data-slot="input-group-control"], textarea[placeholder="请输入..."]',
  );
  if (direct instanceof HTMLElement && direct.offsetParent !== null) {
    return direct;
  }
  // Fallback for placeholder/locale churn: the only sizeable visible textarea
  // anchored near the bottom of the viewport is the composer.
  let best: HTMLElement | null = null;
  let bestTop = -Infinity;
  for (const el of root.querySelectorAll("textarea")) {
    if (!(el instanceof HTMLElement) || el.offsetParent === null) continue;
    const r = el.getBoundingClientRect();
    if (r.width < 200 || r.height < 24) continue;
    if (r.top > bestTop) {
      best = el;
      bestTop = r.top;
    }
  }
  return best;
}

function findAssistantMessages(root: ParentNode): AssistantMessage[] {
  const out: AssistantMessage[] = [];
  // `is-assistant` is a real class token on the outer wrapper, not a Tailwind
  // variant suffix like `is-user:dark`, so `.is-assistant` matches cleanly.
  const nodes = root.querySelectorAll("div.is-assistant");
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
  const m = location.pathname.match(/\/task\/([A-Za-z0-9_-]+)/);
  return m ? m[1] : undefined;
}

function roleFor(node: Element): "user" | "assistant" | "system" | null {
  if (node.closest("div.is-assistant")) return "assistant";
  if (node.closest("div.is-user")) return "user";
  return null;
}

function structuralKey(node: HTMLElement, idx: number, text: string): string {
  const head = text.slice(0, 24).replace(/\s+/g, " ");
  return `mira:${idx}:${head}`;
}

export const miraAdapter: SiteAdapter = {
  agent: "mira",
  source_host: "mira.day",
  findComposer,
  findAssistantMessages,
  conversationId,
  roleFor,
  keySelectors: [
    {
      label: "composer",
      selector:
        'textarea[data-slot="input-group-control"], textarea[placeholder="请输入..."]',
    },
    {
      label: "assistant-message",
      selector: "div.is-assistant",
      optional: true,
    },
  ],
};
