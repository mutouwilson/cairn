"use client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import {
  createEntity,
  listArchivedEntities,
  listDedupCandidates,
  listEntitiesRanked,
  setEntityArchived,
} from "@/lib/tauri";
import type { DedupCandidate, Entity, RankedEntity } from "@/lib/types";
import { ENTITY_TYPES } from "@/lib/types";
import { useT } from "@/lib/i18n";
import { relativeTime, safeJsonParse, truncate } from "@/lib/utils";
import { cn } from "@/lib/utils";
import {
  BtnPrimary,
  BtnSecondary,
  Card,
  EntityTag,
  HashChip,
  SectionEyebrow,
  StatusPill,
} from "@/components/ui";
import { MergePairsOverlay } from "@/components/merge-pairs";

// Aligns with backend MANUAL_EDIT_CAP = 3 (see retrieval scoring). The
// "trust +N%" label can therefore claim at most 30%.
const MANUAL_EDIT_CAP = 3;

// Theme rollup threshold. Themes with fewer than this many members get
// flattened back into the single-entities grid — a one-member "theme"
// reads as noise.
const THEME_MIN_MEMBERS = 3;

// Top of mind cap (mockup `nztv0`: "top 30 of 287"). Solo entities beyond
// this are folded into the "↓ N more entities" toggle.
const TOP_OF_MIND_CAP = 30;

const ENTITY_DOT_COLOR: Record<string, string> = {
  Person: "bg-entity-person",
  Event: "bg-entity-event",
  Preference: "bg-entity-preference",
  Belief: "bg-entity-belief",
  Goal: "bg-entity-goal",
  Asset: "bg-entity-asset",
  Skill: "bg-entity-skill",
  Location: "bg-entity-location",
};

// Filter chip order matches the existing nav — Goal/Belief/Preference/Person
// up front because they're the most common.
const PRIMARY_TYPES: ReadonlyArray<(typeof ENTITY_TYPES)[number]> = [
  "Goal",
  "Belief",
  "Preference",
  "Person",
];
const SECONDARY_TYPES: ReadonlyArray<(typeof ENTITY_TYPES)[number]> = [
  "Event",
  "Asset",
  "Skill",
  "Location",
];

interface ThemeBucket {
  themeId: string;
  themeTitle: string;
  members: RankedEntity[];
}

