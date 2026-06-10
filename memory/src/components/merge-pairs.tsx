"use client";

// MergePairsOverlay — full-page modal for walking through dedup pairs.
// Triggered from the "Review pairs →" CTA on /entities (Pencil mockup
// `YjPfO`). Keyboard-driven: ←/→ to move between pairs, Enter / `m` to
// merge, `r` to reject, `s` to skip, Esc to dismiss.

import { useCallback, useEffect, useState } from "react";
import type { DedupCandidate, Entity, MergeReport } from "@/lib/types";
import { mergeEntities, rejectDedupPair } from "@/lib/tauri";
import { cn, relativeTime, safeJsonParse, truncate } from "@/lib/utils";
import { BtnGhost, BtnSecondary, EntityTag, Kbd, StatusPill } from "@/components/ui";

// Aligns with backend MANUAL_EDIT_CAP = 3 — the trust-boost pill in the
// meta line shouldn't claim more credit than the retrieval layer actually
// applies (10% per correction, capped at 30%).
const MANUAL_EDIT_CAP = 3;

export function MergePairsOverlay({
  candidates,
  onClose,
  onPairResolved,
}: {
  candidates: DedupCandidate[];
  onClose: () => void;
  // Notified after each merge / reject so the parent can refetch or
  // splice the candidate out of its cached list.
  onPairResolved?: (pair: DedupCandidate, action: "merge" | "reject" | "skip", report?: MergeReport) => void;
}) {
  // Working copy of the candidate list — we remove pairs in-place as the
  // user resolves them. Index points at the *current* pair.
  const [pending, setPending] = useState<DedupCandidate[]>(candidates);
  const [index, setIndex] = useState(0);
  // Whether the LEFT column is the chosen "keep" side. The default
  // matches the backend's recommendation (`keep_id` / `keep_entity`).
  // Flipped when the user clicks the "MERGE INTO ABOVE" radio.
  const [leftIsKeep, setLeftIsKeep] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Counter of successful merges in this session — surfaced on the
  // "all caught up" empty state. Reject/skip don't count.
  const [mergesThisSession, setMergesThisSession] = useState(0);

  // Reset which-side-is-keep whenever the pair changes — the user's
  // previous choice was about *that* pair, not this one.
  useEffect(() => {
    setLeftIsKeep(true);
    setError(null);
  }, [index]);

  // Mirror prop changes when the parent refreshes the candidate list
  // (e.g. after a background refresh elsewhere in the app).
  useEffect(() => {
    setPending(candidates);
    setIndex(0);
    setMergesThisSession(0);
  }, [candidates]);

  const current = pending[index];
  const total = pending.length;

  const advanceOrFinish = useCallback(() => {
    setPending((prev) => {
      const next = prev.filter((_, i) => i !== index);
      // When we drop the current pair, the next-pair index is the same
      // numeric position — unless we just ate the last one.
      if (index >= next.length) {
        setIndex(Math.max(0, next.length - 1));
      }
      return next;
    });
  }, [index]);

  const handleMerge = useCallback(async () => {
    if (!current || busy) return;
    setBusy(true);
    setError(null);
    try {
      const keepEntity = leftIsKeep ? current.keep_entity : current.drop_entity;
      const dropEntity = leftIsKeep ? current.drop_entity : current.keep_entity;
      const report = await mergeEntities(keepEntity.id, dropEntity.id);
      setMergesThisSession((n) => n + 1);
      onPairResolved?.(current, "merge", report);
      advanceOrFinish();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }, [advanceOrFinish, busy, current, leftIsKeep, onPairResolved]);

  const handleReject = useCallback(async () => {
    if (!current || busy) return;
    setBusy(true);
    setError(null);
    try {
      // Always reject by the *backend's* original keep/drop ordering — the
      // swap only affects which side wins on merge, not which canonical
      // pair we're saying "no" to.
      await rejectDedupPair(current.keep_id, current.drop_id);
      onPairResolved?.(current, "reject");
      advanceOrFinish();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }, [advanceOrFinish, busy, current, onPairResolved]);

  const handleSkip = useCallback(() => {
    if (!current) return;
    onPairResolved?.(current, "skip");
    // Just nudge the index forward; the pair stays in the working set so
    // the user can come back if they hit prev.
    setIndex((i) => Math.min(i + 1, total - 1));
  }, [current, onPairResolved, total]);

  const handlePrev = useCallback(() => setIndex((i) => Math.max(0, i - 1)), []);
  const handleNext = useCallback(
    () => setIndex((i) => Math.min(total - 1, i + 1)),
    [total],
  );

  // Keyboard shortcuts. Skipping text inputs isn't an issue here — we
  // never put a focusable field inside the overlay.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        handlePrev();
        return;
      }
      if (e.key === "ArrowRight") {
        e.preventDefault();
        handleNext();
        return;
      }
      if (e.key === "Enter" || e.key === "m" || e.key === "M") {
        e.preventDefault();
        void handleMerge();
        return;
      }
      if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        void handleReject();
        return;
      }
      if (e.key === "s" || e.key === "S") {
        e.preventDefault();
        handleSkip();
        return;
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [handleMerge, handleNext, handlePrev, handleReject, handleSkip, onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-ink-50/95 backdrop-blur-sm px-6 py-10"
      role="dialog"
      aria-modal="true"
      aria-label="Review duplicate entity pairs"
      onClick={onClose}
    >
      <div
        className="relative w-full max-w-3xl rounded-2xl border border-ink-200 bg-white shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {current ? (
          <PairBody
            pair={current}
            indexLabel={`Pair ${index + 1} of ${total}`}
            leftIsKeep={leftIsKeep}
            onFlip={() => setLeftIsKeep((v) => !v)}
            onMerge={() => void handleMerge()}
            onReject={() => void handleReject()}
            onSkip={handleSkip}
            onPrev={handlePrev}
            onNext={handleNext}
            onClose={onClose}
            busy={busy}
            error={error}
            hasPrev={index > 0}
            hasNext={index < total - 1}
          />
        ) : (
          <AllCaughtUp count={mergesThisSession} onClose={onClose} />
        )}
      </div>
    </div>
  );
}

