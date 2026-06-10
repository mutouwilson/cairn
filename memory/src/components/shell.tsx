"use client";
// Cairn shell: left rail (56) + AppBar (56) + main body + right Inspector (320).
// Inspector is route-aware: it reads `usePathname()` and renders contextual
// stats fetched via the same IPC the pages use. Pages can override by
// passing `inspector={…}` into AppShell (none do yet).

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { cn, relativeTime } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { InspectorRow, SectionEyebrow, StatusPill } from "@/components/ui";
import {
  listAgents,
  listEntities,
  listNotes,
  listThemes,
  listUsage,
  mcpBridgeStatus,
  selectionStatus,
  verifyAudit,
} from "@/lib/tauri";
import type {
  Agent,
  AuditVerification,
  Entity,
  McpBridgeStatus,
  Note,
  SelectionStatus,
  SemanticMemory,
  UsageRow,
} from "@/lib/types";
import type { ReactNode } from "react";

// SVG icon set — small, monochrome, currentColor.
function Icon({ d, className }: { d: string; className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={cn("h-5 w-5", className)} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d={d} />
    </svg>
  );
}

// Labels carry the dict key, not literal text — `LeftNav` resolves them at
// render time so the rail re-labels live when the user changes language.
const NAV_ITEMS: { href: string; labelKey: string; d: string }[] = [
  { href: "/",          labelKey: "nav.home",     d: "M3 11l9-7 9 7v9a2 2 0 0 1-2 2h-4v-7H9v7H5a2 2 0 0 1-2-2v-9z" },
  { href: "/entities",  labelKey: "nav.entities", d: "M6 7l6-3 6 3-6 3-6-3z M6 12l6 3 6-3 M6 17l6 3 6-3" },
  { href: "/search",    labelKey: "nav.search",   d: "M10 17a7 7 0 1 1 4.95-2.05L20 20" },
  { href: "/themes",    labelKey: "nav.themes",   d: "M4 6h16 M4 12h16 M4 18h10" },
  { href: "/agents",    labelKey: "nav.agents",   d: "M5 20v-2a4 4 0 0 1 4-4h6a4 4 0 0 1 4 4v2 M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8z" },
  { href: "/audit",     labelKey: "nav.audit",    d: "M6 4h9l4 4v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z M9 13l2 2 4-4" },
  { href: "/import",    labelKey: "nav.import",   d: "M12 3v12 M8 11l4 4 4-4 M4 21h16" },
  { href: "/usage",     labelKey: "nav.usage",    d: "M4 20V10 M10 20V4 M16 20v-6 M22 20H2" },
];

// ---------- Left rail ----------

function LeftNav() {
  const pathname = usePathname();
  const t = useT();
  return (
    <aside className="w-nav shrink-0 bg-ink-100 border-r border-ink-200 flex flex-col items-center py-3 gap-1">
      <div className="h-9 w-9 rounded-full bg-ink-900 text-white flex items-center justify-center font-display font-semibold text-sm mb-2">
        c
      </div>
      <nav className="flex flex-col gap-0.5 flex-1">
        {NAV_ITEMS.map((item) => {
          const active = item.href === "/" ? pathname === "/" : pathname?.startsWith(item.href);
          const label = t(item.labelKey);
          return (
            <Link
              key={item.href}
              href={item.href}
              title={label}
              className={cn(
                "h-9 w-9 rounded-lg flex items-center justify-center transition-colors",
                active
                  ? "bg-white text-ink-900 shadow-sm"
                  : "text-ink-500 hover:text-ink-900 hover:bg-white/60",
              )}
            >
              <Icon d={item.d} />
            </Link>
          );
        })}
      </nav>
      <Link
        href="/settings"
        title={t("nav.settings")}
        className={cn(
          "h-9 w-9 rounded-lg flex items-center justify-center transition-colors",
          pathname?.startsWith("/settings")
            ? "bg-white text-ink-900 shadow-sm"
            : "text-ink-500 hover:text-ink-900 hover:bg-white/60",
        )}
      >
        <Icon d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06A2 2 0 1 1 4.21 16.96l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </Link>
    </aside>
  );
}

// ---------- AppBar ----------