export default function EntitiesPage() {
  const t = useT();
  const [type, setType] = useState<string | undefined>(undefined);
  const [ranked, setRanked] = useState<RankedEntity[]>([]);
  const [candidates, setCandidates] = useState<DedupCandidate[]>([]);
  const [archived, setArchived] = useState<Entity[]>([]);
  const [loading, setLoading] = useState(true);
  const [createOpen, setCreateOpen] = useState(false);
  const [showOverflow, setShowOverflow] = useState(false);
  const [expandedThemes, setExpandedThemes] = useState<Set<string>>(new Set());
  const [bannerDismissed, setBannerDismissed] = useState(false);
  const [overlayOpen, setOverlayOpen] = useState(false);

  const refreshAll = useCallback(async () => {
    try {
      // Parallel fetches — the three lists are independent.
      const [cands, ranks, arch] = await Promise.all([
        listDedupCandidates(20).catch(() => [] as DedupCandidate[]),
        listEntitiesRanked(300).catch(() => [] as RankedEntity[]),
        listArchivedEntities(100).catch(() => [] as Entity[]),
      ]);
      setCandidates(cands);
      setRanked(ranks);
      setArchived(arch);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshAll();
  }, [refreshAll]);

  // Bucket entities into theme groups vs solos. Themes need at least
  // THEME_MIN_MEMBERS entities to be worth surfacing — otherwise they
  // fall back into the solo grid so we don't render a "theme of 1".
  const { themeBuckets, solos } = useMemo(() => {
    const buckets = new Map<string, ThemeBucket>();
    const unthemed: RankedEntity[] = [];
    // Stable iteration preserves the rank order coming from the backend.
    for (const row of ranked) {
      if (type && row.entity.type !== type) continue;
      if (row.theme_id && row.theme_title) {
        const bucket = buckets.get(row.theme_id);
        if (bucket) {
          bucket.members.push(row);
        } else {
          buckets.set(row.theme_id, {
            themeId: row.theme_id,
            themeTitle: row.theme_title,
            members: [row],
          });
        }
      } else {
        unthemed.push(row);
      }
    }
    const big: ThemeBucket[] = [];
    for (const b of buckets.values()) {
      if (b.members.length >= THEME_MIN_MEMBERS) {
        big.push(b);
      } else {
        // Fall back to solos when the bucket is too small. Maintains
        // backend rank ordering within each bucket since members are
        // appended in iteration order.
        unthemed.push(...b.members);
      }
    }
    // Re-sort solos by final_score in case the fallback path shuffled
    // ordering relative to the backend list.
    unthemed.sort((a, b) => b.final_score - a.final_score);
    // Themes sorted by total signal — sum of member scores — so the
    // most consequential cluster floats to the top.
    big.sort((a, b) => themeWeight(b) - themeWeight(a));
    return { themeBuckets: big, solos: unthemed };
  }, [ranked, type]);

  // Filter-chip counts derived from the full ranked list (not the type-
  // scoped solos) so the user can see how many of each type exist.
  const typeCounts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const r of ranked) c[r.entity.type] = (c[r.entity.type] ?? 0) + 1;
    return c;
  }, [ranked]);

  const totalAll = ranked.length;

  const visibleSolos = showOverflow ? solos : solos.slice(0, TOP_OF_MIND_CAP);
  const overflowCount = Math.max(0, solos.length - TOP_OF_MIND_CAP);

  function toggleTheme(themeId: string) {
    setExpandedThemes((prev) => {
      const next = new Set(prev);
      if (next.has(themeId)) next.delete(themeId);
      else next.add(themeId);
      return next;
    });
  }

  function dropCandidate(pair: DedupCandidate) {
    setCandidates((prev) =>
      prev.filter((p) => !(p.keep_id === pair.keep_id && p.drop_id === pair.drop_id)),
    );
  }

  async function handleRestore(id: string) {
    try {
      await setEntityArchived(id, false);
      setArchived((prev) => prev.filter((e) => e.id !== id));
      // Refresh the ranked list so the restored entity reappears at its
      // signal-rank position.
      void refreshAll();
    } catch {
      /* swallowed — surfacing per-row archive errors is overkill here */
    }
  }

  return (
    <div className="max-w-4xl mx-auto px-8 py-8 space-y-5">
      <header className="space-y-1.5">
        <SectionEyebrow>
          BY SIGNAL · top {Math.min(TOP_OF_MIND_CAP, solos.length)} of {totalAll}
        </SectionEyebrow>
        <div className="flex items-end justify-between gap-6">
          <div className="flex items-baseline gap-3">
            <h1 className="font-display text-xl font-semibold text-ink-900">
              {t("entities.title")}
            </h1>
            <span className="font-mono text-2xs text-ink-400">
              {t("entities.subtitle")}
            </span>
          </div>
          <BtnPrimary onClick={() => setCreateOpen(true)} title={t("entities.new_entity")}>
            <PlusIcon /> {t("entities.new_entity")}
          </BtnPrimary>
        </div>
      </header>

      {!bannerDismissed && candidates.length > 0 && (
        <DedupBanner
          count={candidates.length}
          onReview={() => setOverlayOpen(true)}
          onDismiss={() => setBannerDismissed(true)}
        />
      )}

      {loading ? (
        <p className="text-sm text-ink-500">{t("common.loading")}</p>
      ) : ranked.length === 0 ? (
        <Card className="text-sm text-ink-500">
          {t("entities.no_entities")}
        </Card>
      ) : (
        <>
          {themeBuckets.length > 0 && (
            <section className="space-y-3">
              {themeBuckets.map((bucket) => (
                <ThemeCard
                  key={bucket.themeId}
                  bucket={bucket}
                  expanded={expandedThemes.has(bucket.themeId)}
                  onToggle={() => toggleTheme(bucket.themeId)}
                />
              ))}
            </section>
          )}

          <section className="space-y-3">
            <div className="flex items-center justify-between gap-3">
              <SectionEyebrow>
                SINGLE ENTITIES · {solos.length} hi-signal
              </SectionEyebrow>
              <FilterChipBar
                type={type}
                counts={typeCounts}
                totalAll={totalAll}
                onChange={setType}
              />
            </div>
            {solos.length === 0 ? (
              <Card className="text-sm text-ink-500">
                no solo entities for this filter.
              </Card>
            ) : (
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
                {visibleSolos.map((row) => (
                  <EntityCard key={row.entity.id} row={row} />
                ))}
              </div>
            )}

            {overflowCount > 0 && (
              <button
                type="button"
                onClick={() => setShowOverflow((v) => !v)}
                className={cn(
                  "w-full rounded-lg border border-dashed border-ink-200 bg-ink-100/40 px-4 py-3",
                  "text-2xs font-mono text-ink-500 hover:bg-ink-100/80 hover:text-ink-700 transition-colors",
                )}
              >
                {showOverflow
                  ? `↑ collapse · ${overflowCount} less frequently referenced`
                  : `↓ ${overflowCount} more entities · less frequently referenced`}
              </button>
            )}
          </section>

          {archived.length > 0 && (
            <ArchivedSection
              archived={archived}
              onRestore={(id) => void handleRestore(id)}
            />
          )}
        </>
      )}

      {createOpen && (
        <CreateEntityModal
          onClose={() => setCreateOpen(false)}
          onCreated={() => {
            void refreshAll();
          }}
        />
      )}

      {overlayOpen && (
        <MergePairsOverlay
          candidates={candidates}
          onClose={() => {
            setOverlayOpen(false);
            // Pull fresh signal data after merges so the dashboard
            // reflects the new entity universe.
            void refreshAll();
          }}
          onPairResolved={(pair, action) => {
            if (action === "merge" || action === "reject") {
              dropCandidate(pair);
            }
          }}
        />
      )}
    </div>
  );
}