function PairBody({
  pair,
  indexLabel,
  leftIsKeep,
  onFlip,
  onMerge,
  onReject,
  onSkip,
  onPrev,
  onNext,
  onClose,
  busy,
  error,
  hasPrev,
  hasNext,
}: {
  pair: DedupCandidate;
  indexLabel: string;
  leftIsKeep: boolean;
  onFlip: () => void;
  onMerge: () => void;
  onReject: () => void;
  onSkip: () => void;
  onPrev: () => void;
  onNext: () => void;
  onClose: () => void;
  busy: boolean;
  error: string | null;
  hasPrev: boolean;
  hasNext: boolean;
}) {
  // The left column is always the "currently selected keep". When the
  // user clicks "MERGE INTO ABOVE" on the right card, we swap which
  // half of the pair we treat as keep. The data tied to each side
  // (note counts, edits) follows the original keep/drop assignment so
  // the meta lines stay accurate to the entity, not to its slot.
  const left = leftIsKeep
    ? { entity: pair.keep_entity, edits: pair.keep_manual_edits, notes: pair.keep_note_count }
    : { entity: pair.drop_entity, edits: pair.drop_manual_edits, notes: pair.drop_note_count };
  const right = leftIsKeep
    ? { entity: pair.drop_entity, edits: pair.drop_manual_edits, notes: pair.drop_note_count }
    : { entity: pair.keep_entity, edits: pair.keep_manual_edits, notes: pair.keep_note_count };

  const pct = Math.round(Math.max(0, Math.min(1, pair.similarity)) * 100);

  return (
    <>
      <header className="flex items-center justify-between gap-3 border-b border-ink-200 px-5 h-12">
        <div className="flex items-center gap-3 min-w-0">
          <span className="font-mono text-2xs text-ink-500">{indexLabel}</span>
          <StatusPill tone="accent">{pct}% similar</StatusPill>
        </div>
        <div className="flex items-center gap-1">
          <BtnGhost onClick={onPrev} disabled={!hasPrev} title="Prev pair (←)">
            <ArrowGlyph direction="left" />
          </BtnGhost>
          <BtnGhost onClick={onNext} disabled={!hasNext} title="Next pair (→)">
            <ArrowGlyph direction="right" />
          </BtnGhost>
          <button
            type="button"
            onClick={onClose}
            className="inline-flex items-center gap-1 text-2xs uppercase tracking-caps text-ink-500 hover:text-ink-900 px-2 h-7"
            title="Close (Esc)"
          >
            esc
          </button>
        </div>
      </header>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 p-5">
        <PairColumn
          slot="keep"
          entity={left.entity}
          edits={left.edits}
          noteCount={left.notes}
          onClickRadio={leftIsKeep ? undefined : onFlip}
        />
        <PairColumn
          slot={leftIsKeep ? "drop" : "keep"}
          entity={right.entity}
          edits={right.edits}
          noteCount={right.notes}
          onClickRadio={leftIsKeep ? onFlip : undefined}
        />
      </div>

      {error && (
        <div className="mx-5 mb-3 rounded-md border border-danger bg-danger-dim px-3 py-2 text-xs text-danger">
          {error}
        </div>
      )}

      <footer className="flex items-center justify-between gap-3 border-t border-ink-200 px-5 h-14 bg-ink-100/40 rounded-b-2xl">
        <p className="text-2xs text-ink-500">
          source notes preserved · relations migrated · history kept
        </p>
        <div className="flex items-center gap-2">
          <BtnGhost onClick={onReject} disabled={busy} title="Mark as not duplicates (r)">
            not duplicates
          </BtnGhost>
          <BtnSecondary onClick={onSkip} disabled={busy} title="Skip (s)">
            skip
          </BtnSecondary>
          <button
            type="button"
            onClick={onMerge}
            disabled={busy}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md bg-accent px-3 h-8 text-xs font-medium text-white",
              "hover:bg-accent-fg disabled:opacity-40 disabled:cursor-not-allowed transition-colors",
            )}
            title="Merge (Enter / m)"
          >
            {busy ? "merging…" : "Merge"}
            <Kbd className="!h-4 !px-1 !text-[10px] bg-white/15 border-white/25 text-white/90 shadow-none">
              ↵
            </Kbd>
          </button>
        </div>
      </footer>
    </>
  );
}

