"use client";
import { Suspense, useEffect, useRef, useState } from "react";
import {
  captureNote,
  onSelectionShow,
  selectionDismiss,
  selectionGetCurrent,
  selectionSaveWithNote,
} from "@/lib/tauri";
import type { SelectionEvent } from "@/lib/types";
import { cn } from "@/lib/utils";

export default function Page() {
  return (
    <Suspense fallback={null}>
      <SelectionPopoverPage />
    </Suspense>
  );
}

function SelectionPopoverPage() {
  const [event, setEvent] = useState<SelectionEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [flash, setFlash] = useState<"saved" | "error" | null>(null);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  async function refresh() {
    try {
      setEvent(await selectionGetCurrent());
    } catch {
      /* ignore — Tauri not ready yet */
    }
  }

  useEffect(() => {
    refresh();
    let off: (() => void) | null = null;
    (async () => {
      try {
        off = await onSelectionShow((payload) => {
          // Panel is reused across selections — wipe stale busy/flash so the
          // next show starts with active buttons.
          setEvent(payload);
          setBusy(false);
          setFlash(null);
          if (flashTimer.current) {
            clearTimeout(flashTimer.current);
            flashTimer.current = null;
          }
        });
      } catch {
        /* not running inside Tauri */
      }
    })();
    return () => {
      off?.();
      if (flashTimer.current) clearTimeout(flashTimer.current);
    };
  }, []);

  function showFlash(kind: "saved" | "error") {
    setFlash(kind);
    if (flashTimer.current) clearTimeout(flashTimer.current);
    flashTimer.current = setTimeout(() => setFlash(null), 900);
  }

  async function save() {
    if (!event || busy) return;
    setBusy(true);
    try {
      const source = event.source_bundle
        ? `selection:${event.source_bundle}`
        : "selection";
      await captureNote(event.text, source);
      showFlash("saved");
      setTimeout(() => {
        void selectionDismiss();
      }, 340);
    } catch (e) {
      console.error(e);
      showFlash("error");
    } finally {
      setBusy(false);
    }
  }

  async function saveWithNote() {
    if (!event || busy) return;
    setBusy(true);
    try {
      await selectionSaveWithNote();
    } catch (e) {
      console.error(e);
      showFlash("error");
    } finally {
      // The IPC dismisses the popover + opens quick-capture in the happy path,
      // but the panel is reused, so always release busy here.
      setBusy(false);
    }
  }

  async function dismiss() {
    try {
      await selectionDismiss();
    } catch {
      /* ignore */
    }
  }

  if (!event) {
    return <div className="h-screen w-screen bg-transparent" />;
  }

  const sourceLabel = event.source_app_name ?? event.source_bundle ?? "selection";

  return (
    <div
      className="h-screen w-screen flex items-center justify-center bg-transparent select-none"
      onKeyDown={(e) => {
        if (e.key === "Escape") dismiss();
      }}
    >
      <div
        className={cn(
          "flex items-center gap-0.5 h-[38px] pl-1.5 pr-1 rounded-full",
          "bg-white/90 backdrop-blur-xl border border-ink-200",
          "shadow-[0_8px_24px_-4px_rgba(0,0,0,0.14),0_2px_6px_-1px_rgba(0,0,0,0.06)]",
        )}
        style={{
          WebkitUserSelect: "none",
        }}
      >
        <PillAction
          icon={
            flash === "saved" ? (
              <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round">
                <path d="M3.5 8.5l3 3 6-6.5" />
              </svg>
            ) : (
              <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
                <path d="M3.5 2.5h9v11l-4.5-3.2L3.5 13.5z" />
              </svg>
            )
          }
          label={flash === "saved" ? "Saved" : "Save"}
          title={`Save selection from ${sourceLabel}`}
          onClick={save}
          disabled={busy && flash !== "saved"}
          intent={flash === "saved" ? "success" : flash === "error" ? "error" : "primary"}
        />
        <Divider />
        <PillAction
          icon={
            <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
              <path d="M3 13l3-1 7-7-2-2-7 7-1 3z" />
              <path d="M10 4l2 2" />
            </svg>
          }
          label="Note"
          title="Save with a note (opens quick-capture)"
          onClick={saveWithNote}
          disabled={busy}
        />
        <button
          type="button"
          onClick={dismiss}
          title="Dismiss (Esc)"
          className={cn(
            "ml-1 h-7 w-7 grid place-items-center rounded-full",
            "text-ink-400 hover:text-ink-700 hover:bg-ink-100",
            "transition-colors",
          )}
        >
          <svg viewBox="0 0 16 16" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </div>
    </div>
  );
}

function Divider() {
  return (
    <span className="h-4 w-px bg-ink-200 mx-0.5" aria-hidden />
  );
}

function PillAction({
  icon,
  label,
  title,
  onClick,
  disabled,
  intent = "default",
}: {
  icon: React.ReactNode;
  label: string;
  title: string;
  onClick: () => void;
  disabled?: boolean;
  intent?: "default" | "primary" | "success" | "error";
}) {
  const base =
    "h-[30px] inline-flex items-center gap-1.5 px-2.5 rounded-full text-[12px] font-medium " +
    "transition-colors disabled:opacity-40 disabled:cursor-not-allowed";
  const tone =
    intent === "success"
      ? "text-accent-fg bg-accent-dim"
      : intent === "error"
        ? "text-danger bg-danger-dim"
        : intent === "primary"
          ? "text-ink-900 hover:bg-ink-100"
          : "text-ink-700 hover:bg-ink-100";
  return (
    <button type="button" title={title} onClick={onClick} disabled={disabled} className={cn(base, tone)}>
      <span className="opacity-90">{icon}</span>
      <span>{label}</span>
    </button>
  );
}