function themeWeight(bucket: ThemeBucket): number {
  let s = 0;
  for (const m of bucket.members) s += m.final_score;
  return s;
}

// Top-of-page dedup banner — mockup `XhMPG`. Accent-dim background +
// accent border, with a "Review pairs →" CTA and a dismiss link.
function DedupBanner({
  count,
  onReview,
  onDismiss,
}: {
  count: number;
  onReview: () => void;
  onDismiss: () => void;
}) {
  return (
    <Card className="bg-accent-dim border-accent p-4 flex items-center gap-4">
      <div className="flex-1 min-w-0">
        <p className="text-sm font-semibold text-accent-fg">
          <span aria-hidden className="mr-1.5">✦</span>
          {count} {count === 1 ? "pair looks" : "pairs look"} like duplicates
        </p>
        <p className="mt-0.5 text-xs text-ink-700">
          Cairn found entities with similar names or meanings. Merging dedupes
          your memory store and preserves both source notes.
        </p>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        <button
          type="button"
          onClick={onDismiss}
          className="text-2xs uppercase tracking-caps text-ink-500 hover:text-ink-900 px-2 h-7"
        >
          dismiss
        </button>
        <button
          type="button"
          onClick={onReview}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-md bg-accent px-3 h-8 text-xs font-medium text-white",
            "hover:bg-accent-fg transition-colors",
          )}
        >
          Review pairs →
        </button>
      </div>
    </Card>
  );
}

function ThemeCard({
  bucket,
  expanded,
  onToggle,
}: {
  bucket: ThemeBucket;
  expanded: boolean;
  onToggle: () => void;
}) {
  const first = bucket.members.slice(0, 4);
  const extra = bucket.members.length - first.length;
  return (
    <Card className="p-4">
      <header className="flex items-center gap-3 flex-wrap">
        <StatusPill tone="neutral" className="!bg-ink-900 !text-white !rounded-md uppercase tracking-caps">
          THEME
        </StatusPill>
        <span className="font-display text-md font-semibold text-ink-900">
          {bucket.themeTitle}
        </span>
        <span className="font-mono text-2xs text-ink-400">
          · {bucket.members.length} entities
        </span>
        <button
          type="button"
          onClick={onToggle}
          className="ml-auto text-2xs uppercase tracking-caps text-ink-500 hover:text-ink-900 inline-flex items-center gap-1"
        >
          {expanded ? "close ↑" : "open ↓"}
        </button>
      </header>
      {!expanded ? (
        <p className="mt-2 text-xs text-ink-700">
          {first.map((m) => m.entity.name).join(" · ")}
          {extra > 0 && (
            <span className="text-ink-500"> · +{extra} more</span>
          )}
        </p>
      ) : (
        <ThemeMemberList members={bucket.members} />
      )}
    </Card>
  );
}

