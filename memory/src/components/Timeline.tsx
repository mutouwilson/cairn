"use client";
import { useEffect, useState } from "react";
import {
  deleteNote,
  listNotes,
  onNoteCaptured,
  onNoteProcessed,
  reprocessNote,
  updateNote,
} from "@/lib/tauri";
import type { Note } from "@/lib/types";
import { useT } from "@/lib/i18n";
import { cn, relativeTime } from "@/lib/utils";
import { StatusPill } from "@/components/ui";

/** Imports + long-form notes can run hundreds of lines; default-rendering
 *  them at full height drowns the rest of the timeline. We collapse anything
 *  past ~6 visible lines or ~280 chars and let the user expand. The
 *  threshold is generous on purpose — short notes stay un-clamped so we
 *  don't add a "Show more" button for two-liners. */
const CLAMP_LINES = 6;
const CLAMP_MIN_LINES = 7; // strictly > CLAMP_LINES so the toggle is meaningful
const CLAMP_MIN_CHARS = 280;

export function Timeline({ refreshKey }: { refreshKey?: number }) {
  const [notes, setNotes] = useState<Note[]>([]);
  const [loading, setLoading] = useState(true);
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);

  async function refresh() {
    try {
      const ns = await listNotes(50);
      setNotes(ns);
    } catch {
      /* outside tauri: ignore */
    }
    setLoading(false);
  }

  useEffect(() => {
    refresh();
    let unlistenCap: (() => void) | undefined;
    let unlistenProc: (() => void) | undefined;
    onNoteCaptured(() => refresh()).then((fn) => (unlistenCap = fn));
    onNoteProcessed(() => refresh()).then((fn) => (unlistenProc = fn));
    return () => {
      unlistenCap?.();
      unlistenProc?.();
    };
  }, []);

  useEffect(() => {
    if (refreshKey !== undefined) refresh();
  }, [refreshKey]);

  if (loading) return <p className="text-sm text-ink-500">loading…</p>;
  if (notes.length === 0)
    return (
      <p className="text-sm text-ink-500">
        no notes yet. capture your first thought above.
      </p>
    );

  async function onDelete(id: string) {
    const prev = notes;
    setNotes(notes.filter((n) => n.id !== id));
    setConfirmId(null);
    try {
      await deleteNote(id);
    } catch (e) {
      console.error("deleteNote failed:", e);
      setNotes(prev);
    }
  }

  async function onRetry(id: string) {
    setNotes((prev) =>
      prev.map((n) =>
        n.id === id
          ? { ...n, processing_status: "pending", extraction_error: null }
          : n,
      ),
    );
    try {
      await reprocessNote(id);
    } catch (e) {
      console.error("reprocessNote failed:", e);
      refresh();
    }
  }

  async function onSaveEdit(id: string, text: string) {
    // Optimistically reflect the new text + "extracting" state, since the
    // backend re-extracts after the write and the existing onNoteProcessed
    // event will repaint the row when it finishes.
    setNotes((prev) =>
      prev.map((n) =>
        n.id === id
          ? {
              ...n,
              text,
              processing_status: "pending",
              extraction_error: null,
            }
          : n,
      ),
    );
    setEditingId(null);
    try {
      await updateNote({ id, text });
    } catch (e) {
      console.error("updateNote failed:", e);
      refresh();
    }
  }

  return (
    <ul className="space-y-2">
      {notes.map((n) => {
        const src = parseSource(n.source);
        const isEditing = editingId === n.id;
        return (
          <li
            key={n.id}
            className={cn(
              "group relative rounded-lg border bg-white px-4 py-3 transition-colors",
              isEditing
                ? "border-accent border-l-4 bg-ink-100/30"
                : "border-ink-200 hover:border-ink-300",
            )}
          >
            <div className="flex items-center justify-between gap-3 mb-1.5">
              <div className="flex items-center gap-1.5 text-2xs text-ink-500 min-w-0">
                <SourceBadge kind={src.kind} label={src.label} />
                <span className="text-ink-300">·</span>
                <span>{relativeTime(n.created_at)}</span>
              </div>
              <div className="flex items-center gap-1.5">
                {isEditing ? (
                  <StatusPill tone="accent" dot>
                    editing
                  </StatusPill>
                ) : (
                  <>
                    <Status status={n.processing_status} error={n.extraction_error} />
                    {confirmId === n.id ? (
                      <>
                        <button
                          type="button"
                          onClick={() => void onDelete(n.id)}
                          className="rounded-full bg-danger text-white px-2.5 h-5 text-2xs font-medium"
                        >
                          confirm
                        </button>
                        <button
                          type="button"
                          onClick={() => setConfirmId(null)}
                          className="rounded-full border border-ink-200 px-2 h-5 text-2xs"
                        >
                          cancel
                        </button>
                      </>
                    ) : (
                      <>
                        {n.processing_status !== "pending" && (
                          <button
                            type="button"
                            onClick={() => void onRetry(n.id)}
                            title="Re-extract"
                            className="rounded text-ink-400 hover:text-ink-900 hover:bg-ink-100 h-5 w-5 grid place-items-center opacity-0 group-hover:opacity-100 transition-opacity"
                          >
                            <svg viewBox="0 0 16 16" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
                              <path d="M13 8a5 5 0 1 1-1.5-3.5M13 3v3h-3" />
                            </svg>
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() => setEditingId(n.id)}
                          title="Edit note"
                          className="rounded text-ink-400 hover:text-ink-900 hover:bg-ink-100 h-5 w-5 grid place-items-center opacity-0 group-hover:opacity-100 transition-opacity"
                        >
                          <svg viewBox="0 0 16 16" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
                            <path d="M11 2.5l2.5 2.5L5 13.5 2 14l.5-3L11 2.5z" />
                          </svg>
                        </button>
                        <button
                          type="button"
                          onClick={() => setConfirmId(n.id)}
                          title="Delete note"
                          className="rounded text-ink-400 hover:text-danger hover:bg-danger-dim h-5 w-5 grid place-items-center opacity-0 group-hover:opacity-100 transition-opacity"
                        >
                          <svg viewBox="0 0 16 16" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round">
                            <path d="M3 4h10M6.5 4V2.8a.8.8 0 01.8-.8h1.4a.8.8 0 01.8.8V4M4.5 4l.6 8.4a1 1 0 001 .9h3.8a1 1 0 001-.9L11.5 4" />
                          </svg>
                        </button>
                      </>
                    )}
                  </>
                )}
              </div>
            </div>
            {isEditing ? (
              <NoteEdit
                initial={n.text}
                onCancel={() => setEditingId(null)}
                onSave={(text) => onSaveEdit(n.id, text)}
              />
            ) : (
              <NoteBody text={n.text} />
            )}
          </li>
        );
      })}
    </ul>
  );
}

