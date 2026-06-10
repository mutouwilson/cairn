"use client";
import { Suspense, useEffect, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import { captureNote } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { BtnPrimary, Kbd } from "@/components/ui";

export default function Page() {
  return (
    <Suspense fallback={null}>
      <QuickCapturePage />
    </Suspense>
  );
}

function QuickCapturePage() {
  const params = useSearchParams();
  const prefill = params.get("prefill") ?? "";
  const [text, setText] = useState(prefill);
  const [busy, setBusy] = useState(false);
  const [hint, setHint] = useState<string | null>(
    prefill ? "from clipboard" : null,
  );
  const ref = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    ref.current?.focus();
    if (prefill && ref.current) {
      ref.current.setSelectionRange(prefill.length, prefill.length);
    }
  }, [prefill]);

  async function closeWindow() {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    try {
      const mod = await import("@tauri-apps/api/webviewWindow");
      await mod.getCurrentWebviewWindow().close();
    } catch {
      /* ignore */
    }
  }

  async function submit() {
    if (!text.trim() || busy) return;
    setBusy(true);
    setHint("saving…");
    try {
      await captureNote(text, "hotkey");
      setText("");
      setHint("saved");
      setTimeout(() => closeWindow(), 220);
    } catch (e) {
      setHint(`error: ${(e as Error).message}`);
      setBusy(false);
    }
  }

  return (
    <main
      className="min-h-screen flex flex-col p-3 bg-ink-50"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          closeWindow();
        }
      }}
    >
      <div className="flex-1 rounded-xl border border-ink-200 bg-white shadow-sm flex flex-col overflow-hidden">
        <textarea
          ref={ref}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              submit();
            }
          }}
          placeholder="quick capture…"
          className={cn(
            "flex-1 w-full bg-transparent p-3 text-sm leading-relaxed",
            "focus:outline-none resize-none placeholder:text-ink-400",
          )}
        />
        <div className="flex items-center justify-between gap-2 px-3 py-2 border-t border-ink-100 text-2xs">
          <div className="flex items-center gap-1.5 text-ink-500">
            {hint ? (
              <span className={hint === "saved" ? "text-accent" : hint.startsWith("error") ? "text-danger" : "text-ink-500"}>
                {hint === "saved" && "✓ "}
                {hint}
              </span>
            ) : (
              <>
                <Kbd>⌘</Kbd>
                <Kbd>↵</Kbd>
                <span>save</span>
                <span className="text-ink-300 px-1">·</span>
                <Kbd>esc</Kbd>
                <span>close</span>
              </>
            )}
          </div>
          <BtnPrimary onClick={submit} disabled={busy || !text.trim()}>
            {busy ? "saving…" : "Save"}
          </BtnPrimary>
        </div>
      </div>
    </main>
  );
}