function ThemeMemberList({ members }: { members: RankedEntity[] }) {
  // When the theme is expanded inline, render a compact single-column
  // list rather than a full 3-column grid — the design assumption is
  // that theme cards live above the main grid, so we don't want them
  // to dwarf it visually when one theme has 17 entities.
  // Cap at a reasonable height with overflow-y for the (rare) 30+
  // member case so the page doesn't stretch into a wall of cards.
  return (
    <div className="mt-3 max-h-[420px] overflow-y-auto rounded-md border border-ink-200 bg-ink-50/40 divide-y divide-ink-200">
      {members.map((row) => {
        const corrected = row.manual_edits > 0;
        const score = row.final_score.toFixed(2);
        return (
          <Link
            key={row.entity.id}
            href={`/entity?id=${encodeURIComponent(row.entity.id)}`}
            className={cn(
              "flex items-center gap-3 px-3 py-2.5 hover:bg-white transition-colors",
              corrected && "border-l-2 border-l-accent",
            )}
          >
            <EntityTag type={row.entity.type} small />
            <span className="flex-1 min-w-0 truncate text-sm text-ink-900">
              {row.entity.name}
            </span>
            {corrected && (
              <span className="text-2xs text-accent-fg font-mono">
                ✎ {row.manual_edits}
              </span>
            )}
            <span className="font-mono text-2xs text-ink-400 tabular-nums">
              {score}
            </span>
          </Link>
        );
      })}
    </div>
  );
}

function FilterChipBar({
  type,
  counts,
  totalAll,
  onChange,
}: {
  type: string | undefined;
  counts: Record<string, number>;
  totalAll: number;
  onChange: (t: string | undefined) => void;
}) {
  // Compact horizontal chip row. Mirrors the layout in mockup `nztv0`
  // ("all · person · event · belief …") — keep the existing primary /
  // secondary ordering so muscle memory transfers from the old view.
  const ordered: ReadonlyArray<(typeof ENTITY_TYPES)[number]> = [
    ...PRIMARY_TYPES,
    ...SECONDARY_TYPES,
  ];
  return (
    <div className="flex flex-wrap items-center gap-1.5 justify-end">
      <Chip
        active={type === undefined}
        onClick={() => onChange(undefined)}
        label="all"
        count={totalAll}
        dark
      />
      {ordered.map((t) => {
        if ((counts[t] ?? 0) === 0 && type !== t) return null;
        return (
          <Chip
            key={t}
            active={type === t}
            onClick={() => onChange(t)}
            label={t.toLowerCase()}
            count={counts[t] ?? 0}
            dotClass={ENTITY_DOT_COLOR[t]}
          />
        );
      })}
    </div>
  );
}

function ArchivedSection({
  archived,
  onRestore,
}: {
  archived: Entity[];
  onRestore: (id: string) => void;
}) {
  // Reduced opacity per spec — these entities are out of active rotation
  // and shouldn't compete visually with the top-of-mind grid.
  return (
    <section className="opacity-60 hover:opacity-100 transition-opacity space-y-3">
      <div className="flex items-center justify-between gap-3">
        <SectionEyebrow>
          ARCHIVED · {archived.length} auto-stashed
        </SectionEyebrow>
        <span className="text-2xs text-ink-500">
          low confidence + no access in 6 months
        </span>
      </div>
      <Card className="p-0 overflow-hidden">
        <ul className="divide-y divide-ink-200">
          {archived.slice(0, 20).map((e) => (
            <li
              key={e.id}
              className="flex items-center gap-3 px-3 py-2"
            >
              <EntityTag type={e.type} small />
              <Link
                href={`/entity?id=${encodeURIComponent(e.id)}`}
                className="flex-1 min-w-0 truncate text-sm text-ink-700 hover:text-ink-900"
              >
                {e.name}
              </Link>
              <span className="font-mono text-2xs text-ink-400">
                {relativeTime(e.last_accessed)}
              </span>
              <button
                type="button"
                onClick={() => onRestore(e.id)}
                className="text-2xs text-ink-500 hover:text-accent-fg underline-offset-2 hover:underline"
                title="Restore this entity to active rotation"
              >
                restore
              </button>
            </li>
          ))}
        </ul>
      </Card>
    </section>
  );
}