const BREADCRUMB_KEYS: Record<string, string> = {
  "/": "nav.home",
  "/entities": "nav.entities",
  "/entity": "nav.entities",
  "/search": "nav.search",
  "/themes": "nav.themes",
  "/agents": "nav.agents",
  "/audit": "nav.audit",
  "/import": "nav.import",
  "/usage": "nav.usage",
  "/settings": "nav.settings",
};

function AppBar() {
  const pathname = usePathname() ?? "/";
  const t = useT();
  // Resolve breadcrumb key from the first path segment; lower-case the label so
  // it looks consistent next to "Cairn / " regardless of source dictionary.
  const segments = ["/" + pathname.split("/").filter(Boolean)[0]];
  const segKey = BREADCRUMB_KEYS[segments[0]];
  const segLabel = (segKey ? t(segKey) : pathname.slice(1)).toLowerCase();
  return (
    <header className="h-header shrink-0 px-5 flex items-center justify-between gap-4 border-b border-ink-200 bg-white">
      <div className="flex items-center gap-2 min-w-0">
        <Link href="/" className="flex items-center gap-1.5 shrink-0">
          <span className="h-5 w-5 rounded-full bg-ink-900" />
          <span className="font-display font-semibold tracking-tight leading-none text-sm">Cairn</span>
        </Link>
        <span className="text-ink-300 text-sm leading-none">/</span>
        <span className="text-sm text-ink-500 truncate leading-none">{segLabel}</span>
      </div>

      <div className="flex-1 max-w-xl">
        <SearchBox />
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <StatusPill tone="accent" dot>
          chain ok
        </StatusPill>
        <StatusPill tone="muted">GET API token</StatusPill>
        <StatusPill tone="muted">on-device</StatusPill>
      </div>
    </header>
  );
}

function SearchBox() {
  const t = useT();
  return (
    <div className="relative">
      <span className="absolute left-2.5 top-1/2 -translate-y-1/2 text-ink-400">
        <Icon d="M10 17a7 7 0 1 1 4.95-2.05L20 20" className="h-3.5 w-3.5" />
      </span>
      <input
        type="search"
        placeholder={t("appbar.search_placeholder")}
        className="w-full h-8 pl-8 pr-12 rounded-md border border-ink-200 bg-ink-100/60 text-sm placeholder:text-ink-400 focus:outline-none focus:ring-2 focus:ring-accent/30 focus:border-accent/40 focus:bg-white"
      />
      <span className="absolute right-2 top-1/2 -translate-y-1/2 flex gap-0.5 text-2xs text-ink-400 font-mono">
        {t("appbar.shortcut_hint")}
      </span>
    </div>
  );
}

// ---------- Inspector slot ----------

// Server-render-safe default: just a placeholder. Pages opt-in by passing
// `inspector={…}` into AppShell. Keep ~320px width.
export function AppShell({
  inspector,
  children,
}: {
  inspector?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="h-screen flex bg-ink-50 overflow-hidden">
      <LeftNav />
      <div className="flex-1 min-w-0 flex flex-col min-h-0">
        <AppBar />
        <div className="flex-1 flex min-h-0">
          <main className="flex-1 min-w-0 overflow-y-auto">{children}</main>
          <aside className="w-inspector shrink-0 border-l border-ink-200 bg-white overflow-y-auto">
            {inspector ?? <DefaultInspector />}
          </aside>
        </div>
      </div>
    </div>
  );
}

// Route-aware default inspector. Pages can still override by passing their
// own ReactNode into <AppShell inspector={…}>.
function DefaultInspector() {
  const pathname = usePathname() ?? "/";
  const route = "/" + (pathname.split("/").filter(Boolean)[0] ?? "");
  return (
    <div className="p-5 space-y-5">
      <header className="flex items-baseline justify-between">
        <h2 className="font-display font-semibold text-md">Inspector</h2>
        <span className="font-mono text-2xs text-ink-400">{route === "/" ? "home" : route.slice(1)}</span>
      </header>
      <InspectorBody route={route} />
      <GlobalVitals />
    </div>
  );
}

function InspectorBody({ route }: { route: string }) {
  switch (route) {
    case "/":
      return <RecentNotesPanel />;
    case "/entities":
    case "/entity":
      return <EntitiesPanel />;
    case "/search":
      return <SearchPanel />;
    case "/themes":
      return <ThemesPanel />;
    case "/agents":
      return <AgentsPanel />;
    case "/audit":
      return <AuditPanel />;
    case "/import":
      return <ImportPanel />;
    case "/usage":
      return <UsagePanel />;
    case "/settings":
      return <SettingsPanel />;
    default:
      return null;
  }
}

