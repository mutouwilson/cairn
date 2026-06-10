"use client";
import { useEffect, useState } from "react";
import {
  exportPreview,
  exportRun,
  importClaudeAutoRun,
  importResyncFile,
  importSetSkip,
  onImportChanged,
  readImportSourceFile,
} from "@/lib/tauri";
import {
  EXPORT_CONFLICT_LABEL,
  IMPORT_SOURCE_LABEL,
  type ExportPreview as ExportPreviewT,
  type ExportResult,
  type ImportDocStatus,
  type ImportSummary,
  type ImportedDocSummary,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { BtnSecondary, Card, SectionEyebrow, StatusPill } from "@/components/ui";

export default function ImportPage() {
  const t = useT();
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [mode, setMode] = useState<"preview" | "apply">("preview");
  const [armed, setArmed] = useState(false);
  const [autoBanner, setAutoBanner] = useState<{ imported: number; updated: number } | null>(null);
  // Unlinked files the user checked to restore on the next Apply. Transient
  // (cleared once Apply re-imports them). "Skip" is persisted server-side,
  // so it deliberately does NOT live in React state.
  const [unlinkedSelected, setUnlinkedSelected] = useState<Set<string>>(new Set());

  function toggleUnlinked(path: string) {
    setUnlinkedSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  // Persist a file's skip flag (unchecking a synced/new row = skip; checking
  // a SKIPPED row = unskip), then re-scan so its row + counts refresh.
  async function setSkip(path: string, skipped: boolean) {
    setBusy(true);
    setErr(null);
    try {
      await importSetSkip(path, skipped);
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
      return;
    }
    await run(true); // run()'s finally clears busy
  }

  useEffect(() => {
    let off: (() => void) | null = null;
    let bannerTimer: ReturnType<typeof setTimeout> | null = null;
    (async () => {
      try {
        off = await onImportChanged((s) => {
          // Watcher already wrote everything — surface the result without
          // forcing the user to click "Apply" again.
          setSummary(s);
          setMode("apply");
          setAutoBanner({ imported: s.imported, updated: s.updated });
          if (bannerTimer) clearTimeout(bannerTimer);
          bannerTimer = setTimeout(() => setAutoBanner(null), 6000);
        });
      } catch {
        /* not running inside Tauri */
      }
    })();
    return () => {
      off?.();
      if (bannerTimer) clearTimeout(bannerTimer);
    };
  }, []);

  // Auto-scan on mount so opening the page reflects the current on-disk
  // state (including files whose note was just deleted → Unlinked) without
  // a manual Preview click. Dry-run only — no writes, no extraction.
  useEffect(() => {
    void run(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function run(dryRun: boolean) {
    setBusy(true);
    setErr(null);
    setMode(dryRun ? "preview" : "apply");
    try {
      const s = await importClaudeAutoRun({
        dry_run: dryRun,
        // On Apply, force-restore the Unlinked rows the user checked.
        force_paths: dryRun ? [] : Array.from(unlinkedSelected),
      });
      setSummary(s);
      if (!dryRun) setUnlinkedSelected(new Set()); // restored → now SYNCED
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
      setArmed(false);
    }
  }

  function handleImportClick() {
    if (!armed) {
      setArmed(true);
      setTimeout(() => setArmed(false), 4000);
      return;
    }
    void run(false);
  }

  // Force-reimport a single file (per-row "re-sync"): pulls back an Unlinked
  // file or re-runs extraction on an unchanged one, then re-scans so the row
  // and counts refresh.
  async function handleResync(path: string) {
    setBusy(true);
    setErr(null);
    try {
      await importResyncFile(path);
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
      return;
    }
    await run(true); // run()'s finally clears busy
  }

  const totalFiles = summary?.scanned ?? 0;
  // Dry-run tags new files `status: "dry-run"`; the Apply count is those plus
  // any Unlinked rows the user checked to restore. After a real Apply, fall
  // back to imported+updated.
  const dryRunDocs = summary
    ? summary.docs.filter((d) => d.status === "dry-run").length
    : 0;
  const wouldImport = summary
    ? mode === "preview"
      ? dryRunDocs + unlinkedSelected.size
      : summary.imported + summary.updated
    : 0;
  // Unlinked rows + a select-all affordance for batch restore.
  const unlinkedPaths = summary
    ? summary.docs.filter((d) => d.status === "unlinked").map((d) => d.source_path)
    : [];
  const allUnlinkedSelected =
    unlinkedPaths.length > 0 && unlinkedPaths.every((p) => unlinkedSelected.has(p));
  function toggleAllUnlinked() {
    setUnlinkedSelected(allUnlinkedSelected ? new Set() : new Set(unlinkedPaths));
  }
  const failed = summary?.failed ?? 0;
  const kindBreakdown = summary ? makeKindBreakdown(summary.docs) : null;
  const showStartGate = summary == null && !busy;

  return (
    <div className="max-w-4xl mx-auto px-8 py-8 space-y-5">
      <header className="space-y-2">
        <div className="flex items-end gap-2.5">
          <h1 className="font-display text-xl font-semibold text-ink-900">
            {t("import.title")}
          </h1>
          <span className="font-mono text-2xs text-accent-fg uppercase tracking-caps">
            v6 · beta
          </span>
        </div>
        <p className="text-md text-ink-500 leading-relaxed max-w-prose">
          Pull your Claude Code memory files into Cairn — one source of truth that
          every tool can read. Each file becomes one note; the extractor turns it
          into typed entities. Cairn watches{" "}
          <code className="font-mono text-ink-700">~/.claude</code> and re-imports
          automatically when files change.
        </p>
      </header>

      {autoBanner && (
        <div className="rounded-md border border-accent/30 bg-accent-dim px-3 py-2 text-xs text-accent-fg">
          Auto-imported from disk · {autoBanner.imported} new, {autoBanner.updated} updated.
        </div>
      )}

      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <StatBox
          eyebrow="FILES FOUND"
          value={summary ? String(totalFiles) : "—"}
          sub={kindBreakdown ?? "scan to discover memory files"}
        />
        <StatBox
          eyebrow={mode === "preview" ? "ENTITIES TO EXTRACT" : "FILES IMPORTED"}
          value={summary ? String(wouldImport) : "—"}
          sub={
            summary
              ? mode === "preview"
                ? `${summary.imported} new · ${summary.updated} update · ${summary.skipped_dup} unchanged`
                : `${summary.updated} updated · ${summary.skipped_dup} unchanged`
              : "click Preview to see what would change"
          }
        />
        <StatBox
          eyebrow="CONFLICTS"
          value={summary ? String(failed) : "—"}
          sub={
            failed > 0
              ? "need your review before apply"
              : "no conflicts detected"
          }
          tone={failed > 0 ? "warn" : "muted"}
        />
        <StatBox
          eyebrow="EXTRACT ENGINE"
          value="background"
          sub="async · runs after note insert"
          dark
        />
      </div>

      <section className="space-y-2">
        <div className="flex items-end justify-between gap-3">
          <h2 className="font-display text-lg font-semibold text-ink-900">
            Discovered files
          </h2>
          <div className="flex items-center gap-3">
            {unlinkedPaths.length > 0 && (
              <button
                type="button"
                onClick={toggleAllUnlinked}
                disabled={busy}
                className="text-2xs text-warn hover:text-warn/80 disabled:opacity-50"
              >
                {allUnlinkedSelected ? "取消全选" : `全选 ${unlinkedPaths.length} 个待恢复`}
              </button>
            )}
            <button
              type="button"
              onClick={() => void run(true)}
              disabled={busy}
              className="inline-flex items-center gap-1.5 text-2xs text-ink-500 hover:text-ink-900 disabled:opacity-50"
            >
              <RotateIcon /> {busy && mode === "preview" ? "scanning…" : "rescan"}
            </button>
          </div>
        </div>

        {err && (
          <div className="rounded-md border border-danger bg-danger-dim px-3 py-2 text-xs text-danger">
            {err}
          </div>
        )}

        {showStartGate ? (
          <Card className="text-sm text-ink-500 flex flex-col gap-2.5">
            <span>Sources scanned (all user-level only):</span>
            <ul className="text-xs text-ink-700 font-mono space-y-1 ml-2">
              <li>· <code>~/.claude/CLAUDE.md</code> — Claude Code global</li>
              <li>· <code>~/.claude/projects/&lt;encoded&gt;/memory/*.md</code> — Claude auto memory</li>
              <li>· <code>~/.cursor/skills-cursor/&lt;name&gt;/SKILL.md</code> — Cursor skills</li>
              <li>· <code>~/.cursor/plans/*.md</code> — Cursor plans</li>
              <li>· <code>~/.codex/memories/*.md</code> — Codex memory</li>
            </ul>
            <div className="flex justify-end">
              <BtnSecondary onClick={() => void run(true)}>Preview</BtnSecondary>
            </div>
          </Card>
        ) : busy && summary == null ? (
          <Card className="text-sm text-ink-500">scanning…</Card>
        ) : summary && summary.docs.length === 0 ? (
          <Card className="text-sm text-ink-500">
            No memory files found across any source (Claude / Cursor / Codex).
            Check that the relevant directories exist under <code className="text-xs">~/</code>.{" "}
            Original Claude Code message: you haven&rsquo;t used the memory tool yet, or{" "}
            <code className="text-xs">~/.claude</code> doesn&rsquo;t
            exist on this machine.
          </Card>
        ) : (
          summary && (
            <div className="rounded-lg border border-ink-200 bg-white overflow-hidden">
              {summary.docs.map((d, idx) => {
                const p = d.source_path;
                const st = d.status;
                // One checkbox = "sync this file?". New/synced default checked
                // (uncheck → persistent skip); skipped default unchecked (check
                // → unskip); unlinked default unchecked (check → restore on
                // Apply). Failed/empty get no checkbox.
                // Checkbox only means "bring this back into sync": on SKIPPED
                // (uncheck-state → check to unskip) and UNLINKED (check to
                // stage a restore). Synced/new files are NOT checkboxes — that
                // read as a status light; instead they get an explicit "skip"
                // button. So the only checked-by-default box is gone, which is
                // what confused the batch select-all.
                let checkbox: { checked: boolean; onToggle: () => void } | undefined;
                let onResync: (() => void) | undefined;
                let onSkip: (() => void) | undefined;
                if (st === "skipped") {
                  checkbox = { checked: false, onToggle: () => void setSkip(p, false) };
                } else if (st === "unlinked") {
                  checkbox = { checked: unlinkedSelected.has(p), onToggle: () => toggleUnlinked(p) };
                  onResync = () => void handleResync(p);
                  // Also let the user permanently ignore a deleted file so it
                  // stops showing up as UNLINKED every scan.
                  onSkip = () => void setSkip(p, true);
                } else if (st === "dry-run") {
                  onSkip = () => void setSkip(p, true);
                } else if (st === "unchanged" || st === "updated" || st === "imported") {
                  onSkip = () => void setSkip(p, true);
                  onResync = () => void handleResync(p);
                }
                return (
                  <FileRow
                    key={p + d.note_id + st}
                    doc={d}
                    first={idx === 0}
                    checkbox={checkbox}
                    onResync={onResync}
                    onSkip={onSkip}
                    busy={busy}
                  />
                );
              })}
            </div>
          )
        )}
      </section>

      {summary && (
        <Card className="flex items-center justify-between gap-4 rounded-lg">
          <div className="flex items-center gap-2.5 min-w-0">
            <InfoIcon />
            <div>
              <p className="text-xs font-medium text-ink-900">
                {mode === "preview"
                  ? "Dry-run · no writes until you click Import"
                  : "Import applied · extraction running in background"}
              </p>
              <p className="text-2xs text-ink-500">
                All operations recorded to audit chain
                {failed > 0 ? ` · ${failed} failure${failed === 1 ? "" : "s"}` : ""}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <BtnSecondary onClick={() => void run(true)} disabled={busy}>
              {busy && mode === "preview" ? "scanning…" : "Re-preview"}
            </BtnSecondary>
            <button
              type="button"
              onClick={handleImportClick}
              disabled={busy}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-md h-8 px-3 text-xs font-medium disabled:opacity-40 transition-colors",
                armed
                  ? "bg-danger text-white"
                  : "bg-ink-900 text-white hover:bg-ink-800",
              )}
            >
              {busy && mode === "apply"
                ? "importing…"
                : armed
                  ? `Click again to import ${wouldImport}`
                  : `Apply ${wouldImport} ready`}
              {!armed && !busy && <CheckIcon />}
            </button>
          </div>
        </Card>
      )}

      <div className="pt-2 border-t border-ink-200" />

      <ExportPanel />
    </div>
  );
}

function FileRow({
  doc,
  first,
  checkbox,
  onResync,
  onSkip,
  busy,
}: {
  doc: ImportedDocSummary;
  first: boolean;
  /** Sync toggle for this row. Undefined → no checkbox (failed/empty rows).
   *  Semantics depend on status (set/clear skip, or mark unlinked-to-restore);
   *  the parent decides what `onToggle` does. */
  checkbox?: { checked: boolean; onToggle: () => void };
  /** Force-reimport this file. Shown on already-tracked rows. */
  onResync?: () => void;
  /** Mark this file "do not sync" (persistent). Shown on syncable rows
   *  (new / synced) — the explicit alternative to a default-checked box. */
  onSkip?: () => void;
  busy?: boolean;
}) {
  const style = STATUS_STYLE[doc.status];
  const muted = checkbox != null && !checkbox.checked; // unchecked → de-emphasize
  const [open, setOpen] = useState(false);
  const [content, setContent] = useState<string | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleToggleOpen() {
    const next = !open;
    setOpen(next);
    if (next && content == null && !loading) {
      setLoading(true);
      setLoadErr(null);
      try {
        const text = await readImportSourceFile(doc.source_path);
        setContent(text);
      } catch (e) {
        setLoadErr((e as Error).message);
      } finally {
        setLoading(false);
      }
    }
  }

  return (
    <div className={cn(!first && "border-t border-ink-200", doc.status === "failed" && "bg-warn-dim/40")}>
      <div
        className="flex items-center gap-3 px-3 py-3 text-sm cursor-pointer hover:bg-ink-50/60"
        onClick={() => void handleToggleOpen()}
      >
        {checkbox ? (
          <input
            type="checkbox"
            checked={checkbox.checked}
            disabled={busy}
            onChange={checkbox.onToggle}
            onClick={(e) => e.stopPropagation()}
            className="h-4 w-4 rounded border-ink-300 accent-accent cursor-pointer disabled:opacity-50"
            aria-label={`sync ${doc.source_path}`}
          />
        ) : (
          <FileIcon />
        )}
        <div className="flex-1 min-w-0">
          <div className={cn("font-mono text-xs truncate", muted ? "text-ink-500" : "text-ink-900")}>
            {doc.source_path}
          </div>
          <div className="text-2xs text-ink-400 mt-0.5">
            {IMPORT_SOURCE_LABEL[doc.source_kind] ?? doc.source_kind}
            {doc.bytes > 0 && <> · {doc.bytes} bytes</>}
            {doc.display_name && (
              <>
                {" · "}
                <span className="text-ink-500">{doc.display_name}</span>
              </>
            )}
            {doc.note_id && doc.note_id !== "(dry-run)" && (
              <>
                {" · note "}
                <code className="font-mono text-ink-500">{doc.note_id.slice(0, 8)}</code>
              </>
            )}
          </div>
        </div>
        {onSkip && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onSkip();
            }}
            disabled={busy}
            title="跳过 — 不同步此文件（可随时勾选恢复）"
            aria-label="skip this file"
            className="h-7 w-7 grid place-items-center rounded-full text-ink-400 hover:text-ink-700 hover:bg-ink-100 disabled:opacity-40 transition-colors shrink-0"
          >
            <EyeOffIcon />
          </button>
        )}
        {onResync && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onResync();
            }}
            disabled={busy}
            title="重新同步 — 强制重新导入此文件（忽略去重）"
            aria-label="re-sync this file"
            className="h-7 w-7 grid place-items-center rounded-full text-ink-400 hover:text-ink-700 hover:bg-ink-100 disabled:opacity-40 transition-colors shrink-0"
          >
            <RotateIcon />
          </button>
        )}
        <span
          className={cn(
            "inline-flex items-center rounded-sm px-2 h-5 font-mono text-2xs font-semibold tracking-caps",
            style.cls,
          )}
        >
          {style.label}
        </span>
        <span className={cn("transition-transform inline-flex", open && "rotate-90")}>
          <ChevronIcon />
        </span>
      </div>
      {open && (
        <div className="px-3 pb-3 bg-ink-50/60 border-t border-ink-100">
          {loading && <p className="text-2xs text-ink-500 py-2">loading…</p>}
          {loadErr && (
            <p className="text-2xs text-danger py-2">read failed: {loadErr}</p>
          )}
          {content != null && (
            <pre className="mt-2 max-h-80 overflow-auto rounded border border-ink-200 bg-white p-3 text-2xs font-mono leading-relaxed text-ink-700 whitespace-pre-wrap break-words">
              {content}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

function ExportPanel() {
  const [preview, setPreview] = useState<ExportPreviewT | null>(null);
  const [result, setResult] = useState<ExportResult | null>(null);
  const [busy, setBusy] = useState<"preview" | "apply" | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [armed, setArmed] = useState(false);
  const [forceArmed, setForceArmed] = useState(false);

  async function runPreview() {
    setBusy("preview");
    setErr(null);
    setResult(null);
    try {
      const p = await exportPreview({});
      setPreview(p);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  async function runApply(force: boolean) {
    setBusy("apply");
    setErr(null);
    try {
      const r = await exportRun({ force });
      setResult(r);
      if (r.written) {
        const p = await exportPreview({});
        setPreview(p);
      }
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(null);
      setArmed(false);
      setForceArmed(false);
    }
  }

  function handleApplyClick() {
    if (!armed) {
      setArmed(true);
      setTimeout(() => setArmed(false), 4000);
      return;
    }
    void runApply(false);
  }

  function handleForceClick() {
    if (!forceArmed) {
      setForceArmed(true);
      setTimeout(() => setForceArmed(false), 4000);
      return;
    }
    void runApply(true);
  }

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <SectionEyebrow>export</SectionEyebrow>
        <h2 className="font-display text-lg font-semibold text-ink-900">
          Export to Claude CLAUDE.md
        </h2>
        <p className="text-xs text-ink-500 max-w-prose leading-relaxed">
          Write a Cairn-managed block back to{" "}
          <code className="font-mono text-ink-700">~/.claude/CLAUDE.md</code> so Claude
          Code sees your top Preferences / Goals / Beliefs at session start. The block
          is bracketed by{" "}
          <code className="font-mono text-ink-700">&lt;!-- cairn:start v1 --&gt;</code>{" "}
          /{" "}
          <code className="font-mono text-ink-700">&lt;!-- cairn:end --&gt;</code>;
          content outside is left untouched. If you edit inside, Cairn refuses to
          overwrite without explicit force.
        </p>
      </header>

      <div className="flex flex-wrap gap-2 items-center">
        <BtnSecondary onClick={() => void runPreview()} disabled={busy !== null}>
          {busy === "preview" ? "loading…" : "Preview"}
        </BtnSecondary>
        <button
          type="button"
          onClick={handleApplyClick}
          disabled={busy !== null}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-md h-8 px-3 text-xs font-medium disabled:opacity-40 transition-colors",
            armed
              ? "bg-danger text-white"
              : "bg-ink-900 text-white hover:bg-ink-800",
          )}
        >
          {busy === "apply"
            ? "writing…"
            : armed
              ? "Click again to write"
              : "Apply"}
        </button>
      </div>

      {err && (
        <div className="rounded-md border border-danger bg-danger-dim px-3 py-2 text-xs text-danger">
          {err}
        </div>
      )}

      {result && !result.written && result.conflict && (
        <div className="rounded-lg border border-warn/40 bg-warn-dim p-3 space-y-2">
          <div className="text-sm font-medium text-warn">
            Refused to overwrite — {EXPORT_CONFLICT_LABEL[result.conflict]}
          </div>
          <p className="text-xs text-ink-700 leading-relaxed">
            Re-capture your manual edits in Cairn (so they survive future
            syncs), then click <strong>Force Overwrite</strong> below.
          </p>
          <button
            type="button"
            onClick={handleForceClick}
            disabled={busy !== null}
            className={cn(
              "inline-flex items-center rounded-md h-8 px-3 text-xs font-medium border",
              forceArmed
                ? "border-danger bg-danger text-white"
                : "border-warn bg-white text-warn hover:bg-warn-dim",
            )}
          >
            {forceArmed ? "Click again to force" : "Force Overwrite"}
          </button>
        </div>
      )}

      {result && result.written && (
        <div className="rounded-md border border-accent/30 bg-accent-dim px-3 py-2 text-xs text-accent-fg">
          Wrote {result.bytes_written} bytes to{" "}
          <code className="font-mono">{result.dest_path}</code> · {result.entity_count}{" "}
          entities exported.
        </div>
      )}

      {preview && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <StatusPill tone="accent">
              {preview.entity_count} entities
            </StatusPill>
            <StatusPill tone="muted">
              {preview.proposed_block.length} bytes
            </StatusPill>
            {preview.had_block && (
              <StatusPill tone="muted">existing block on disk</StatusPill>
            )}
            {preview.conflict && (
              <StatusPill tone="warn" dot>
                {EXPORT_CONFLICT_LABEL[preview.conflict]}
              </StatusPill>
            )}
          </div>
          <div className="font-mono text-2xs text-ink-500 break-all">
            {preview.dest_path}
          </div>
          <div className="grid gap-3 md:grid-cols-2">
            {preview.had_block && (
              <BlockBox title="Current on disk" body={preview.current_block ?? ""} />
            )}
            <BlockBox title="Proposed block" body={preview.proposed_block} />
          </div>
        </div>
      )}
    </section>
  );
}

function BlockBox({ title, body }: { title: string; body: string }) {
  return (
    <div className="rounded-md border border-ink-200 overflow-hidden">
      <div className="text-2xs font-medium px-3 h-7 flex items-center border-b border-ink-200 bg-ink-100 text-ink-700">
        {title}
      </div>
      <pre className="text-2xs font-mono whitespace-pre-wrap p-3 max-h-72 overflow-auto leading-relaxed text-ink-700">
        {body || "(empty)"}
      </pre>
    </div>
  );
}

function StatBox({
  eyebrow,
  value,
  sub,
  tone,
  dark,
}: {
  eyebrow: string;
  value: string;
  sub: string;
  tone?: "warn" | "muted";
  dark?: boolean;
}) {
  return (
    <div
      className={cn(
        "rounded-lg border p-4 space-y-1.5",
        dark
          ? "bg-ink-900 text-white border-ink-900"
          : "bg-white border-ink-200",
      )}
    >
      <p
        className={cn(
          "text-2xs uppercase tracking-caps font-semibold",
          dark ? "text-ink-400" : "text-ink-400",
        )}
      >
        {eyebrow}
      </p>
      <p
        className={cn(
          "font-display text-2xl font-semibold leading-none",
          tone === "warn" ? "text-warn" : dark ? "text-white" : "text-ink-900",
        )}
      >
        {value}
      </p>
      <p
        className={cn(
          "font-mono text-2xs",
          tone === "warn" ? "text-warn" : "text-ink-400",
        )}
      >
        {sub}
      </p>
    </div>
  );
}

function makeKindBreakdown(docs: ImportedDocSummary[]): string {
  const counts = new Map<string, number>();
  for (const d of docs) counts.set(d.source_kind, (counts.get(d.source_kind) ?? 0) + 1);
  return Array.from(counts.entries())
    .map(
      ([k, n]) =>
        `${n} ${IMPORT_SOURCE_LABEL[k as keyof typeof IMPORT_SOURCE_LABEL] ?? k}`,
    )
    .join(" · ");
}

const STATUS_STYLE: Record<ImportDocStatus, { label: string; cls: string }> = {
  imported: {
    label: "NEW",
    cls: "bg-accent-dim text-accent-fg",
  },
  updated: {
    label: "UPDATED",
    cls: "bg-accent-dim text-accent-fg",
  },
  unchanged: {
    label: "SYNCED",
    cls: "bg-ink-100 text-ink-500 border border-ink-200",
  },
  skipped: {
    label: "SKIPPED",
    cls: "bg-ink-100 text-ink-400 border border-ink-200",
  },
  unlinked: {
    label: "UNLINKED",
    cls: "bg-warn-dim text-warn border border-warn/40",
  },
  "dry-run": {
    label: "WOULD IMPORT",
    cls: "bg-accent-dim text-accent-fg",
  },
  failed: {
    label: "CONFLICT",
    cls: "bg-white text-warn border border-warn",
  },
  empty: {
    label: "EMPTY",
    cls: "bg-ink-100 text-ink-400",
  },
};

function FileIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4 text-ink-900 shrink-0" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6 M16 18l-4-4-4 4 M12 14v8" />
    </svg>
  );
}

function ChevronIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3.5 w-3.5 text-ink-400" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 6l6 6-6 6" />
    </svg>
  );
}

function InfoIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3.5 w-3.5 text-ink-400 shrink-0" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" />
      <path d="M12 8v4 M12 16h.01" />
    </svg>
  );
}

function RotateIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 12a9 9 0 0 1 15-6.7L21 8 M21 3v5h-5 M21 12a9 9 0 0 1-15 6.7L3 16 M3 21v-5h5" />
    </svg>
  );
}

function EyeOffIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9.88 4.24A9.1 9.1 0 0 1 12 4c7 0 10 8 10 8a18.4 18.4 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24" />
      <path d="M1 1l22 22 M6.61 6.61A13.5 13.5 0 0 0 2 12s3 8 10 8a9.6 9.6 0 0 0 5.39-1.61" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 13l4 4L19 7" />
    </svg>
  );
}
