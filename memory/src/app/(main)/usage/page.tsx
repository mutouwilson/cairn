"use client";
import { useEffect, useState } from "react";
import { listUsage, usageSummary } from "@/lib/tauri";
import type { UsageDaySummary, UsageRow } from "@/lib/types";
import { useT } from "@/lib/i18n";
import { relativeTime } from "@/lib/utils";
import { cn } from "@/lib/utils";
import { Card, StatusPill } from "@/components/ui";

export default function UsagePage() {
  const t = useT();
  const [rows, setRows] = useState<UsageRow[]>([]);
  const [summary, setSummary] = useState<UsageDaySummary[]>([]);
  const [days, setDays] = useState(14);

  useEffect(() => {
    listUsage(200).then(setRows).catch(() => setRows([]));
    usageSummary(days).then(setSummary).catch(() => setSummary([]));
  }, [days]);

  const totals = summary.reduce(
    (acc, s) => {
      acc.cost += s.cost_micro_usd;
      acc.calls += s.calls;
      acc.pt += s.prompt_tokens;
      acc.ct += s.completion_tokens;
      acc.lat = Math.max(acc.lat, s.p50_latency_ms);
      return acc;
    },
    { cost: 0, calls: 0, pt: 0, ct: 0, lat: 0 },
  );

  return (
    <div className="max-w-4xl mx-auto px-8 py-8 space-y-5">
      <header className="flex items-end justify-between gap-6">
        <div className="flex items-baseline gap-3">
          <h1 className="font-display text-xl font-semibold text-ink-900">{t("usage.title")}</h1>
          <span className="font-mono text-2xs text-ink-400">
            {totals.calls.toLocaleString()} calls in last {days}d
          </span>
        </div>
        <select
          value={days}
          onChange={(e) => setDays(parseInt(e.target.value, 10))}
          className="rounded-md border border-ink-200 bg-white px-2 h-8 text-xs text-ink-900 focus:outline-none focus:border-ink-900"
        >
          <option value={1}>last 24h</option>
          <option value={7}>last 7 days</option>
          <option value={14}>last 14 days</option>
          <option value={30}>last 30 days</option>
        </select>
      </header>

      <p className="text-xs text-ink-500 max-w-prose leading-relaxed">
        Per-call latency and cost. Cost comes straight from the provider response
        (Anthropic-direct and local providers leave cost null).
      </p>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <StatBox
          eyebrow="TOTAL COST"
          value={`$${(totals.cost / 1_000_000).toFixed(4)}`}
          sub={`${days}-day window`}
        />
        <StatBox
          eyebrow="API CALLS"
          value={totals.calls.toLocaleString()}
          sub="successful"
        />
        <StatBox
          eyebrow="P50 LATENCY"
          value={`${totals.lat}`}
          sub="ms · slowest day"
        />
        <StatBox
          eyebrow="TOKENS"
          value={(totals.pt + totals.ct).toLocaleString()}
          sub={`${totals.pt.toLocaleString()} in · ${totals.ct.toLocaleString()} out`}
          dark
        />
      </div>

      <section className="space-y-3">
        <div className="flex items-end justify-between">
          <h2 className="font-display text-lg font-semibold text-ink-900">
            Daily breakdown
          </h2>
          <span className="font-mono text-2xs text-ink-400">
            {summary.length} {summary.length === 1 ? "row" : "rows"}
          </span>
        </div>
        {summary.length === 0 ? (
          <Card className="text-sm text-ink-500">
            no usage in this window — write a note to trigger extraction.
          </Card>
        ) : (
          <div className="rounded-lg border border-ink-200 bg-white overflow-hidden">
            <div className="flex items-center gap-3 px-3 py-2 border-b border-ink-200">
              <Th className="w-20 shrink-0">Day</Th>
              <Th className="w-24 shrink-0">Op</Th>
              <Th className="w-32 shrink-0">Provider</Th>
              <Th className="w-14 shrink-0" right>
                Calls
              </Th>
              <Th className="flex-1 min-w-0" right>
                Tokens
              </Th>
              <Th className="w-24 shrink-0" right>
                Cost
              </Th>
              <Th className="w-16 shrink-0" right>
                P50 ms
              </Th>
            </div>
            {summary.map((s, i) => (
              <div
                key={i}
                className="flex items-center gap-3 px-3 py-2.5 border-t border-ink-200 text-xs font-mono"
              >
                <span className="w-20 shrink-0 text-ink-900 truncate">{s.day}</span>
                <span className="w-24 shrink-0 text-ink-700 truncate">{s.operation}</span>
                <span className="w-32 shrink-0 text-ink-500 truncate">{s.provider}</span>
                <span className="w-14 shrink-0 text-right text-ink-900">{s.calls}</span>
                <span className="flex-1 min-w-0 text-right text-ink-500 truncate">
                  {s.prompt_tokens}p / {s.completion_tokens}c
                </span>
                <span className="w-24 shrink-0 text-right text-ink-900">
                  ${(s.cost_micro_usd / 1_000_000).toFixed(6)}
                </span>
                <span className="w-16 shrink-0 text-right text-ink-700">
                  {s.p50_latency_ms}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="space-y-3">
        <div className="flex items-end justify-between">
          <h2 className="font-display text-lg font-semibold text-ink-900">
            Recent calls
          </h2>
          <span className="font-mono text-2xs text-ink-400">
            latest {Math.min(rows.length, 50)}
          </span>
        </div>
        {rows.length === 0 ? (
          <Card className="text-sm text-ink-500">no calls logged yet.</Card>
        ) : (
          <div className="rounded-lg border border-ink-200 bg-white overflow-hidden">
            {rows.slice(0, 50).map((r, idx) => (
              <div
                key={r.id}
                className={cn(
                  "flex items-center gap-3 px-3 py-2.5 text-xs",
                  idx > 0 && "border-t border-ink-200",
                  !r.success && "bg-danger-dim/40",
                )}
              >
                <span className="w-16 shrink-0">
                  {r.success ? (
                    <StatusPill tone="accent" dot>
                      ok
                    </StatusPill>
                  ) : (
                    <StatusPill tone="danger" dot>
                      err
                    </StatusPill>
                  )}
                </span>
                <span className="w-24 shrink-0 font-mono text-2xs text-ink-700 truncate">
                  {r.operation}
                </span>
                <span className="w-40 shrink-0 font-mono text-2xs text-ink-500 truncate">
                  {r.provider}/{r.model}
                </span>
                <span className="flex-1 min-w-0 font-mono text-2xs text-ink-500 truncate">
                  {r.latency_ms}ms
                  {r.prompt_tokens != null && (
                    <> · {r.prompt_tokens}p</>
                  )}
                  {r.completion_tokens != null && (
                    <>/{r.completion_tokens}c</>
                  )}
                  {r.cost_micro_usd != null && (
                    <> · ${(r.cost_micro_usd / 1_000_000).toFixed(6)}</>
                  )}
                  {r.error && (
                    <span className="text-danger ml-2">· {r.error}</span>
                  )}
                </span>
                <span className="font-mono text-2xs text-ink-400 shrink-0">
                  {relativeTime(r.occurred_at)}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function Th({
  children,
  right,
  className,
}: {
  children: React.ReactNode;
  right?: boolean;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "font-mono text-2xs font-medium text-ink-400 truncate",
        right ? "text-right" : "text-left",
        className,
      )}
    >
      {children}
    </span>
  );
}

function StatBox({
  eyebrow,
  value,
  sub,
  dark,
}: {
  eyebrow: string;
  value: string;
  sub: string;
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
          dark ? "text-white" : "text-ink-900",
        )}
      >
        {value}
      </p>
      <p className="font-mono text-2xs text-ink-400">{sub}</p>
    </div>
  );
}

