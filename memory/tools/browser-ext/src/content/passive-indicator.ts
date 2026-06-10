// Passive recall indicator (mock `oDjEW`).
//
// A tiny accent badge anchored at the composer's bottom-left. It tells the
// user that Cairn found N relevant memories for what they're typing — no
// `@cairn` summon required. Clicking it opens the picker so the user can
// review / select / inject.
//
// Pure DOM. The handle is mutable so the controller can keep updating the
// counts as the user keeps typing (we don't tear down the indicator on every
// new search — that flickers).
//
// Anchoring strategy:
//   * `position: fixed` (the slash picker uses fixed too, so the two stay in
//     visual lockstep). Re-positions on `scroll` capture + `resize`.
//   * Anchored to the composer's bottom-left using `getBoundingClientRect`,
//     offset 6px down. Falls back to the bottom-right of the viewport if the
//     composer is off-screen (e.g. mid-route-transition).
//   * Some hosts (ChatGPT in particular) sometimes briefly render a placeholder
//     composer at top:0 during a soft navigation. We hide the indicator while
//     `r.height === 0` so the badge doesn't snap to the top of the page.

import { CAIRN_NS, ensureStyles } from "./styles";

export interface IndicatorHandle {
  /** Update the visible counts and tier styling. Strong > 0 → "strong"
   *  variant; only weak > 0 → "weak" variant; both 0 → tear down. */
  setCounts(strong: number, weak: number): void;
  /** Show an error pill instead of the normal counts. Stays until next
   *  `setCounts`. */
  setError(msg: string): void;
  /** Remove from the DOM and detach listeners. */
  dispose(): void;
  /** Re-position now (used after the controller drops the picker so the
   *  indicator reclaims its slot under the composer). */
  reposition(): void;
}

export interface IndicatorOptions {
  onClick: () => void;
  onDismiss?: () => void;
}

export function mountPassiveIndicator(
  anchor: HTMLElement,
  opts: IndicatorOptions,
): IndicatorHandle {
  ensureStyles();
  const el = document.createElement("div");
  el.className = `${CAIRN_NS}-indicator`;
  el.setAttribute("role", "button");
  el.setAttribute("tabindex", "0");
  el.dataset.tier = "strong";
  el.innerHTML = `
    <span class="${CAIRN_NS}-indicator-dot"></span>
    <span class="${CAIRN_NS}-indicator-text"></span>
    <span class="${CAIRN_NS}-indicator-dismiss" role="button" aria-label="dismiss">×</span>
  `;
  document.body.appendChild(el);

  const textEl = el.querySelector(`.${CAIRN_NS}-indicator-text`) as HTMLSpanElement;
  const dismissEl = el.querySelector(`.${CAIRN_NS}-indicator-dismiss`) as HTMLSpanElement;

  function reposition() {
    // If the anchor isn't in the DOM any more, fall back to bottom-right of
    // viewport — that avoids the indicator going AWOL during SPA route
    // transitions that detach the composer for a frame.
    if (!document.contains(anchor)) {
      el.style.left = "auto";
      el.style.right = "16px";
      el.style.top = "auto";
      el.style.bottom = "16px";
      el.style.visibility = "visible";
      return;
    }
    const r = anchor.getBoundingClientRect();
    if (r.height === 0 || r.width === 0) {
      // Composer collapsed (mid-render). Hide rather than snap to (0,0).
      el.style.visibility = "hidden";
      return;
    }
    el.style.visibility = "visible";
    // ChatGPT's bottom toolbar scrolls under the message list sometimes; offset
    // the badge so it sits in the airspace just below the composer's bottom
    // edge, aligned to its left. If the badge would render off-screen at the
    // bottom (composer is at the very bottom of the viewport with no room),
    // flip it above the composer instead.
    const desiredTop = r.bottom + 6;
    const safeTop =
      desiredTop + 26 > window.innerHeight ? r.top - 30 : desiredTop;
    el.style.left = `${Math.max(8, r.left)}px`;
    el.style.top = `${Math.max(4, safeTop)}px`;
    el.style.right = "auto";
    el.style.bottom = "auto";
  }

  const onScroll = () => reposition();
  const onResize = () => reposition();
  window.addEventListener("scroll", onScroll, true);
  window.addEventListener("resize", onResize);
  // SPA hosts (ChatGPT, Claude) re-layout when the composer auto-grows. A
  // ResizeObserver on the anchor keeps the badge stuck to its bottom edge.
  const ro = new ResizeObserver(() => reposition());
  try {
    ro.observe(anchor);
  } catch {
    /* anchor may already be detached; ignored */
  }

  reposition();

  el.addEventListener("click", (ev) => {
    // Dismiss button hands off to onDismiss instead of opening the picker.
    if (ev.target instanceof Node && dismissEl.contains(ev.target)) {
      ev.stopPropagation();
      if (opts.onDismiss) opts.onDismiss();
      return;
    }
    opts.onClick();
  });
  el.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      opts.onClick();
    }
  });

  function setCounts(strong: number, weak: number) {
    if (strong > 0) {
      el.dataset.tier = "strong";
      el.style.display = "inline-flex";
      textEl.textContent = `Cairn · ${strong} relevant ${strong === 1 ? "memory" : "memories"} — click to use`;
    } else if (weak > 0) {
      el.dataset.tier = "weak";
      el.style.display = "inline-flex";
      textEl.textContent = `Cairn · ${weak} weak ${weak === 1 ? "match" : "matches"}`;
    } else {
      // Nothing useful — hide entirely.
      el.style.display = "none";
    }
    // After the text changes, the badge's intrinsic width changes too — keep
    // it anchored to the composer's left edge.
    reposition();
  }

  function setError(msg: string) {
    el.dataset.tier = "error";
    el.style.display = "inline-flex";
    textEl.textContent = `Cairn · ${msg}`;
    reposition();
  }

  function dispose() {
    window.removeEventListener("scroll", onScroll, true);
    window.removeEventListener("resize", onResize);
    try {
      ro.disconnect();
    } catch {
      /* already disconnected */
    }
    el.remove();
  }

  return { setCounts, setError, dispose, reposition };
}
