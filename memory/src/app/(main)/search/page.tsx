"use client";
import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import {
  search,
  logRetrieval,
  recordSignalAction,
} from "@/lib/tauri";
import type { RetrievedEntity, RetrievedNote, SearchPayload } from "@/lib/types";
import { cn, relativeTime, safeJsonParse, truncate } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { EntityTag, HashChip, Kbd, SectionEyebrow } from "@/components/ui";

type Filter = "all" | "entities" | "notes";

export default function SearchPage() {
  const t = useT();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [results, setResults] = useState<SearchPayload | null>(null);
  const [signalId, setSignalId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [latency, setLatency] = useState<number | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    ref.current?.focus();
  }, []);

  async function run() {
    if (busy) return;
    const q = query.trim();
    if (!q) {
      setResults(null);
      return;
    }
    setBusy(true);
    setErr(null);
    const started = performance.now();
    try {
      const r = await search(q, undefined, 20);
      setResults(r);
      setLatency(Math.round(performance.now() - started));
      try {
        const sid = await logRetrieval(
          q,
          undefined,
          r.entities.map((e) => e.entity.id),
        );
        setSignalId(sid);
      } catch {
        setSignalId(null);
      }
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  async function onClick(entityId: string) {
    if (!signalId) return;
    try {
      await recordSignalAction(signalId, "click", entityId);
    } catch {
      /* tolerant */
    }
  }

  async function onDismiss(entityId: string) {
    if (!signalId) return;
    try {
      await recordSignalAction(signalId, "dismiss", entityId);
      setResults((r) =>
        r
          ? {
              ...r,
              entities: r.entities.filter((e) => e.entity.id !== entityId),
            }
          : r,
      );
    } catch {
      /* tolerant */
    }
  }

  const totalCount =
    (results?.entities.length ?? 0) + (results?.notes.length ?? 0);
  const showEntities = (filter === "all" || filter === "entities") && results;
  const showNotes = (filter === "all" || filter === "notes") && results;

  return (
    <div className="max-w-3xl mx-auto px-8 py-10 space-y-5">
      <div
        className={cn(
          "rounded-xl bg-white border-2 transition-shadow",
          "shadow-[0_8px_24px_rgba(0,0,0,0.06)]",
          query.trim() ? "border-ink-900" : "border-ink-200",
        )}
      >
        <div className="flex items-center gap-3 px-5 py-3">
          <SearchIcon />
          <input
            ref={ref}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                run();
              }
            }}
            placeholder={t("appbar.search_placeholder")}
            className="flex-1 bg-transparent text-md text-ink-900 placeholder:text-ink-400 focus:outline-none"
          />
          {busy && <span className="text-2xs text-ink-400 font-mono">searching…</span>}
          <Kbd>esc</Kbd>
        </div>
      </div>

      {err && <p className="text-danger text-xs font-mono">{err}</p>}

      {results && (
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-baseline gap-2.5">
            <span className="font-mono text-xs font-medium text-ink-900">
              {totalCount} {totalCount === 1 ? "result" : "results"}
            </span>
            <span className="text-2xs text-ink-300">·</span>
            <span className="text-2xs text-ink-400">
              {latency !== null ? `${latency} ms · ` : ""}
              local BM25 {results.diagnostics.bm25_hits ? `(${results.diagnostics.bm25_hits} hits)` : ""}
              {results.diagnostics.vector_hits > 0
                ? ` + vector (${results.diagnostics.vector_hits} hits)`
                : ""}
              {" "}+ importance + recency
            </span>
          </div>
          <FilterChips value={filter} onChange={setFilter} />
        </div>
      )}

      {showEntities && results.entities.length > 0 && (
        <Group label="entities" count={results.entities.length}>
          {results.entities.map((re, idx) => (
            <EntityResult
              key={re.entity.id}
              item={re}
              highlight={idx === 0}
              onClick={() => onClick(re.entity.id)}
              onDismiss={() => onDismiss(re.entity.id)}
            />
          ))}
        </Group>
      )}

      {showNotes && results.notes.length > 0 && (
        <Group label="notes" count={results.notes.length}>
          {results.notes.map((n) => (
            <NoteResult key={n.note.id} item={n} />
          ))}
        </Group>
      )}

      {results &&
        results.entities.length === 0 &&
        results.notes.length === 0 && (
          <p className="text-sm text-ink-500">no matches.</p>
        )}

      <div className="pt-2 flex items-center justify-center gap-4 text-2xs text-ink-400">
        <span>↑↓ navigate</span>
        <span>
          <Kbd>↵</Kbd> open
        </span>
        <span className="text-ink-300">·</span>
        <span>
          <Kbd>⌘</Kbd>
          <Kbd>↵</Kbd> open in new pane
        </span>
      </div>
    </div>
  );
}

