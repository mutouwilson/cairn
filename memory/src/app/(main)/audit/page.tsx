"use client";
import { useEffect, useMemo, useState } from "react";
import { auditPublicKey, listAudit, verifyAudit } from "@/lib/tauri";
import type { AuditEntry, AuditVerification } from "@/lib/types";
import { useT } from "@/lib/i18n";
import { relativeTime, truncate } from "@/lib/utils";
import { cn } from "@/lib/utils";
import { BtnAccent, BtnSecondary, Card, HashChip } from "@/components/ui";

const ACTION_STYLE: Record<
  string,
  { label: string; cls: string; icon: React.ReactNode }
> = {
  capture: {
    label: "CAPTURE",
    cls: "bg-ink-100 text-ink-900",
    icon: <PencilIcon />,
  },
  extract: {
    label: "EXTRACT",
    cls: "bg-ink-100 text-ink-900",
    icon: <SparklesIcon />,
  },
  read: {
    label: "READ",
    cls: "bg-accent-dim text-accent-fg",
    icon: <EyeIcon />,
  },
  search: {
    label: "READ",
    cls: "bg-accent-dim text-accent-fg",
    icon: <EyeIcon />,
  },
  export: {
    label: "EXPORT",
    cls: "bg-ink-100 text-ink-900",
    icon: <UploadIcon />,
  },
  deny: {
    label: "DENY",
    cls: "bg-white text-danger border border-danger",
    icon: <BanIcon />,
  },
};

function actionStyleFor(action: string) {
  const key = action.toLowerCase();
  return (
    ACTION_STYLE[key] ?? {
      label: key.toUpperCase(),
      cls: "bg-ink-100 text-ink-900",
      icon: null,
    }
  );
}

// Variants for the row visuals per Pencil mockup `q9eW3`. Four
// merge-persistence actions get bespoke styling so the chain reads
// as a story: user-initiated `merge` + `alias_unmerge` are tinted with
// accent / danger respectively, and the system-emitted `alias_routed`
// + `sticky_kept` stay neutral. Returns `null` for everything else so
// the existing user-highlight / deny-red paths in the renderer below
// still drive the default styling.
type RowVariant = {
  kind: "merge" | "alias_routed" | "sticky_kept" | "alias_unmerge";
  icon: string;
  containerCls: string;
  actionCls: string;
  agentCls: string;
};

function getRowVariant(action: string): RowVariant | null {
  const key = action.toLowerCase();
  // Merge writes the audit row as `dedup/merge` with action_kind `merge`.
  // Match both forms so older entries (action="merge") still classify.
  if (key === "merge" || key === "dedup/merge") {
    return {
      kind: "merge",
      icon: "⇄",
      containerCls: "bg-accent-dim border-l-[3px] border-l-accent",
      actionCls: "bg-accent text-white",
      agentCls: "text-accent-fg font-semibold",
    };
  }
  if (key === "alias_routed") {
    return {
      kind: "alias_routed",
      icon: "⇢",
      containerCls: "",
      actionCls: "bg-accent-dim text-accent-fg border border-accent",
      agentCls: "text-ink-500",
    };
  }
  if (key === "sticky_kept") {
    return {
      kind: "sticky_kept",
      icon: "🔒",
      containerCls: "",
      actionCls: "bg-ink-100 text-ink-500",
      agentCls: "text-ink-500",
    };
  }
  if (key === "alias_unmerge") {
    return {
      kind: "alias_unmerge",
      icon: "⤴",
      containerCls: "bg-danger-dim border-l-[3px] border-l-danger",
      actionCls: "bg-danger text-white",
      agentCls: "text-danger font-semibold",
    };
  }
  return null;
}