// ---------- Per-route panels ----------

function RecentNotesPanel() {
  const [notes, setNotes] = useState<Note[]>([]);
  useEffect(() => {
    listNotes(5).then(setNotes).catch(() => setNotes([]));
  }, []);
  if (!notes.length) return null;
  const latest = notes[0];
  const pending = notes.filter((n) => n.processing_status === "pending").length;
  const failed = notes.filter((n) => n.processing_status === "failed").length;
  return (
    <section className="space-y-2">
      <SectionEyebrow>last capture</SectionEyebrow>
      <p className="text-xs text-ink-700 line-clamp-3 leading-relaxed">{latest.text}</p>
      <div className="space-y-0.5">
        <InspectorRow label="when" value={relativeTime(latest.created_at)} />
        <InspectorRow label="status" value={
          <StatusPill
            tone={latest.processing_status === "done" ? "accent" : latest.processing_status === "failed" ? "danger" : "warn"}
            dot
          >
            {latest.processing_status}
          </StatusPill>
        } />
        <InspectorRow label="pending" value={String(pending)} />
        <InspectorRow label="failed" value={String(failed)} />
      </div>
    </section>
  );
}

function EntitiesPanel() {
  const [counts, setCounts] = useState<Record<string, number> | null>(null);
  useEffect(() => {
    listEntities(undefined, 500).then((es: Entity[]) => {
      const c: Record<string, number> = {};
      for (const e of es) c[e.type] = (c[e.type] ?? 0) + 1;
      setCounts(c);
    }).catch(() => setCounts({}));
  }, []);
  if (!counts) return null;
  const total = Object.values(counts).reduce((a, b) => a + b, 0);
  return (
    <section className="space-y-2">
      <SectionEyebrow>by type</SectionEyebrow>
      <InspectorRow label="total" value={String(total)} />
      {Object.entries(counts)
        .sort((a, b) => b[1] - a[1])
        .map(([type, n]) => (
          <InspectorRow key={type} label={type.toLowerCase()} value={String(n)} />
        ))}
    </section>
  );
}

function SearchPanel() {
  return (
    <section className="space-y-2">
      <SectionEyebrow>retrieval</SectionEyebrow>
      <p className="text-xs text-ink-500 leading-relaxed">
        Type a query to surface entities + notes. Hybrid score combines BM25 +
        vector + RRF with importance &amp; recency.
      </p>
      <InspectorRow label="fusion" value="reciprocal rank" />
      <InspectorRow label="vector dist max" value="0.65 cosine" />
      <InspectorRow label="bm25 tokenizer" value="trigram" />
    </section>
  );
}

function ThemesPanel() {
  const [themes, setThemes] = useState<SemanticMemory[]>([]);
  useEffect(() => {
    listThemes(50).then(setThemes).catch(() => setThemes([]));
  }, []);
  if (!themes.length) return null;
  const active = themes.filter((t) => !t.superseded_by).length;
  return (
    <section className="space-y-2">
      <SectionEyebrow>consolidation</SectionEyebrow>
      <InspectorRow label="total themes" value={String(themes.length)} />
      <InspectorRow label="active" value={String(active)} />
      <InspectorRow label="superseded" value={String(themes.length - active)} />
      {themes[0] && (
        <>
          <p className="eyebrow mt-3 mb-1">latest</p>
          <p className="text-xs text-ink-700 line-clamp-3">{themes[0].title}</p>
          <InspectorRow label="last run" value={relativeTime(themes[0].last_consolidated_at)} />
        </>
      )}
    </section>
  );
}

function AgentsPanel() {
  const [agents, setAgents] = useState<Agent[]>([]);
  useEffect(() => {
    listAgents().then(setAgents).catch(() => setAgents([]));
  }, []);
  if (!agents.length) return null;
  const live = agents.filter((a) => !a.revoked_at).length;
  return (
    <section className="space-y-2">
      <SectionEyebrow>connected</SectionEyebrow>
      <InspectorRow label="agents" value={String(agents.length)} />
      <InspectorRow label="live" value={String(live)} />
      <InspectorRow label="revoked" value={String(agents.length - live)} />
      {agents[0]?.last_seen && (
        <InspectorRow label="last seen" value={relativeTime(agents[0].last_seen)} />
      )}
    </section>
  );
}

