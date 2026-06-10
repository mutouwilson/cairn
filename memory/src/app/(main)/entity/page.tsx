"use client";
import { useEffect, useState, Suspense } from "react";
import { useSearchParams } from "next/navigation";
import Link from "next/link";
import {
  getEntity,
  listEntityAliases,
  listEntityEdits,
  removeEntityAlias,
  updateEntity,
} from "@/lib/tauri";
import type { AliasRow, EntityDetail, EntityEditRow } from "@/lib/types";
import { relativeTime, safeJsonParse } from "@/lib/utils";
import { cn } from "@/lib/utils";
import {
  BtnGhost,
  BtnSecondary,
  Card,
  EntityTag,
  HashChip,
  SectionEyebrow,
  StatusPill,
} from "@/components/ui";

// Caps the retrieval-boost pill so the "+N% retrieval boost" displayed
// in the eyebrow row matches the multiplicative trust boost the
// retrieval layer applies (MANUAL_EDIT_CAP = 3 on the backend).
const MANUAL_EDIT_CAP = 3;

export default function Page() {
  return (
    <Suspense fallback={<p className="p-8 text-sm text-ink-500">loading…</p>}>
      <EntityDetailInner />
    </Suspense>
  );
}

function EntityDetailInner() {
  const params = useSearchParams();
  const id = params.get("id");
  const [data, setData] = useState<EntityDetail | null>(null);
  const [edits, setEdits] = useState<EntityEditRow[]>([]);
  const [aliases, setAliases] = useState<AliasRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [delArmed, setDelArmed] = useState(false);
  const [editing, setEditing] = useState(false);

  // Refetch the entity after an unmerge can clear `is_sticky` server-side
  // (when no aliases AND no manual edits remain). Bumping this counter
  // re-runs the `getEntity` effect without re-reading the URL param.
  const [reloadTick, setReloadTick] = useState(0);

  useEffect(() => {
    if (!id) return;
    getEntity(id)
      .then(setData)
      .catch((e) => setError((e as Error).message));
  }, [id, reloadTick]);

  // Pull the edit history alongside the entity so the timeline + badge
  // can render in one frame. Refetches when `editing` flips back to
  // false because a successful save bumps the history table.
  useEffect(() => {
    if (!id) return;
    if (editing) return;
    listEntityEdits({ entity_id: id, limit: 50 })
      .then(setEdits)
      .catch(() => setEdits([]));
  }, [id, editing]);

  // Pull alias rows for the Merge Aliases section. Refetches alongside
  // the entity so a re-merge from elsewhere shows up on next render.
  useEffect(() => {
    if (!id) return;
    listEntityAliases({ entity_id: id })
      .then(setAliases)
      .catch(() => setAliases([]));
  }, [id, reloadTick]);

  if (!id) return <p className="p-8 text-sm text-ink-500">missing id.</p>;
  if (error) return <p className="p-8 text-sm text-danger">{error}</p>;
  if (!data) return <p className="p-8 text-sm text-ink-500">loading…</p>;

  const { entity, relations } = data;
  const props = safeJsonParse<Record<string, unknown>>(entity.properties, {});
  const propValues = Object.entries(props);
  const summary =
    propValues.length > 0 ? propValues.map(([, v]) => formatValue(v)).join(" · ") : entity.name;

  async function copyHash() {
    try {
      await navigator.clipboard.writeText(entity.id);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* ignore */
    }
  }

  function handleDelete() {
    if (!delArmed) {
      setDelArmed(true);
      setTimeout(() => setDelArmed(false), 4000);
      return;
    }
    // delete IPC isn't wired into the page; the armed-state is a visual stub.
    setDelArmed(false);
  }

  return (
    <div className="max-w-4xl mx-auto px-8 py-8 space-y-5">
      <div className="flex items-center justify-between">
        <Link
          href="/entities"
          className="inline-flex items-center gap-2 text-2xs text-ink-500 hover:text-ink-900"
        >
          <Arrow direction="left" /> back to entities
        </Link>
        <div className="flex items-center gap-2">
          {!editing && (
            <BtnGhost onClick={() => setEditing(true)} title="Correct the AI">
              <PencilIcon /> correct
            </BtnGhost>
          )}
          <BtnSecondary onClick={copyHash}>
            <CopyIcon /> {copied ? "copied" : "copy hash"}
          </BtnSecondary>
          <button
            type="button"
            onClick={handleDelete}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md border h-8 px-3 text-xs font-medium transition-colors",
              delArmed
                ? "border-danger bg-danger text-white"
                : "border-ink-200 bg-white text-danger hover:bg-danger-dim",
            )}
          >
            <TrashIcon />
            {delArmed ? "click again to delete" : "delete"}
          </button>
        </div>
      </div>

      {editing ? (
        <EntityEditCard
          entity={entity}
          props={props}
          onCancel={() => setEditing(false)}
          onSaved={(updated) => {
            setData({ entity: updated, relations });
            setEditing(false);
          }}
        />
      ) : (
        <Card className="rounded-xl p-6 space-y-4">
          <div className="flex items-center flex-wrap gap-3">
            <EntityTag type={entity.type} />
            {edits.length > 0 && <CorrectedBadge count={edits.length} />}
            {entity.is_sticky === 1 && <StickyPill />}
            <HashChip hash={entity.id} />
            <span className="text-2xs text-ink-400">
              created {relativeTime(entity.created_at)} · last accessed {relativeTime(entity.last_accessed)}
            </span>
          </div>
          <h1 className="font-display text-2xl font-semibold leading-tight text-ink-900">
            {summary || entity.name}
          </h1>
          <div className="flex flex-wrap gap-x-10 gap-y-4 pt-2">
            <Meta label="importance">
              <div className="flex items-center gap-2">
                <span className="font-mono text-md font-semibold text-ink-900">
                  {entity.base_importance.toFixed(2)}
                </span>
                <span className="relative inline-block h-1 w-20 rounded-full bg-ink-100 overflow-hidden">
                  <span
                    className="absolute inset-y-0 left-0 bg-ink-900"
                    style={{ width: `${Math.min(100, entity.base_importance * 100)}%` }}
                  />
                </span>
              </div>
            </Meta>
            <Meta label="access">
              <span className="font-mono text-xs text-ink-900">
                {entity.access_count} reads
              </span>
            </Meta>
            <Meta label="confidence">
              <span className="font-mono text-xs text-accent-fg">
                {entity.confidence.toFixed(2)}
              </span>
            </Meta>
            <Meta label="strength">
              <span className="font-mono text-xs text-ink-900">
                {entity.strength.toFixed(2)}
              </span>
            </Meta>
          </div>
        </Card>
      )}

      {propValues.length > 0 && (
        <section className="space-y-3">
          <SectionEyebrow>properties</SectionEyebrow>
          <Card>
            <pre className="text-xs font-mono whitespace-pre-wrap text-ink-700">
              {JSON.stringify(props, null, 2)}
            </pre>
          </Card>
        </section>
      )}

      <section className="space-y-3">
        <div className="flex items-end justify-between">
          <div className="flex items-baseline gap-3">
            <h2 className="font-display text-lg font-semibold text-ink-900">
              Relations
            </h2>
            <span className="font-mono text-2xs text-ink-400">
              {relations.length} connected {relations.length === 1 ? "entity" : "entities"}
            </span>
          </div>
        </div>
        {relations.length === 0 ? (
          <Card className="text-sm text-ink-500">no relations.</Card>
        ) : (
          <div className="rounded-lg border border-ink-200 bg-white overflow-hidden">
            {relations.map((r, idx) => {
              const isOut = r.from_entity === entity.id;
              const otherId = isOut ? r.to_entity : r.from_entity;
              return (
                <div
                  key={r.id}
                  className={cn(
                    "flex items-center gap-3 px-3 py-3.5",
                    idx > 0 && "border-t border-ink-200",
                  )}
                >
                  <span className="text-2xs text-ink-500 lowercase shrink-0">
                    {r.type}
                  </span>
                  <Arrow direction="right" />
                  <Link
                    href={`/entity?id=${encodeURIComponent(otherId)}`}
                    className="flex-1 min-w-0 text-sm font-mono text-ink-900 truncate hover:text-accent-fg"
                  >
                    {otherId}
                  </Link>
                  <HashChip hash={otherId} />
                </div>
              );
            })}
          </div>
        )}
      </section>

      <AliasesSection
        aliases={aliases}
        onUnmerged={(aliasId) => {
          setAliases((rows) => rows.filter((r) => r.id !== aliasId));
          // Re-fetch the entity so the sticky pill drops if the backend
          // auto-cleared `is_sticky` (no aliases + no manual edits left).
          setReloadTick((t) => t + 1);
        }}
      />

      <EditHistorySection edits={edits} />

      <div>
        <BtnGhost>
          <Arrow direction="left" />
          back to entities
        </BtnGhost>
      </div>
    </div>
  );
}

