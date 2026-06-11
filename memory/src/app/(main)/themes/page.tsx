"use client";
import { useEffect, useState } from "react";
import {
  listConsolidationRuns,
  listThemes,
  triggerConsolidation,
} from "@/lib/tauri";
import type { ConsolidationRun, SemanticMemory } from "@/lib/types";
import { useT } from "@/lib/i18n";
import { relativeTime, safeJsonParse } from "@/lib/utils";
import { cn } from "@/lib/utils";
import {
  BtnPrimary,
  Card,
  HashChip,
  SectionEyebrow,
  StatusPill,
} from "@/components/ui";

export default function ThemesPage() {
  const t = useT();
  const [themes, setThemes] = useState<SemanticMemory[]>([]);
  const [runs, setRuns] = useState<ConsolidationRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  async function refresh() {
    try {
      const [tt, r] = await Promise.all([listThemes(200), listConsolidationRuns(20)]);
      setThemes(tt);
      setRuns(r);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    refresh();
  }, []);

  async function trigger() {
    if (busy) return;
    setBusy(true);
    setHint(null);
    try {
      const stats = await triggerConsolidation();
      setHint(
        `topics ${stats.topics_scanned} · +${stats.semantic_created} new · ~${stats.semantic_updated} updated · ${stats.errors.length} errors`,
      );
      refresh();
    } catch (e) {
      // Tauri commands reject with a plain string (CmdResult's Err), not an
      // Error — `.message` on it is undefined and eats the real reason.
      setHint(`error: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="max-w-4xl mx-auto px-8 py-8 space-y-5">
      <header className="flex items-end justify-between gap-6">
        <div className="flex items-baseline gap-3">
          <h1 className="font-display text-xl font-semibold text-ink-900">{t("themes.title")}</h1>
          <span className="font-mono text-2xs text-ink-400">
            {t("themes.stats", { n: themes.length, runs: runs.length })}
          </span>
        </div>
        <BtnPrimary onClick={trigger} disabled={busy}>
          <SparkleIcon />
          {busy ? t("themes.running") : t("themes.run_btn")}
        </BtnPrimary>
      </header>

      <p className="text-xs text-ink-500 max-w-prose leading-relaxed">
        {t("themes.subtitle")}
      </p>

      {hint && (
        <p
          className={cn(
            "font-mono text-2xs",
            hint.startsWith("error") ? "text-danger" : "text-accent-fg",
          )}
        >
          {hint}
        </p>
      )}

      <section className="space-y-3">
        <SectionEyebrow>{t("themes.active_eyebrow")}</SectionEyebrow>
        {loading ? (
          <p className="text-sm text-ink-500">{t("common.loading")}</p>
        ) : themes.length === 0 ? (
          <Card className="text-sm text-ink-500">
            {t("themes.no_themes")} <strong className="text-ink-900">{t("themes.no_themes_runner_label")}</strong>{t("themes.no_themes_dot")}
          </Card>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            {themes.map((theme, idx) => (
              <ThemeCard key={theme.id} theme={theme} highlight={idx === 0} />
            ))}
          </div>
        )}
      </section>

      <section className="space-y-3">
        <SectionEyebrow>{t("themes.runs_eyebrow")}</SectionEyebrow>
        {runs.length === 0 ? (
          <Card className="text-sm text-ink-500">{t("themes.no_runs")}</Card>
        ) : (
          <div className="rounded-lg border border-ink-200 bg-white overflow-hidden">
            {runs.map((r, idx) => (
              <div
                key={r.id}
                className={cn(
                  "flex items-center gap-3 px-3 py-2.5 text-xs",
                  idx > 0 && "border-t border-ink-200",
                )}
              >
                <RunPill status={r.status} />
                <span className="font-mono text-2xs text-ink-500 w-16 shrink-0">
                  {r.trigger}
                </span>
                <span className="flex-1 min-w-0 text-ink-900 truncate">
                  scanned {r.topics_scanned} · +{r.semantic_created} new · ~
                  {r.semantic_updated} updated
                </span>
                {r.error && (
                  <span className="text-danger font-mono text-2xs truncate max-w-[40%]">
                    {r.error}
                  </span>
                )}
                <span className="font-mono text-2xs text-ink-400 shrink-0">
                  {r.started_at ? relativeTime(r.started_at) : "—"}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function ThemeCard({
  theme,
  highlight,
}: {
  theme: SemanticMemory;
  highlight?: boolean;
}) {
  const evidence = safeJsonParse<string[]>(theme.evidence, []);
  return (
    <div
      className={cn(
        "rounded-lg border bg-white p-4 transition-all hover:shadow-sm space-y-2.5",
        highlight ? "border-ink-900" : "border-ink-200 hover:border-ink-300",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <span className="inline-flex items-center gap-1.5 rounded-md bg-ink-100/70 px-2 h-6 text-2xs font-medium text-ink-700">
          <span className="h-1.5 w-1.5 rounded-full bg-accent" />
          {theme.topic.toLowerCase()}
        </span>
        <span className="font-mono text-2xs text-ink-400">
          {relativeTime(theme.last_consolidated_at)}
        </span>
      </div>
      <h3 className="text-md font-medium text-ink-900 leading-snug">
        {theme.title}
      </h3>
      <p className="text-xs text-ink-500 leading-relaxed line-clamp-3 whitespace-pre-wrap">
        {theme.summary}
      </p>
      <div className="flex items-center justify-between pt-1">
        <HashChip hash={theme.id} />
        <div className="font-mono text-2xs text-ink-400 flex items-center gap-2">
          <span>evidence {evidence.length}</span>
          <span className="text-ink-300">·</span>
          <span>conf {theme.confidence.toFixed(2)}</span>
        </div>
      </div>
    </div>
  );
}

function RunPill({ status }: { status: string }) {
  if (status === "done")
    return (
      <StatusPill tone="accent" dot>
        done
      </StatusPill>
    );
  if (status === "failed")
    return (
      <StatusPill tone="danger" dot>
        failed
      </StatusPill>
    );
  return (
    <StatusPill tone="warn" dot>
      {status}
    </StatusPill>
  );
}

function SparkleIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-3 w-3"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M12 3v6 M12 15v6 M3 12h6 M15 12h6 M5 5l4 4 M15 15l4 4 M19 5l-4 4 M9 15l-4 4" />
    </svg>
  );
}