function AuditPanel() {
  const [v, setV] = useState<AuditVerification | null>(null);
  useEffect(() => {
    verifyAudit(200).then(setV).catch(() => setV({ ok: false, message: "n/a", entries_checked: 0 }));
  }, []);
  return (
    <section className="space-y-2">
      <SectionEyebrow>chain</SectionEyebrow>
      <InspectorRow
        label="status"
        value={
          v == null ? (
            <span className="text-2xs text-ink-400">checking…</span>
          ) : (
            <StatusPill tone={v.ok ? "accent" : "danger"} dot>
              {v.ok ? "verified" : "broken"}
            </StatusPill>
          )
        }
      />
      {v && <InspectorRow label="entries checked" value={String(v.entries_checked)} />}
      {v && !v.ok && (
        <p className="text-2xs text-danger break-words">{v.message}</p>
      )}
    </section>
  );
}

function ImportPanel() {
  return (
    <section className="space-y-2">
      <SectionEyebrow>sources</SectionEyebrow>
      <InspectorRow label="claude/CLAUDE.md" value={<StatusPill tone="muted">on-disk</StatusPill>} />
      <InspectorRow label="claude/projects" value={<StatusPill tone="muted">on-disk</StatusPill>} />
      <p className="text-2xs text-ink-500 leading-relaxed mt-3">
        Dedup is by content sha256. Re-running Import is safe — unchanged
        files skip, changed files replace their prior note.
      </p>
    </section>
  );
}

function UsagePanel() {
  const [rows, setRows] = useState<UsageRow[]>([]);
  useEffect(() => {
    listUsage(500).then(setRows).catch(() => setRows([]));
  }, []);
  if (!rows.length) return null;
  const dayMs = 24 * 60 * 60 * 1000;
  const since = Date.now() - dayMs;
  const today = rows.filter((r) => r.occurred_at >= since);
  const cost = today.reduce((s, r) => s + (r.cost_micro_usd ?? 0), 0) / 1_000_000;
  const failures = today.filter((r) => r.success === 0).length;
  return (
    <section className="space-y-2">
      <SectionEyebrow>last 24h</SectionEyebrow>
      <InspectorRow label="calls" value={String(today.length)} />
      <InspectorRow label="cost" value={`$${cost.toFixed(3)}`} />
      <InspectorRow label="failures" value={String(failures)} />
    </section>
  );
}

function SettingsPanel() {
  const [mcp, setMcp] = useState<McpBridgeStatus | null>(null);
  const [sel, setSel] = useState<SelectionStatus | null>(null);
  useEffect(() => {
    mcpBridgeStatus().then(setMcp).catch(() => setMcp(null));
    selectionStatus().then(setSel).catch(() => setSel(null));
  }, []);
  return (
    <section className="space-y-2">
      <SectionEyebrow>surfaces</SectionEyebrow>
      <InspectorRow
        label="MCP bridge"
        value={
          <StatusPill tone={mcp?.running ? "accent" : "muted"} dot>
            {mcp?.running ? `:${mcp.port}` : "off"}
          </StatusPill>
        }
      />
      <InspectorRow
        label="selection popover"
        value={
          <StatusPill tone={sel?.enabled ? "accent" : "muted"} dot>
            {sel?.enabled ? "enabled" : "off"}
          </StatusPill>
        }
      />
      <InspectorRow label="permission" value={sel?.permission_granted ? "granted" : "denied"} />
    </section>
  );
}

// Footer block: shared across every page.
function GlobalVitals() {
  const [stats, setStats] = useState<{ entities: number; notes: number } | null>(null);
  useEffect(() => {
    Promise.all([listEntities(undefined, 1000), listNotes(1000)])
      .then(([es, ns]) => setStats({ entities: es.length, notes: ns.length }))
      .catch(() => setStats(null));
  }, []);
  return (
    <section className="space-y-1 pt-4 border-t border-ink-200">
      <SectionEyebrow>cairn vitals</SectionEyebrow>
      <InspectorRow label="entities" value={stats ? String(stats.entities) : "—"} />
      <InspectorRow label="notes" value={stats ? String(stats.notes) : "—"} />
      <InspectorRow label="version" value="v0.1.0" mono />
      <InspectorRow label="storage" value="local" mono />
    </section>
  );
}