// Stable badge for "this entity has been corrected N times" — shown in
// the entity header row when count > 0, and on every grid card. Visually
// matches Pencil mockup `ixw71`.
function CorrectedBadge({ count }: { count: number }) {
  return (
    <span className="inline-flex items-center gap-1 px-1.5 py-px rounded-sm bg-accent-dim border border-accent text-accent-fg text-[10px] font-mono font-semibold">
      <span aria-hidden>{"✎"}</span>
      corrected {count}&times;
    </span>
  );
}

// Sticky pill shown in the entity header when `is_sticky === 1`. Marks
// entities that survive import-watcher cascades (manual create, manual
// edit, or merge-keep). Matches Pencil mockup `m4Buyg` — ink-primary
// fill, white text. Cleared by the backend when the last alias is
// unmerged AND there are no manual edits.
function StickyPill() {
  return (
    <span className="inline-flex items-center gap-1 px-1.5 py-px rounded-sm bg-ink-900 text-white text-[10px] font-mono font-semibold">
      <span aria-hidden>{"🔒"}</span>
      sticky
    </span>
  );
}

// Merge Aliases section per Pencil mockup `m4Buyg`. One row per
// `entity_aliases` table entry. Unmerge button uses the 2-click armed
// pattern (no `confirm()` per design constraints). Hidden entirely when
// the entity has no aliases.
function AliasesSection({
  aliases,
  onUnmerged,
}: {
  aliases: AliasRow[];
  onUnmerged: (aliasId: string) => void;
}) {
  if (aliases.length === 0) return null;
  return (
    <section className="space-y-3">
      <Card className="border-l-[3px] border-l-accent">
        <header className="flex items-start justify-between gap-4 pb-3 border-b border-ink-200">
          <SectionEyebrow>
            MERGE ALIASES &middot; {aliases.length} {aliases.length === 1 ? "route" : "routes"} here
          </SectionEyebrow>
          <span className="text-2xs text-ink-500 max-w-xs text-right">
            new extractions matching these will route into this entity
          </span>
        </header>
        <div className="space-y-2 pt-3">
          {aliases.map((row) => (
            <AliasRowItem key={row.id} row={row} onUnmerged={onUnmerged} />
          ))}
        </div>
        <p className="pt-3 mt-3 border-t border-ink-200 text-2xs text-warn">
          <span aria-hidden>⚠ </span>
          unmerge does NOT recreate the dropped entity &middot; it just stops
          future extractions from routing here.
        </p>
      </Card>
    </section>
  );
}