function PairColumn({
  slot,
  entity,
  edits,
  noteCount,
  onClickRadio,
}: {
  slot: "keep" | "drop";
  entity: Entity;
  edits: number;
  noteCount: number;
  onClickRadio?: () => void;
}) {
  const keep = slot === "keep";
  const props = safeJsonParse<Record<string, unknown>>(entity.properties, {});
  const stmt =
    typeof props.statement === "string" && props.statement.trim().length > 0
      ? props.statement
      : truncate(entity.properties, 120);
  const corrected = edits > 0;
  const trustBoost = Math.min(edits, MANUAL_EDIT_CAP) * 10;

  return (
    <section
      className={cn(
        "rounded-xl border p-4 transition-colors",
        keep
          ? "bg-accent-dim border-accent"
          : "bg-white border-ink-200",
      )}
    >
      <header className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <Radio
            filled={keep}
            // Only the *non-keep* radio is clickable — clicking the
            // already-selected one would be a no-op.
            onClick={onClickRadio}
            label={keep ? "Mark as keep (already selected)" : "Make this the keep side"}
          />
          <span
            className={cn(
              "eyebrow",
              keep ? "text-accent-fg" : "text-ink-500",
            )}
          >
            {keep ? "KEEP THIS" : "MERGE INTO ABOVE"}
          </span>
        </div>
        {corrected && (
          <span className="inline-flex items-center gap-1 px-1.5 py-px rounded-sm bg-accent-dim border border-accent text-accent-fg text-[10px] font-mono font-semibold whitespace-nowrap">
            <span aria-hidden>{"✎"}</span>
            corrected {edits}&times;
          </span>
        )}
      </header>

      <div className="mt-3 flex items-center gap-2">
        <EntityTag type={entity.type} small />
      </div>

      <h3 className="mt-2 font-display text-md font-semibold leading-snug text-ink-900">
        {entity.name}
      </h3>

      <p className="mt-1.5 text-xs text-ink-700 leading-relaxed line-clamp-4 whitespace-pre-wrap break-words">
        {stmt}
      </p>

      <div className="mt-3 flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-2xs text-ink-500">
        <span>{relativeTime(entity.updated_at)}</span>
        <span className="text-ink-300">·</span>
        <span>
          {noteCount} {noteCount === 1 ? "source note" : "source notes"}
        </span>
        <span className="text-ink-300">·</span>
        <span>used {entity.access_count}×</span>
        {corrected && (
          <>
            <span className="text-ink-300">·</span>
            <span className="text-accent-fg">trust +{trustBoost}%</span>
          </>
        )}
      </div>
    </section>
  );
}

function Radio({
  filled,
  onClick,
  label,
}: {
  filled: boolean;
  onClick?: () => void;
  label: string;
}) {
  const interactive = !!onClick;
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={!interactive}
      aria-label={label}
      title={label}
      className={cn(
        "inline-flex items-center justify-center h-4 w-4 rounded-full border transition-colors shrink-0",
        filled ? "border-accent bg-white" : "border-ink-300 bg-white",
        interactive && "hover:border-accent cursor-pointer",
        !interactive && "cursor-default",
      )}
    >
      {filled && <span className="h-2 w-2 rounded-full bg-accent" />}
    </button>
  );
}

function AllCaughtUp({ count, onClose }: { count: number; onClose: () => void }) {
  return (
    <div className="px-6 py-10 text-center space-y-3">
      <p className="font-display text-lg font-semibold text-ink-900">
        <span aria-hidden className="text-accent">✓</span> All caught up
      </p>
      <p className="text-xs text-ink-500">
        {count > 0
          ? `${count} ${count === 1 ? "merge" : "merges"} this session`
          : "No more pairs to review."}
      </p>
      <div className="pt-2">
        <BtnSecondary onClick={onClose}>close</BtnSecondary>
      </div>
    </div>
  );
}

function ArrowGlyph({ direction }: { direction: "left" | "right" }) {
  const d = direction === "left" ? "M15 18l-6-6 6-6" : "M9 6l6 6-6 6";
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-3.5 w-3.5 text-ink-400"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d={d} />
    </svg>
  );
}