/** Note text with default-collapsed long-form bodies. Renders the full text
 *  for short notes; clamps anything past [`CLAMP_LINES`] lines or
 *  [`CLAMP_MIN_CHARS`] chars and shows a "Show more / Show less" toggle.
 *  The toggle state resets if the underlying text changes (re-extraction,
 *  edit) by keying on `text`. */
function NoteBody({ text }: { text: string }) {
  const t = useT();
  const [expanded, setExpanded] = useState(false);

  // Reset when the underlying note is replaced (edit / re-extract).
  useEffect(() => {
    setExpanded(false);
  }, [text]);

  const lineCount = (text.match(/\n/g)?.length ?? 0) + 1;
  const isLong = lineCount >= CLAMP_MIN_LINES || text.length >= CLAMP_MIN_CHARS;

  if (!isLong) {
    return (
      <p className="whitespace-pre-wrap text-sm leading-relaxed text-ink-900">
        {text}
      </p>
    );
  }

  return (
    <div className="relative">
      <p
        className={cn(
          "whitespace-pre-wrap text-sm leading-relaxed text-ink-900",
          !expanded && `line-clamp-[${CLAMP_LINES}]`,
        )}
        // Tailwind's JIT will pick up the `line-clamp-[<n>]` arbitrary value
        // above, but for safety we also set it inline so a missing JIT pass
        // can't silently drop the clamp.
        style={
          !expanded
            ? {
                display: "-webkit-box",
                WebkitLineClamp: CLAMP_LINES,
                WebkitBoxOrient: "vertical",
                overflow: "hidden",
              }
            : undefined
        }
      >
        {text}
      </p>
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="mt-1.5 inline-flex items-center gap-1 text-2xs font-medium text-ink-500 hover:text-ink-900 transition-colors"
      >
        {expanded ? t("common.show_less") : t("common.show_more")}
        <svg
          viewBox="0 0 12 12"
          className={cn("h-2.5 w-2.5 transition-transform", expanded && "rotate-180")}
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M2.5 4.5l3.5 3.5 3.5-3.5" />
        </svg>
      </button>
    </div>
  );
}