function AliasRowItem({
  row,
  onUnmerged,
}: {
  row: AliasRow;
  onUnmerged: (aliasId: string) => void;
}) {
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function handleClick() {
    if (!armed) {
      setArmed(true);
      setTimeout(() => setArmed(false), 4000);
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await removeEntityAlias({ alias_id: row.id });
      onUnmerged(row.id);
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
      setArmed(false);
    }
  }

  // The backend doesn't return per-alias cosine — omit it from the meta
  // line rather than fake a number. Spec is explicit on this.
  const metaParts = [
    `merged ${relativeTime(row.created_at)}`,
    `model ${row.embedding_model_id || "—"}`,
  ];
  if (row.routed_count > 0) {
    metaParts.push(`routed ${row.routed_count}× since`);
  }

  return (
    <div className="flex items-center gap-3 rounded-md border border-ink-200 bg-white px-3 py-2">
      <div className="flex-1 min-w-0 space-y-0.5">
        <div className="text-sm font-medium text-ink-900 truncate">
          {row.name_lower}
        </div>
        <div className="text-2xs text-ink-500 truncate">
          {metaParts.join(" · ")}
        </div>
        {err && <div className="text-2xs text-danger">{err}</div>}
      </div>
      <button
        type="button"
        onClick={() => void handleClick()}
        disabled={busy}
        className={cn(
          "inline-flex items-center gap-1 rounded-md border h-7 px-2.5 text-2xs font-medium shrink-0 transition-colors",
          armed
            ? "border-danger bg-danger text-white"
            : "border-danger bg-white text-danger hover:bg-danger-dim",
          busy && "opacity-40 cursor-not-allowed",
        )}
      >
        {busy ? "unmerging…" : armed ? "click again to unmerge" : "unmerge"}
      </button>
    </div>
  );
}

