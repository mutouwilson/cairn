// Shared shape for all per-site adapters. Each adapter knows three things:
//
//   1. How to FIND the composer (textarea/contenteditable) the user types in.
//   2. How to LIST the assistant messages currently rendered, so we can attach
//      a "save to Cairn" button to each one.
//   3. The agent label we record when saving (`agent: "chatgpt"`,
//      `"claude"`, …) so the timeline shows where each note came from.
//
// Adapters return *fresh* DOM references each call — chat SPAs re-render
// often, so we never cache nodes across observations.

export interface SiteAdapter {
  /** Short label stored in the captured note's `agent` metadata. */
  agent: string;

  /** Bundle id–like host id for the source string (e.g. "chatgpt.com"). */
  source_host: string;

  /**
   * Locate the active composer element. Returns either an `HTMLTextAreaElement`
   * or a `contenteditable` `HTMLElement`. We treat both uniformly via
   * `setComposerText` below.
   */
  findComposer(root: ParentNode): HTMLElement | null;

  /**
   * Locate all rendered assistant messages — used to attach the save button
   * + to pull text out when the user clicks it.
   *
   * Each item must include a stable key so we don't decorate the same node
   * twice across MutationObserver firings. The key can be the DOM node's
   * `data-id` / `id` / structural hash — adapters compute whatever's stable.
   */
  findAssistantMessages(root: ParentNode): AssistantMessage[];

  /**
   * Best-effort attempt to read the conversation id from the URL or DOM,
   * so the saved note links back to where it came from. Optional; return
   * `undefined` when unknown.
   */
  conversationId?(): string | undefined;

  /**
   * Classify whether `node` is part of a user prompt, an assistant reply, or
   * a system block. Used by the save flow to tag the captured note's source
   * (e.g. `chatgpt:assistant` vs `chatgpt:user`) so downstream retrieval can
   * filter by role. Adapters that can't tell may omit this — the save flow
   * then falls back to the plain `source_host` string.
   *
   * Implementations should walk up the DOM tree from `node` and look at the
   * containing turn's role markers; they should NOT rely on `node` itself
   * having the marker.
   */
  roleFor?(node: Element): "user" | "assistant" | "system" | null;

  /**
   * The selectors this adapter relies on at boot. Used by the canary check
   * (see `controller.ts`) to detect when a site DOM change has broken us.
   *
   * `optional: true` means the selector is expected to be missing in some
   * normal states — e.g. assistant-message selectors on a brand-new chat
   * with no assistant replies yet. The canary won't warn for optional
   * selectors; it just records their state for later inspection.
   */
  keySelectors?: ReadonlyArray<{
    label: string;
    selector: string;
    optional?: boolean;
  }>;
}

export interface AssistantMessage {
  /** Stable per-message id (used to avoid duplicate decorations). */
  key: string;
  /** Container we attach the save button into. */
  container: HTMLElement;
  /** Raw text content of the assistant message. */
  text: string;
  /** Optional per-message id Cairn can store as metadata. */
  messageId?: string;
}

/** Read whatever the user has typed in the composer. */
export function getComposerText(el: HTMLElement): string {
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) {
    return el.value;
  }
  return el.innerText ?? el.textContent ?? "";
}

/** Replace the composer's text. Triggers the input events the host SPA
 *  listens to so React/Vue state syncs.
 */
export function setComposerText(el: HTMLElement, text: string): void {
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) {
    const setter = Object.getOwnPropertyDescriptor(
      el instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype,
      "value",
    )?.set;
    setter?.call(el, text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
    return;
  }
  // contenteditable case — typical of Claude, ChatGPT's Lexical editor, etc.
  el.focus();
  // `execCommand` is deprecated but is still the only reliable way to drive
  // contenteditable through the framework's onInput handlers (Lexical,
  // ProseMirror, Slate) without breaking their model.
  document.execCommand("selectAll", false);
  document.execCommand("insertText", false, text);
}