function NoteEdit({
  initial,
  onCancel,
  onSave,
}: {
  initial: string;
  onCancel: () => void;
  onSave: (text: string) => void | Promise<void>;
}) {
  const [text, setText] = useState(initial);
  const [saving, setSaving] = useState(false);

  async function submit() {
    const trimmed = text.trim();
    if (saving || trimmed.length === 0 || trimmed === initial.trim()) {
      if (trimmed === initial.trim()) {
        onCancel();
      }
      return;
    }
    setSaving(true);
    try {
      await onSave(text);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-3">
      <textarea
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void submit();
          } else if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        rows={Math.min(10, Math.max(3, text.split("\n").length + 1))}
        className="w-full rounded-md border border-accent bg-white px-3 py-2 text-sm text-ink-900 focus:outline-none focus:ring-2 focus:ring-accent/30 leading-relaxed"
      />
      <div className="flex items-center justify-between gap-3">
        <p className="text-2xs text-ink-500">save → re-extract</p>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={saving}
            className="inline-flex items-center rounded-md border border-ink-200 bg-white px-3 h-8 text-xs font-medium text-ink-900 hover:bg-ink-100"
          >
            cancel
          </button>
          <button
            type="button"
            onClick={() => void submit()}
            disabled={saving || text.trim().length === 0}
            className={cn(
              "inline-flex items-center rounded-md bg-ink-900 px-3 h-8 text-xs font-medium text-white hover:bg-ink-800",
              "disabled:opacity-40 disabled:cursor-not-allowed",
            )}
          >
            {saving ? "saving…" : "save & re-extract"}
          </button>
        </div>
      </div>
    </div>
  );
}

type SourceKind = "manual" | "hotkey" | "selection" | "email" | "calendar" | "import" | "other";

function parseSource(s: string): { kind: SourceKind; label: string } {
  if (s === "manual") return { kind: "manual", label: "manual" };
  if (s === "hotkey") return { kind: "hotkey", label: "hotkey" };
  if (s.startsWith("selection:")) return { kind: "selection", label: shortBundle(s.slice(10)) };
  if (s.startsWith("selection")) return { kind: "selection", label: "selection" };
  if (s.startsWith("email:")) return { kind: "email", label: "email" };
  if (s.startsWith("calendar:")) return { kind: "calendar", label: "calendar" };
  if (s.startsWith("import:")) return { kind: "import", label: "import" };
  return { kind: "other", label: s };
}

function shortBundle(bundle: string): string {
  const parts = bundle.split(".");
  return parts[parts.length - 1] || bundle;
}

const SOURCE_DOT: Record<SourceKind, string> = {
  manual: "bg-ink-400",
  hotkey: "bg-entity-event",
  selection: "bg-entity-skill",
  email: "bg-entity-belief",
  calendar: "bg-entity-goal",
  import: "bg-entity-asset",
  other: "bg-ink-400",
};

function SourceBadge({ kind, label }: { kind: SourceKind; label: string }) {
  return (
    <span
      className="inline-flex items-center gap-1 rounded text-2xs font-medium text-ink-600 uppercase tracking-caps"
      title={label}
    >
      <span className={cn("h-1.5 w-1.5 rounded-full", SOURCE_DOT[kind])} />
      {label}
    </span>
  );
}

function Status({ status, error }: { status: string; error: string | null }) {
  if (status === "pending")
    return <StatusPill tone="warn" dot>extracting</StatusPill>;
  if (status === "failed")
    return (
      <span title={error ?? ""}>
        <StatusPill tone="danger" dot>failed</StatusPill>
      </span>
    );
  return <StatusPill tone="accent" dot>saved</StatusPill>;
}