// Edit history timeline per Pencil mockup `kGqoh`. Hidden entirely
// when there's nothing to show — no "no edits yet" placeholder.
function EditHistorySection({ edits }: { edits: EntityEditRow[] }) {
  if (edits.length === 0) return null;
  const boost = Math.min(edits.length, MANUAL_EDIT_CAP) * 10;
  return (
    <section className="space-y-3">
      <Card>
        <header className="flex items-center justify-between gap-3 pb-3 border-b border-ink-200">
          <SectionEyebrow>
            EDIT HISTORY &middot; {edits.length}{" "}
            {edits.length === 1 ? "correction" : "corrections"} by you
          </SectionEyebrow>
          <StatusPill tone="accent">+{boost}% retrieval boost</StatusPill>
        </header>
        <div className="divide-y divide-ink-200">
          {edits.map((row) => (
            <EditHistoryRow key={row.id} row={row} />
          ))}
        </div>
      </Card>
    </section>
  );
}

function EditHistoryRow({ row }: { row: EntityEditRow }) {
  const action = row.action.toUpperCase();
  const tone: "accent" | "muted" | "danger" =
    action === "UPDATE" ? "accent" : action === "DELETE" ? "danger" : "muted";
  return (
    <div className="flex items-start gap-4 py-3">
      <div className="w-24 shrink-0">
        <div className="text-xs font-semibold text-ink-900">
          {relativeTime(row.edited_at)}
        </div>
        <div className="text-2xs font-mono text-ink-400">
          {formatHM(row.edited_at)}
        </div>
      </div>
      <div className="flex-1 min-w-0 space-y-2">
        <div className="flex items-center gap-2 flex-wrap">
          <StatusPill tone={tone}>{action}</StatusPill>
          {row.field_changed && (
            <span className="font-mono text-xs text-ink-700">
              {row.field_changed}
            </span>
          )}
        </div>
        {row.action === "update" ? (
          <DiffRows before={row.value_before} after={row.value_after} />
        ) : (
          <p className="text-xs text-ink-500">
            {row.action === "create"
              ? "created manually · entity surfaced into memory store"
              : "deleted manually · entity removed from memory store"}
          </p>
        )}
      </div>
    </div>
  );
}

function DiffRows({
  before,
  after,
}: {
  before: string | null;
  after: string | null;
}) {
  return (
    <div className="space-y-1">
      {before !== null && (
        <DiffLine sign="-" value={before} tone="danger" />
      )}
      {after !== null && <DiffLine sign="+" value={after} tone="accent" />}
    </div>
  );
}

function DiffLine({
  sign,
  value,
  tone,
}: {
  sign: "+" | "-";
  value: string;
  tone: "accent" | "danger";
}) {
  // Multi-line properties JSON arrives verbatim (server pre-truncates
  // to 1024 chars at write time, so we render whatever's in the row
  // and let `whitespace-pre-wrap` handle line breaks instead of trying
  // to be clever with a JSON-aware folder).
  return (
    <div
      className={cn(
        "flex gap-2 rounded-sm px-2 py-1.5 font-mono text-xs",
        tone === "accent"
          ? "bg-accent-dim text-accent-fg"
          : "bg-danger-dim text-danger",
      )}
    >
      <span className="select-none shrink-0">{sign}</span>
      <span className="min-w-0 whitespace-pre-wrap break-all">{value}</span>
    </div>
  );
}

