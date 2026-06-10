"use client";
import { useEffect, useState } from "react";
import {
  addAgent,
  deleteAgent,
  grantPermission,
  listAgents,
  listPermissions,
  revokePermission,
} from "@/lib/tauri";
import type { Agent, Permission } from "@/lib/types";
import { ENTITY_TYPES } from "@/lib/types";
import { useT } from "@/lib/i18n";
import { relativeTime } from "@/lib/utils";
import { cn } from "@/lib/utils";
import { BtnPrimary, BtnSecondary, Card, SectionEyebrow } from "@/components/ui";

type Scope = "read" | "write" | "none";

const TYPE_SHORT: Record<string, string> = {
  Preference: "PREF",
  Skill: "SKILL",
  Event: "EVENT",
  Asset: "FACT",
  Person: "PERS",
  Goal: "GOAL",
  Belief: "BELF",
  Location: "LOC",
};

export default function AgentsPage() {
  const t = useT();
  const [agents, setAgents] = useState<Agent[]>([]);
  const [permsByAgent, setPermsByAgent] = useState<Record<string, Permission[]>>({});
  const [showAdd, setShowAdd] = useState(false);
  const [newId, setNewId] = useState("");
  const [newName, setNewName] = useState("");
  const [loading, setLoading] = useState(true);

  async function refresh() {
    try {
      const ags = await listAgents();
      setAgents(ags);
      const all: Record<string, Permission[]> = {};
      for (const a of ags) {
        all[a.id] = await listPermissions(a.id);
      }
      setPermsByAgent(all);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function cycleScope(agentId: string, entityType: string, current: Scope) {
    // Click: cycle none → read → write → none.
    const next: Scope =
      current === "none" ? "read" : current === "read" ? "write" : "none";
    if (next === "none") {
      await revokePermission(agentId, entityType);
    } else {
      await grantPermission(agentId, entityType, next);
    }
    refresh();
  }

  async function addNew() {
    if (!newId.trim()) return;
    await addAgent(newId.trim(), newName.trim() || newId.trim());
    setNewId("");
    setNewName("");
    setShowAdd(false);
    refresh();
  }

  async function removeAgent(agentId: string) {
    await deleteAgent(agentId);
    refresh();
  }

  const connected = agents.filter((a) => a.revoked_at == null).length;

  return (
    <div className="max-w-4xl mx-auto px-8 py-8 space-y-5">
      <header className="flex items-end justify-between gap-6">
        <div className="flex items-baseline gap-3">
          <h1 className="font-display text-xl font-semibold text-ink-900">{t("agents.title")}</h1>
          <span className="font-mono text-2xs text-ink-400">
            {t("agents.connected", { n: connected })}
            {agents.length - connected > 0
              ? " " + t("agents.revoked", { n: agents.length - connected })
              : ""}
          </span>
        </div>
        <BtnPrimary onClick={() => setShowAdd((v) => !v)}>
          <PlusIcon /> {t("agents.connect_btn")}
        </BtnPrimary>
      </header>

      {showAdd && (
        <Card className="space-y-3">
          <div className="flex items-baseline gap-2">
            <SectionEyebrow>{t("agents.form.eyebrow")}</SectionEyebrow>
          </div>
          <div className="flex flex-wrap gap-2">
            <input
              value={newId}
              onChange={(e) => setNewId(e.target.value)}
              placeholder={t("agents.form.id_placeholder")}
              className="flex-1 min-w-40 rounded-md border border-ink-200 bg-white px-3 h-8 text-xs font-mono focus:outline-none focus:border-ink-900"
            />
            <input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={t("agents.form.name_placeholder")}
              className="flex-1 min-w-40 rounded-md border border-ink-200 bg-white px-3 h-8 text-xs focus:outline-none focus:border-ink-900"
            />
            <BtnPrimary onClick={addNew} disabled={!newId.trim()}>
              {t("agents.form.add")}
            </BtnPrimary>
            <BtnSecondary onClick={() => setShowAdd(false)}>{t("agents.form.cancel")}</BtnSecondary>
          </div>
        </Card>
      )}

      {loading ? (
        <p className="text-sm text-ink-500">{t("common.loading")}</p>
      ) : agents.length === 0 ? (
        <Card className="text-sm text-ink-500">
          {t("agents.empty", { action: t("agents.connect_btn") })}
        </Card>
      ) : (
        <div className="rounded-lg border border-ink-200 bg-white overflow-hidden">
          <div className="flex items-center px-3 py-3 border-b border-ink-200">
            <div className="w-48 shrink-0" />
            <div className="flex-1 flex items-center justify-end gap-1.5">
              {ENTITY_TYPES.map((et) => (
                <span
                  key={et}
                  className="w-10 text-center font-mono text-2xs font-medium text-ink-400"
                >
                  {TYPE_SHORT[et] ?? et.slice(0, 4).toUpperCase()}
                </span>
              ))}
            </div>
            <div className="w-16 text-right font-mono text-2xs font-medium text-ink-400 shrink-0">
              {t("agents.reads_col")}
            </div>
            <div className="w-8 shrink-0" />
          </div>
          {agents.map((agent) => (
            <AgentRow
              key={agent.id}
              agent={agent}
              perms={permsByAgent[agent.id] ?? []}
              onCycle={cycleScope}
              onDelete={removeAgent}
            />
          ))}
        </div>
      )}

      <div className="flex items-center gap-5 px-2 py-2 text-2xs text-ink-400">
        <span className="flex items-center gap-1.5">
          <span className="h-1.5 w-1.5 rounded-full bg-accent" /> read allowed
        </span>
        <span className="flex items-center gap-1.5">
          <span className="h-1.5 w-1.5 rounded-full bg-ink-900" /> write allowed
        </span>
        <span className="flex items-center gap-1.5">
          <span className="h-px w-2 bg-ink-300" /> unset
        </span>
      </div>
    </div>
  );
}

function AgentRow({
  agent,
  perms,
  onCycle,
  onDelete,
}: {
  agent: Agent;
  perms: Permission[];
  onCycle: (agentId: string, entityType: string, current: Scope) => void;
  onDelete: (agentId: string) => void;
}) {
  const t = useT();
  const permMap: Record<string, Scope> = {};
  for (const p of perms) permMap[p.entity_type] = p.scope as Scope;
  const revoked = agent.revoked_at != null;
  const totalReads = perms.filter((p) => p.scope !== "none").length;
  const lastSeenLabel = agent.last_seen
    ? t("agents.row.connected", { when: relativeTime(agent.last_seen) })
    : t("agents.row.never_connected");
  const [armed, setArmed] = useState(false);

  // Disarm the delete button after a few seconds so a stale armed state
  // can't take down an agent on the next stray click.
  useEffect(() => {
    if (!armed) return;
    const timer = setTimeout(() => setArmed(false), 3000);
    return () => clearTimeout(timer);
  }, [armed]);

  return (
    <div
      className={cn(
        "flex items-center px-3 py-3 border-t border-ink-200",
        revoked && "opacity-60",
      )}
    >
      <div className="w-48 flex items-center gap-2.5 shrink-0">
        <span className="inline-flex h-7 w-7 items-center justify-center rounded-md border border-ink-200 bg-ink-100 text-ink-900">
          <BotIcon />
        </span>
        <div className="min-w-0">
          <div className="text-xs font-medium text-ink-900 truncate">
            {agent.display_name}
          </div>
          <div className="flex items-center gap-1.5 text-2xs text-ink-400">
            <span
              className={cn(
                "h-1 w-1 rounded-full",
                revoked ? "bg-ink-300" : "bg-accent",
              )}
            />
            <span className="truncate">{revoked ? t("agents.row.revoked") : lastSeenLabel}</span>
          </div>
        </div>
      </div>
      <div className="flex-1 flex items-center justify-end gap-1.5">
        {ENTITY_TYPES.map((et) => {
          const cur = permMap[et] ?? "none";
          return (
            <button
              key={et}
              type="button"
              onClick={() => onCycle(agent.id, et, cur)}
              className={cn(
                "w-10 h-6 inline-flex items-center justify-center rounded hover:bg-ink-100 transition-colors",
              )}
              aria-label={`${agent.id} · ${et} = ${cur}`}
              title={`${et}: ${cur}`}
            >
              {cur === "read" && (
                <span className="h-1.5 w-1.5 rounded-full bg-accent" />
              )}
              {cur === "write" && (
                <span className="h-1.5 w-1.5 rounded-full bg-ink-900" />
              )}
              {cur === "none" && <span className="h-px w-1.5 bg-ink-300" />}
            </button>
          );
        })}
      </div>
      <div className="w-16 text-right font-mono text-xs font-medium text-ink-900 shrink-0">
        {totalReads}
      </div>
      <div className="w-8 flex justify-end shrink-0">
        <button
          type="button"
          onClick={() => {
            if (armed) {
              onDelete(agent.id);
              setArmed(false);
            } else {
              setArmed(true);
            }
          }}
          className={cn(
            "h-6 w-6 inline-flex items-center justify-center rounded text-ink-400 hover:text-ink-900 hover:bg-ink-100 transition-colors",
            armed && "bg-rose-50 text-rose-600 hover:bg-rose-100 hover:text-rose-700",
          )}
          aria-label={`Remove agent ${agent.id}`}
          title={armed ? `click again to remove ${agent.id}` : `remove ${agent.id}`}
        >
          {armed ? <span className="font-mono text-[10px] leading-none">DEL?</span> : <XIcon />}
        </button>
      </div>
    </div>
  );
}

function PlusIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 5v14 M5 12h14" />
    </svg>
  );
}

function XIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 6l12 12 M6 18L18 6" />
    </svg>
  );
}

function BotIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="8" width="18" height="12" rx="2" />
      <path d="M12 5v3 M9 13h.01 M15 13h.01" />
    </svg>
  );
}
