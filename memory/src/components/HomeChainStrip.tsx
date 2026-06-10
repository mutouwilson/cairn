"use client";
// Compact horizontal visualisation of the most recent audit-chain entries,
// rendered above the Capture box on the Home page. Matches the Pencil
// design: 8 dots connected by hairlines, hash short-label below each.
// Clicking the strip routes to `/audit` for the full verification flow.

import Link from "next/link";
import { useEffect, useState } from "react";
import { listAudit, verifyAudit } from "@/lib/tauri";
import type { AuditEntry, AuditVerification } from "@/lib/types";
import { SectionEyebrow } from "@/components/ui";
import { cn } from "@/lib/utils";

const COUNT = 8;

function short(hash: string): string {
  return hash.length >= 4 ? hash.slice(0, 4) : hash;
}

export function HomeChainStrip() {
  const [entries, setEntries] = useState<AuditEntry[] | null>(null);
  const [verified, setVerified] = useState<AuditVerification | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [e, v] = await Promise.all([
          listAudit(COUNT),
          verifyAudit(COUNT * 4),
        ]);
        if (cancelled) return;
        // listAudit returns most-recent-first; we want oldest → newest so the
        // strip reads left-to-right as a timeline.
        setEntries([...e].reverse());
        setVerified(v);
      } catch {
        if (!cancelled) setEntries([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Hide entirely if the user has never captured — the strip looks anaemic
  // with one node and feels like a UI bug.
  if (entries && entries.length < 2) return null;

  const chainOk = verified ? verified.ok : null;
  const total = verified ? verified.entries_checked : entries?.length ?? 0;

  return (
    <section className="space-y-2">
      <div className="flex items-baseline justify-between">
        <SectionEyebrow>audit chain</SectionEyebrow>
        <div className="flex items-center gap-2 text-2xs">
          <span
            className={cn(
              "inline-flex items-center gap-1 font-mono",
              chainOk === false ? "text-danger" : "text-ink-500",
            )}
          >
            <span
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                chainOk === false
                  ? "bg-danger"
                  : chainOk === true
                    ? "bg-accent"
                    : "bg-ink-300",
              )}
            />
            {chainOk === false ? "chain broken" : chainOk === true ? "chain ok" : "verifying…"}
            {total ? <span className="text-ink-400"> · {total}</span> : null}
          </span>
          <Link
            href="/audit"
            className="text-ink-500 hover:text-ink-900 underline-offset-2 hover:underline"
          >
            verify
          </Link>
        </div>
      </div>

      <Link
        href="/audit"
        className="block rounded-lg border border-ink-200 bg-white px-4 py-3 hover:border-ink-300 transition-colors"
      >
        {entries === null ? (
          <div className="h-9 flex items-center text-2xs text-ink-400 font-mono">
            loading chain…
          </div>
        ) : (
          <div className="flex items-center justify-between gap-2">
            {entries.map((entry, i) => {
              const last = i === entries.length - 1;
              return (
                <div key={entry.seq} className="flex items-center gap-0 flex-1 last:flex-initial">
                  <Node
                    hash={short(entry.hash)}
                    accent={last}
                    action={entry.action}
                    seq={entry.seq}
                  />
                  {!last && (
                    <span
                      aria-hidden
                      className="flex-1 h-px bg-ink-200"
                    />
                  )}
                </div>
              );
            })}
          </div>
        )}
      </Link>
    </section>
  );
}

function Node({
  hash,
  accent,
  action,
  seq,
}: {
  hash: string;
  accent: boolean;
  action: string;
  seq: number;
}) {
  return (
    <div
      className="flex flex-col items-center gap-1 shrink-0"
      title={`#${seq} · ${action} · ${hash}`}
    >
      <span
        className={cn(
          "h-2 w-2 rounded-full",
          accent ? "bg-accent ring-2 ring-accent/20" : "bg-ink-700",
        )}
      />
      <code
        className={cn(
          "font-mono text-[10px] leading-none",
          accent ? "text-accent" : "text-ink-400",
        )}
      >
        {hash}
      </code>
    </div>
  );
}