function CreateEntityModal({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: () => void;
}) {
  const router = useRouter();
  const [type, setType] = useState<(typeof ENTITY_TYPES)[number]>("Goal");
  const [name, setName] = useState("");
  const [detail, setDetail] = useState("");
  const [moreOpen, setMoreOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  const secondarySelected = SECONDARY_TYPES.includes(type);
  const expanded = moreOpen || secondarySelected;

  useEffect(() => {
    nameRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  async function submit() {
    const trimmed = name.trim();
    if (submitting || trimmed.length === 0) return;
    setSubmitting(true);
    setErr(null);
    try {
      const created = await createEntity({
        entity_type: type,
        name: trimmed,
        detail: detail.trim() || undefined,
      });
      onCreated();
      router.push(`/entity?id=${encodeURIComponent(created.id)}`);
    } catch (e) {
      setErr((e as Error).message);
      setSubmitting(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-ink-900/40 backdrop-blur-sm px-6 py-12"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Create memory directly"
        className="w-full max-w-xl rounded-xl border border-ink-200 bg-white shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-ink-200 px-5 h-12">
          <h2 className="font-display text-md font-semibold text-ink-900">
            Create memory directly
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="inline-flex items-center gap-1 text-2xs uppercase tracking-caps text-ink-500 hover:text-ink-900"
          >
            esc
          </button>
        </header>

        <div className="px-5 py-5 space-y-4">
          <p className="text-xs text-ink-500 leading-relaxed">
            Bypass capture + LLM extraction. For when you know exactly what you
            want to remember.
          </p>

          <section className="space-y-2">
            <FieldLabel>type</FieldLabel>
            <div className="flex flex-wrap items-center gap-1.5">
              {PRIMARY_TYPES.map((t) => (
                <TypeChip
                  key={t}
                  type={t}
                  active={type === t}
                  onClick={() => setType(t)}
                />
              ))}
              {expanded
                ? SECONDARY_TYPES.map((t) => (
                    <TypeChip
                      key={t}
                      type={t}
                      active={type === t}
                      onClick={() => setType(t)}
                    />
                  ))
                : (
                  <button
                    type="button"
                    onClick={() => setMoreOpen(true)}
                    className="inline-flex items-center gap-1 rounded-full border border-ink-200 bg-white px-2.5 h-7 text-2xs font-medium text-ink-500 hover:border-ink-300 hover:text-ink-900"
                  >
                    + {SECONDARY_TYPES.length} more
                  </button>
                )}
            </div>
          </section>

          <section className="space-y-2">
            <FieldLabel>name</FieldLabel>
            <input
              ref={nameRef}
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="short name"
              required
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void submit();
                }
              }}
              className="w-full rounded-md border border-accent bg-white px-3 h-9 text-sm text-ink-900 focus:outline-none focus:ring-2 focus:ring-accent/30"
            />
          </section>

          <section className="space-y-2">
            <FieldLabel>detail (optional)</FieldLabel>
            <textarea
              value={detail}
              onChange={(e) => setDetail(e.target.value)}
              placeholder="why now / triggering condition / example…"
              rows={3}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void submit();
                }
              }}
              className="w-full rounded-md border border-ink-200 bg-white px-3 py-2 text-sm text-ink-900 focus:outline-none focus:ring-2 focus:ring-accent/30"
            />
          </section>

          {err && (
            <div className="rounded-md border border-danger bg-danger-dim px-3 py-2 text-xs text-danger">
              {err}
            </div>
          )}
        </div>

        <footer className="flex items-center justify-between border-t border-ink-200 px-5 h-12 bg-ink-100/40 rounded-b-xl">
          <p className="text-2xs text-ink-500">marked manual · trust = 1.0</p>
          <div className="flex items-center gap-2">
            <BtnSecondary onClick={onClose} disabled={submitting}>
              cancel
            </BtnSecondary>
            <button
              type="button"
              onClick={() => void submit()}
              disabled={submitting || name.trim().length === 0}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-md bg-ink-900 px-3 h-8 text-xs font-medium text-white",
                "hover:bg-ink-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors",
              )}
            >
              {submitting ? "creating…" : "Create →"}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}

function FieldLabel({ children }: { children: React.ReactNode }) {
  return (
    <span className="block text-2xs uppercase tracking-caps font-semibold text-ink-500">
      {children}
    </span>
  );
}