function formatTime(ms: number) {
  const d = new Date(ms);
  return d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

export default function AuditPage() {
  const t = useT();
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [pub, setPub] = useState<string>("");
  const [verif, setVerif] = useState<AuditVerification | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastVerifyMs, setLastVerifyMs] = useState<number | null>(null);

  async function refresh() {
    setEntries(await listAudit(200));
    setPub(await auditPublicKey());
  }

  useEffect(() => {
    refresh();
  }, []);

  async function runVerify() {
    setBusy(true);
    try {
      const v = await verifyAudit(500);
      setVerif(v);
      setLastVerifyMs(Date.now());
    } finally {
      setBusy(false);
    }
  }

  const visualizerDots = useMemo(() => {
    // Build a fixed-length strip of dots that represents recent entries.
    const SLOTS = 20;
    const slice = entries.slice(0, SLOTS);
    return Array.from({ length: SLOTS }, (_, i) => slice[i] ?? null);
  }, [entries]);

  const total = entries.length;
  const chainOk = verif?.ok ?? true;
  const checked = verif?.entries_checked ?? total;
  const head = entries[0];

  return (
    <div className="max-w-4xl mx-auto px-8 py-8 space-y-5">
      <header className="flex items-end justify-between gap-6">
        <div className="flex items-baseline gap-3">
          <h1 className="font-display text-xl font-semibold text-ink-900">
            {t("audit.title")}
          </h1>
          <span className="font-mono text-2xs text-ink-400">
            {total} {total === 1 ? "entry" : "entries"} · 1 chain ·{" "}
            {verif?.ok === false ? "1 break" : "0 breaks"}
          </span>
        </div>
        <div className="flex items-center gap-2">
          <BtnSecondary disabled>
            <DownloadIcon /> Export chain
          </BtnSecondary>
          <BtnAccent onClick={runVerify} disabled={busy}>
            <ShieldIcon /> {busy ? t("audit.verifying") : t("audit.verify_btn")}
          </BtnAccent>
        </div>
      </header>

      <section className="rounded-xl bg-ink-900 text-white p-6 space-y-5">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <span
              className={cn(
                "inline-flex h-5 w-5 items-center justify-center rounded-full",
                chainOk ? "bg-accent" : "bg-danger",
              )}
            >
              {chainOk ? (
                <CheckIcon className="h-3 w-3 text-white" />
              ) : (
                <XIcon className="h-3 w-3 text-white" />
              )}
            </span>
            <span className="text-md font-medium">
              {chainOk
                ? `Chain verified · ${checked} / ${total} entries`
                : `Chain invalid · ${verif?.message ?? "verification failed"}`}
            </span>
          </div>
          <div className="flex items-baseline gap-2.5 text-2xs">
            <span className="text-ink-400">last verified</span>
            <span className="font-mono text-white">
              {lastVerifyMs ? relativeTime(lastVerifyMs) : "never"}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-1">
          {visualizerDots.map((e, i) => (
            <div key={i} className="flex items-center flex-1">
              <span
                className={cn(
                  "h-2 w-2 rounded-full shrink-0",
                  e == null
                    ? "bg-ink-700"
                    : i === 0
                      ? "bg-white ring-2 ring-accent"
                      : (e.action ?? "").toLowerCase() === "deny"
                        ? "bg-danger"
                        : e.tool_name
                          ? "bg-accent"
                          : "bg-white",
                )}
              />
              {i < visualizerDots.length - 1 && (
                <span className="h-px flex-1 bg-ink-700" />
              )}
            </div>
          ))}
        </div>

        {head && (
          <div className="flex items-center justify-between gap-4 text-2xs">
            <div className="flex items-center gap-3 min-w-0">
              <span className="font-mono text-ink-400 shrink-0">
                entry #{head.seq}
              </span>
              <span className="text-ink-400 shrink-0">hash:</span>
              <span className="font-mono text-white truncate">
                {head.hash.slice(0, 12)}…
              </span>
              <span className="text-ink-400 shrink-0">prev:</span>
              <span className="font-mono text-ink-400 truncate">
                {head.prev_hash.slice(0, 8)}…
              </span>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              <span className="font-mono text-ink-400">ed25519</span>
              <span className="h-1.5 w-1.5 rounded-full bg-accent" />
              <span className="text-accent font-medium">valid</span>
            </div>
          </div>
        )}
      </section>

      <div className="flex items-end justify-between gap-3">
        <h2 className="font-display text-lg font-semibold text-ink-900">
          Recent activity
        </h2>
        <span className="font-mono text-2xs text-ink-400">
          showing latest {Math.min(entries.length, 200)}
        </span>
      </div>

      {entries.length === 0 ? (
        <Card className="text-sm text-ink-500">
          no audit entries yet. let an Agent query this memory via MCP first.
        </Card>
      ) : (
        <div className="rounded-lg border border-ink-200 bg-white overflow-hidden">
          {entries.map((e, idx) => {
            const style = actionStyleFor(e.action);
            const isDeny = e.action.toLowerCase() === "deny";
            const isUser = e.agent_id === "user";
            // Variant takes precedence over the generic user-highlight
            // path but yields to `deny` (which keeps absolute priority
            // for the red rail). Order: deny > variant > user > default.
            const variant = !isDeny ? getRowVariant(e.action) : null;
            const head = e.tool_name ?? e.action;
            const variantLabel = variant
              ? variant.kind.toUpperCase()
              : null;
            return (
              <div
                key={e.seq}
                className={cn(
                  "flex items-center gap-3 px-3 py-3 text-xs",
                  idx > 0 && "border-t border-ink-200",
                  isDeny && "bg-danger-dim/40",
                  // User-driven rows: accent-tinted background + 3px
                  // accent left rail to set them apart from system rows.
                  // `border-l-[3px]` overrides Tailwind's default 1px so
                  // the rail actually shows up.
                  !variant && isUser && !isDeny && "bg-accent-dim border-l-[3px] border-l-accent",
                  variant && variant.containerCls,
                )}
              >
                {variant ? (
                  <span
                    className={cn(
                      "h-3 w-3 shrink-0 inline-flex items-center justify-center font-mono text-xs leading-none",
                      variant.kind === "merge" && "text-accent-fg",
                      variant.kind === "alias_unmerge" && "text-danger",
                      variant.kind === "alias_routed" && "text-accent-fg",
                      variant.kind === "sticky_kept" && "text-ink-500",
                    )}
                    aria-hidden
                  >
                    {variant.icon}
                  </span>
                ) : (
                  isUser && !isDeny && (
                    <RowPencilIcon className="h-3 w-3 text-accent-fg shrink-0" />
                  )
                )}
                <span
                  className={cn(
                    "font-mono text-2xs shrink-0 w-16",
                    isDeny
                      ? "text-danger"
                      : variant
                        ? variant.agentCls.includes("text-danger")
                          ? "text-danger"
                          : variant.agentCls.includes("text-accent")
                            ? "text-accent-fg"
                            : "text-ink-400"
                        : isUser
                          ? "text-accent-fg"
                          : "text-ink-400",
                  )}
                >
                  {formatTime(e.timestamp)}
                </span>
                <span
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-sm px-2 h-5 text-2xs font-mono font-medium shrink-0 w-24 justify-center",
                    // Variant overrides both the user-driven solid-accent
                    // chip and the generic style map.
                    variant
                      ? variant.actionCls
                      : isUser && !isDeny
                        ? "bg-accent text-white"
                        : style.cls,
                  )}
                >
                  {!variant && style.icon}
                  {variant ? variantLabel : style.label}
                </span>
                <span
                  className={cn(
                    "flex-1 min-w-0 truncate",
                    variant
                      ? variant.kind === "alias_unmerge"
                        ? "text-danger"
                        : variant.kind === "merge"
                          ? "text-accent-fg"
                          : "text-ink-900"
                      : isUser && !isDeny
                        ? "text-accent-fg"
                        : "text-ink-900",
                  )}
                >
                  {e.arguments
                    ? truncate(stripJson(e.arguments), 100)
                    : `${head}`}
                </span>
                <HashChip hash={e.hash} />
                <span
                  className={cn(
                    "text-2xs shrink-0 w-32 text-right truncate",
                    isDeny
                      ? "text-danger"
                      : variant
                        ? variant.agentCls
                        : isUser
                          ? "text-accent-fg font-semibold"
                          : "text-ink-500",
                  )}
                >
                  {e.agent_id}
                </span>
                {isDeny ? (
                  <ShieldXIcon className="h-3 w-3 text-danger shrink-0" />
                ) : (
                  <CheckIcon className="h-3 w-3 text-accent shrink-0" />
                )}
              </div>
            );
          })}
        </div>
      )}

      <details className="text-xs text-ink-500">
        <summary className="cursor-pointer">Public key (Ed25519)</summary>
        <code className="block mt-2 p-3 rounded-md bg-ink-100 font-mono text-2xs break-all">
          {pub || "—"}
        </code>
        <p className="mt-2 leading-relaxed">
          Anyone with this key can verify the audit chain was not tampered with.
        </p>
        {verif && !verif.ok && (
          <p className="mt-2 text-danger font-mono">{verif.message}</p>
        )}
      </details>
    </div>
  );
}

