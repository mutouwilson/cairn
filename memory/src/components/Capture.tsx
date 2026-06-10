"use client";
import { useEffect, useRef, useState } from "react";
import { captureNote } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { BtnPrimary, Kbd } from "@/components/ui";

export function Capture({ onCaptured }: { onCaptured?: (id: string) => void }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const [focused, setFocused] = useState(false);
  const ref = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    ref.current?.focus();
  }, []);

  async function submit() {
    if (!text.trim() || busy) return;
    setBusy(true);
    setHint(null);
    try {
      const { note_id } = await captureNote(text, "manual");
      setText("");
      onCaptured?.(note_id);
      setHint("captured · extracting…");
      setTimeout(() => setHint(null), 1800);
    } catch (e) {
      setHint(`error: ${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className={cn(
        "relative rounded-xl border bg-white transition-all",
        focused
          ? "border-ink-300 shadow-[0_8px_28px_-16px_rgba(0,0,0,0.18)]"
          : "border-ink-200",
      )}
    >
      <textarea
        ref={ref}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            submit();
          }
        }}
        placeholder="What's on your mind?"
        rows={4}
        className={cn(
          "block w-full bg-transparent px-4 pt-4 pb-2 text-sm leading-relaxed",
          "focus:outline-none resize-y min-h-[120px]",
          "placeholder:text-ink-400",
        )}
      />
      <div className="flex items-center justify-between gap-3 px-3 pb-3 pt-1 border-t border-ink-100">
        <div className="flex items-center gap-1.5 text-2xs text-ink-500 pl-1">
          {hint ? (
            <span className={hint.startsWith("error") ? "text-danger" : "text-accent"}>
              {hint}
            </span>
          ) : (
            <>
              <Kbd>⌘</Kbd>
              <Kbd>↵</Kbd>
              <span>save</span>
              <span className="text-ink-300 px-1">·</span>
              <span>local · extracted by Claude</span>
            </>
          )}
        </div>
        <BtnPrimary onClick={submit} disabled={busy || !text.trim()}>
          {busy ? "saving…" : "Save"}
        </BtnPrimary>
      </div>
    </div>
  );
}