function TypeChip({
  type,
  active,
  onClick,
}: {
  type: (typeof ENTITY_TYPES)[number];
  active: boolean;
  onClick: () => void;
}) {
  const fill = ENTITY_FILL[type];
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full px-2.5 h-7 text-2xs font-medium border transition-colors",
        active
          ? cn(fill, "text-white border-transparent")
          : "bg-white text-ink-700 border-ink-200 hover:border-ink-300",
      )}
    >
      {!active && (
        <span className={cn("h-1.5 w-1.5 rounded-full", ENTITY_DOT_COLOR[type])} />
      )}
      <span className="lowercase">{type}</span>
    </button>
  );
}

const ENTITY_FILL: Record<(typeof ENTITY_TYPES)[number], string> = {
  Person: "bg-entity-person",
  Event: "bg-entity-event",
  Preference: "bg-entity-preference",
  Belief: "bg-entity-belief",
  Goal: "bg-entity-goal",
  Asset: "bg-entity-asset",
  Skill: "bg-entity-skill",
  Location: "bg-entity-location",
};

function Chip({
  active,
  onClick,
  label,
  count,
  dotClass,
  dark,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  count: number;
  dotClass?: string;
  dark?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full px-2.5 h-6 text-2xs font-medium border transition-colors",
        active && dark
          ? "bg-ink-900 text-white border-ink-900"
          : active
            ? "bg-ink-900 text-white border-ink-900"
            : "bg-ink-100 text-ink-900 border-ink-200 hover:border-ink-300",
      )}
    >
      {dotClass && <span className={cn("h-1.5 w-1.5 rounded-sm", dotClass)} />}
      <span className="font-medium">{label}</span>
      <span
        className={cn(
          "font-mono",
          active ? "text-white/80" : "text-ink-400",
        )}
      >
        {count}
      </span>
    </button>
  );
}

// Card variant used by the solo grid + the theme list. Reads
// `manual_edits` from the RankedEntity row so we can render the
// accent border + corrections affordance without an extra IPC.
function EntityCard({ row }: { row: RankedEntity }) {
  const { entity, final_score, manual_edits } = row;
  const props = safeJsonParse<Record<string, unknown>>(entity.properties, {});
  const snippet = Object.entries(props)
    .map(([k, v]) => `${k}: ${truncate(formatValue(v), 40)}`)
    .slice(0, 1)
    .join("");
  const hashSeed = (entity.source_note_id ?? entity.id).replace(/[^a-f0-9]/gi, "");
  const hash = hashSeed.length >= 8 ? hashSeed.slice(0, 8) : hashSeed.padEnd(8, "0");
  const corrected = manual_edits > 0;
  const archived = entity.archived_at != null;
  const trustBoost = Math.min(manual_edits, MANUAL_EDIT_CAP) * 10;
  return (
    <Link
      href={`/entity?id=${encodeURIComponent(entity.id)}`}
      className={cn(
        "block rounded-lg border bg-white p-4 transition-all hover:shadow-sm",
        corrected ? "border-accent" : "border-ink-200 hover:border-ink-300",
        archived && "opacity-60",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <EntityTag type={entity.type} small />
          {archived && (
            <StatusPill tone="muted" className="!h-5 !px-1.5">
              archived
            </StatusPill>
          )}
        </div>
        <span className="font-mono text-2xs text-ink-400 shrink-0 tabular-nums">
          {final_score.toFixed(2)}
        </span>
      </div>
      <h3 className="mt-2 text-md font-medium text-ink-900 leading-snug line-clamp-2">
        {entity.name}
      </h3>
      {corrected ? (
        <p className="mt-1 text-xs text-accent-fg leading-relaxed line-clamp-2">
          <span className="font-mono">
            ✎ {manual_edits} corrections · trust +{trustBoost}%
          </span>
        </p>
      ) : snippet ? (
        <p className="mt-1 text-xs text-ink-500 leading-relaxed line-clamp-2">
          {snippet}
        </p>
      ) : null}
      <div className="mt-3 flex items-center justify-between gap-2">
        <HashChip hash={hash} />
        <div className="font-mono text-2xs text-ink-400 flex items-center gap-2">
          <span>{entity.access_count}×</span>
          <span className="text-ink-300">·</span>
          <span>{relativeTime(entity.updated_at)}</span>
        </div>
      </div>
    </Link>
  );
}

function PlusIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

function formatValue(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return JSON.stringify(v);
}