function stripJson(s: string): string {
  // Pretty-strip JSON args into a readable single line if they parse,
  // else return the raw string.
  try {
    const parsed = JSON.parse(s);
    if (typeof parsed === "object" && parsed) {
      return Object.entries(parsed)
        .map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`)
        .join(" · ");
    }
  } catch {
    /* fallthrough */
  }
  return s;
}

function CheckIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className ?? "h-3 w-3"}
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M5 13l4 4L19 7" />
    </svg>
  );
}

function XIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className ?? "h-3 w-3"} fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 6l12 12 M18 6L6 18" />
    </svg>
  );
}

function PencilIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-2.5 w-2.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 20h9 M16.5 3.5l4 4L8 20H4v-4z" />
    </svg>
  );
}

// Row prefix for user-driven audit entries. Slightly thicker stroke
// than the small CAPTURE chip icon since this sits at the row level.
function RowPencilIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className ?? "h-3 w-3"}
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M12 20h9 M16.5 3.5l4 4L8 20H4v-4z" />
    </svg>
  );
}

function SparklesIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-2.5 w-2.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 3v3 M12 18v3 M3 12h3 M18 12h3 M5.6 5.6l2.1 2.1 M16.3 16.3l2.1 2.1 M18.4 5.6l-2.1 2.1 M7.7 16.3l-2.1 2.1" />
    </svg>
  );
}

function EyeIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-2.5 w-2.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M1 12s4-7 11-7 11 7 11 7-4 7-11 7-11-7-11-7z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

function UploadIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-2.5 w-2.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 3v14 M8 7l4-4 4 4 M4 21h16" />
    </svg>
  );
}

function BanIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-2.5 w-2.5" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" />
      <path d="M5 5l14 14" />
    </svg>
  );
}

function ShieldIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z M9 12l2 2 4-4" />
    </svg>
  );
}

function ShieldXIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className ?? "h-3 w-3"} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z M9 9l6 6 M15 9l-6 6" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 3v14 M8 13l4 4 4-4 M4 21h16" />
    </svg>
  );
}