function formatHM(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function EntityEditCard({
  entity,
  props,
  onCancel,
  onSaved,
}: {
  entity: EntityDetail["entity"];
  props: Record<string, unknown>;
  onCancel: () => void;
  onSaved: (updated: EntityDetail["entity"]) => void;
}) {
  // We render either a `statement` form (the happy path the design assumes) or a raw
  // JSON fallback when properties hold something other than a string statement.
  const initialStatement = typeof props.statement === "string" ? props.statement : "";
  const hasStringStatement = typeof props.statement === "string";
  const hasOtherProps = Object.keys(props).some((k) => k !== "statement");
  // Fall back to raw JSON only if there's data we can't surface as a "statement".
  const rawMode = hasOtherProps && !hasStringStatement;

  const [name, setName] = useState(entity.name);
  const [statement, setStatement] = useState(initialStatement);
  const [rawJson, setRawJson] = useState(() =>
    rawMode ? JSON.stringify(props, null, 2) : entity.properties,
  );
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function save() {
    setSaving(true);
    setErr(null);
    try {
      let propertiesJson: string | undefined;
      if (rawMode) {
        try {
          JSON.parse(rawJson);
        } catch {
          throw new Error("properties is not valid JSON");
        }
        propertiesJson = rawJson;
      } else {
        // Replace statement; preserve any other keys verbatim.
        const next: Record<string, unknown> = { ...props };
        const trimmed = statement.trim();
        if (trimmed.length > 0) next.statement = statement;
        else delete next.statement;
        propertiesJson = JSON.stringify(next);
      }

      const trimmedName = name.trim();
      const updated = await updateEntity({
        id: entity.id,
        name: trimmedName !== entity.name ? trimmedName : undefined,
        properties: propertiesJson !== entity.properties ? propertiesJson : undefined,
      });
      onSaved(updated);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="rounded-xl border border-ink-200 bg-ink-100/40 border-l-4 border-l-accent p-6 space-y-4">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="flex items-center gap-3 flex-wrap">
          <EntityTag type={entity.type} />
          <HashChip hash={entity.id} />
        </div>
        <SectionEyebrow className="text-accent-fg">correct the AI</SectionEyebrow>
      </div>

      <div className="space-y-3">
        <Field label="name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            className="w-full rounded-md border border-accent bg-white px-3 h-9 text-sm text-ink-900 focus:outline-none focus:ring-2 focus:ring-accent/30"
          />
        </Field>
        {rawMode ? (
          <Field label="properties (JSON)">
            <textarea
              value={rawJson}
              onChange={(e) => setRawJson(e.target.value)}
              rows={6}
              className="w-full rounded-md border border-accent bg-white px-3 py-2 text-xs font-mono text-ink-900 focus:outline-none focus:ring-2 focus:ring-accent/30"
            />
          </Field>
        ) : (
          <Field label="statement">
            <textarea
              value={statement}
              onChange={(e) => setStatement(e.target.value)}
              rows={3}
              className="w-full rounded-md border border-accent bg-white px-3 py-2 text-sm text-ink-900 focus:outline-none focus:ring-2 focus:ring-accent/30"
            />
          </Field>
        )}
      </div>

      {err && (
        <div className="rounded-md border border-danger bg-danger-dim px-3 py-2 text-xs text-danger">
          {err}
        </div>
      )}

      <div className="flex items-center justify-between gap-3 pt-1">
        <p className="text-2xs text-ink-500">
          saves trigger re-embed · audit logged
        </p>
        <div className="flex items-center gap-2">
          <BtnSecondary onClick={onCancel} disabled={saving}>
            cancel
          </BtnSecondary>
          <button
            type="button"
            onClick={() => void save()}
            disabled={saving || name.trim().length === 0}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md h-8 px-3 text-xs font-medium text-white transition-colors",
              "bg-accent hover:bg-accent-fg disabled:opacity-40 disabled:cursor-not-allowed",
            )}
          >
            {saving ? "saving…" : "save correction"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <span className="text-2xs uppercase tracking-caps font-semibold text-ink-500">
        {label}
      </span>
      {children}
    </label>
  );
}

function Meta({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <SectionEyebrow>{label}</SectionEyebrow>
      <div>{children}</div>
    </div>
  );
}

function Arrow({ direction }: { direction: "left" | "right" }) {
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

function CopyIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}

function PencilIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.121 2.121 0 1 1 3 3L7 19l-4 1 1-4 12.5-12.5z" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 6h18 M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2 M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
    </svg>
  );
}

function formatValue(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return JSON.stringify(v);
}