function FilterChips({
  value,
  onChange,
}: {
  value: Filter;
  onChange: (v: Filter) => void;
}) {
  const opts: { id: Filter; label: string }[] = [
    { id: "all", label: "All" },
    { id: "entities", label: "Entities" },
    { id: "notes", label: "Notes" },
  ];
  return (
    <div className="flex items-center gap-1.5">
      {opts.map((o) => (
        <button
          key={o.id}
          type="button"
          onClick={() => onChange(o.id)}
          className={cn(
            "rounded-full px-2.5 h-6 text-2xs font-medium transition-colors",
            value === o.id
              ? "bg-ink-900 text-white"
              : "bg-ink-100 text-ink-500 hover:text-ink-900",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function Group({
  label,
  count,
  children,
}: {
  label: string;
  count: number;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-2">
      <div className="flex items-baseline gap-2">
        <SectionEyebrow>{label}</SectionEyebrow>
        <span className="font-mono text-2xs text-ink-400">{count}</span>
      </div>
      <div className="space-y-2">{children}</div>
    </section>
  );
}

function EntityResult({
  item,
  highlight,
  onClick,
  onDismiss,
}: {
  item: RetrievedEntity;
  highlight?: boolean;
  onClick: () => void;
  onDismiss: () => void;
}) {
  const props = safeJsonParse<Record<string, unknown>>(item.entity.properties, {});
  const propVal = Object.values(props)[0];
  const summary = propVal != null ? formatVal(propVal) : item.entity.name;
  const pct = (n: number) =>
    Number.isFinite(n) ? `${Math.max(0, Math.min(100, n * 100)).toFixed(0)}%` : "—";
  const bm = pct(item.rrf_score);
  const vec = Number.isFinite(item.vector_score)
    ? item.vector_score.toFixed(2)
    : null;
  const imp = pct(item.importance_score);
  const rec = pct(item.recency_score);

  return (
    <div
      className={cn(
        "rounded-md bg-white border transition-all",
        highlight ? "border-ink-900" : "border-ink-200 hover:border-ink-300",
      )}
    >
      <Link
        href={`/entity?id=${encodeURIComponent(item.entity.id)}`}
        onClick={onClick}
        className="flex items-center gap-3 px-3 py-3"
      >
        <EntityTag type={item.entity.type} small />
        <div className="flex-1 min-w-0 space-y-1">
          <p className="text-sm text-ink-900 truncate">
            {truncate(summary, 100)}
          </p>
          <div className="flex items-center gap-2 text-2xs font-mono">
            <span className="text-accent-fg">score {item.final_score.toFixed(2)}</span>
            <span className="text-ink-300">·</span>
            <span className="text-ink-400">
              BM25 {bm}
              {vec && ` + vec ${vec}`} + imp {imp} + rec {rec}
            </span>
          </div>
        </div>
        <HashChip hash={item.entity.id} />
        <button
          type="button"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onDismiss();
          }}
          className="text-ink-300 hover:text-danger text-xs h-6 w-6 inline-flex items-center justify-center"
          aria-label="dismiss"
          title="dismiss"
        >
          ×
        </button>
      </Link>
    </div>
  );
}

function NoteResult({ item }: { item: RetrievedNote }) {
  return (
    <div className="rounded-md bg-white border border-ink-200 px-3 py-3 hover:border-ink-300">
      <div className="flex items-start gap-3">
        <FileIcon />
        <div className="flex-1 min-w-0 space-y-1">
          <p className="text-sm text-ink-900 whitespace-pre-wrap">
            {truncate(item.note.text, 200)}
          </p>
          <div className="font-mono text-2xs text-ink-400">
            score {item.final_score.toFixed(2)} ·{" "}
            {relativeTime(item.note.created_at)} · {item.note.source || "manual"}
          </div>
        </div>
        <HashChip hash={item.note.id} />
      </div>
    </div>
  );
}

function SearchIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 text-ink-900"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-4-4" />
    </svg>
  );
}

function FileIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 text-ink-400 shrink-0 mt-0.5"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6 M16 13H8 M16 17H8 M10 9H8" />
    </svg>
  );
}

function formatVal(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return JSON.stringify(v);
}
