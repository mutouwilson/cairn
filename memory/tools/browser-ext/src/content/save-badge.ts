// Tiny "save to Cairn" pill we attach to each assistant message. Single
// button with three visible states: idle → saving → saved/error.

import type { AssistantMessage, SiteAdapter } from "./types";
import { ensureStyles, CAIRN_NS } from "./styles";

export function mountSaveBadge(
  message: AssistantMessage,
  adapter: SiteAdapter,
  onSave: () => Promise<void>,
) {
  ensureStyles();
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = `${CAIRN_NS}-save-btn`;
  btn.title = `Save this ${adapter.agent} reply to Cairn`;
  btn.innerHTML = `${ICON_BOOKMARK}<span>Save</span>`;

  let state: "idle" | "saving" | "saved" | "error" = "idle";
  const update = () => {
    btn.dataset.state = state;
    switch (state) {
      case "idle":
        btn.innerHTML = `${ICON_BOOKMARK}<span>Save</span>`;
        return;
      case "saving":
        btn.innerHTML = `${ICON_SPIN}<span>Saving</span>`;
        return;
      case "saved":
        btn.innerHTML = `${ICON_CHECK}<span>Saved</span>`;
        return;
      case "error":
        btn.innerHTML = `${ICON_X}<span>Failed</span>`;
        return;
    }
  };

  btn.addEventListener("click", async (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    if (state === "saving") return;
    state = "saving";
    update();
    try {
      await onSave();
      state = "saved";
    } catch {
      state = "error";
    }
    update();
    setTimeout(() => {
      state = "idle";
      update();
    }, 2000);
  });

  // We append inside the message container so the badge inherits the
  // assistant message's natural flow. Sites that lay messages out flex/grid
  // may want to do something smarter — for that, an adapter can override
  // by mounting the button itself before the controller gets a chance.
  message.container.appendChild(btn);
  update();
}

const ICON_BOOKMARK =
  `<svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3.5 2.5h9v11l-4.5-3.2L3.5 13.5z"/></svg>`;
const ICON_CHECK =
  `<svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3.5 8.5l3 3 6-6.5"/></svg>`;
const ICON_X =
  `<svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8"/></svg>`;
const ICON_SPIN =
  `<svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true" class="${CAIRN_NS}-spin"><path d="M8 1.5a6.5 6.5 0 1 1-6.5 6.5"/></svg>`;
