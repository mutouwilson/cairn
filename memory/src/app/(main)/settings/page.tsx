"use client";
import { useEffect, useState } from "react";
import Link from "next/link";
import { useLocale, useT, type Locale } from "@/lib/i18n";
import {
  addIcsSource,
  addImapSource,
  auditPublicKey,
  deleteCaptureSource,
  exportPreview,
  listAgents,
  listCaptureSources,
  mcpBridgeSetEnabled,
  mcpBridgeStatus,
  providersGet,
  providersSave,
  providersTest,
  selectionRequestPermission,
  selectionSetEnabled,
  selectionStatus,
  setCaptureSourceEnabled,
  vacuumOrphanVecs,
} from "@/lib/tauri";
import type {
  Agent,
  CaptureSource,
  McpBridgeStatus,
  ProviderConfig,
  ProviderFamily,
  ProviderPresetView,
  ProvidersConfig,
  ProvidersView,
  SelectionStatus,
  VacuumOrphanReport,
} from "@/lib/types";
import { cn, relativeTime, safeJsonParse } from "@/lib/utils";
import {
  BtnPrimary,
  BtnSecondary,
  Card,
  StatusPill,
} from "@/components/ui";

export default function SettingsPage() {
  const t = useT();
  return (
    <div className="max-w-3xl mx-auto px-8 py-8 space-y-4">
      <header className="space-y-1">
        <h1 className="font-display text-xl font-semibold text-ink-900">{t("settings.title")}</h1>
        <p className="font-mono text-2xs text-ink-400">
          Cairn lives in ~/Library/Application Support/cairn · v0.1.0
        </p>
      </header>

      <LanguageSection />
      <IdentitySection />
      <AiProvidersSection />
      <SelectionPopoverSection />
      <RemoteMcpSection />
      <ConnectorsSection />
      <MaintenanceSection />
      <CaptureSourcesSection />
      <EscapeHatchSection />
    </div>
  );
}

// ---------- Language ----------

function LanguageSection() {
  const t = useT();
  const { locale, effectiveLabel, setLocale } = useLocale();

  // Three-state segmented control. The "Auto" segment is what tracks the
  // user's system locale; "中文" / "English" override it.
  const options: { value: Locale; labelKey: string }[] = [
    { value: "auto", labelKey: "settings.language.opt_auto" },
    { value: "zh-CN", labelKey: "settings.language.opt_zh_cn" },
    { value: "en", labelKey: "settings.language.opt_en" },
  ];

  const captionKey =
    locale === "auto"
      ? "settings.language.current_auto"
      : "settings.language.current_manual";

  return (
    <SectionCard
      icon={<LanguageIcon />}
      title={t("settings.language.section_title")}
      sub={t("settings.language.description")}
    >
      <div className="px-5 py-3 border-t border-ink-200 flex flex-wrap items-center justify-between gap-3">
        <p className="text-2xs text-ink-500 font-mono">
          {t(captionKey, { locale: effectiveLabel })}
        </p>
        <div
          role="radiogroup"
          aria-label={t("settings.language.section_title")}
          className="inline-flex items-center rounded-md border border-ink-200 bg-ink-100/60 p-0.5"
        >
          {options.map((opt) => {
            const selected = locale === opt.value;
            return (
              <button
                key={opt.value}
                type="button"
                role="radio"
                aria-checked={selected}
                onClick={() => setLocale(opt.value)}
                className={cn(
                  "h-7 px-3 rounded text-xs font-medium transition-colors",
                  selected
                    ? "bg-white text-ink-900 shadow-sm border border-ink-200"
                    : "text-ink-500 hover:text-ink-900",
                )}
              >
                {t(opt.labelKey)}
              </button>
            );
          })}
        </div>
      </div>
    </SectionCard>
  );
}


// ---------- Identity ----------

function IdentitySection() {
  const t = useT();
  const [pub, setPub] = useState<string>("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    auditPublicKey()
      .then(setPub)
      .catch(() => setPub(""));
  }, []);

  async function copy() {
    if (!pub) return;
    try {
      await navigator.clipboard.writeText(pub);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* ignore */
    }
  }

  return (
    <SectionCard
      icon={<KeyIcon />}
      title={t("settings.identity.title")}
      sub={t("settings.identity.sub")}
    >
      <Row
        label={t("settings.identity.public_key")}
        value={
          pub ? (
            <code className="font-mono text-2xs text-ink-500 break-all">
              {pub.slice(0, 8)}…{pub.slice(-4)}
            </code>
          ) : (
            <span className="text-2xs text-ink-400">{t("common.loading")}</span>
          )
        }
        right={
          <BtnSecondary onClick={copy} disabled={!pub}>
            <CopyIcon /> {copied ? t("common.copied") : t("common.copy")}
          </BtnSecondary>
        }
      />
      <Row
        label={t("settings.identity.private_key_location")}
        value={
          <code className="font-mono text-2xs text-ink-400">
            ~/Library/Application Support/cairn/keystore
          </code>
        }
        right={
          <StatusPill tone="accent">
            <ShieldIcon /> {t("settings.identity.secure_enclave")}
          </StatusPill>
        }
      />
    </SectionCard>
  );
}

// ---------- AI Providers ----------

function AiProvidersSection() {
  const t = useT();
  const [view, setView] = useState<ProvidersView | null>(null);
  const [draft, setDraft] = useState<ProvidersConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedMsg, setSavedMsg] = useState<string | null>(null);
  const [testMsg, setTestMsg] = useState<{ slot: string; ok: boolean; text: string } | null>(null);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    try {
      const v = await providersGet();
      setView(v);
      setDraft(v.config);
    } catch (e) {
      console.error("providersGet failed", e);
    }
  }

  function presetOf(family: ProviderFamily): ProviderPresetView | undefined {
    return view?.presets.find((p) => p.family === family);
  }

  function applyDraft(updater: (d: ProvidersConfig) => ProvidersConfig) {
    setDraft((prev) => (prev ? updater(prev) : prev));
  }

  function setSlotFamily(slot: "extract" | "embed", family: ProviderFamily | "") {
    if (!view) return;
    if (family === "") {
      applyDraft((d) => ({ ...d, [slot]: null }));
      return;
    }
    const preset = view.presets.find((p) => p.family === family);
    if (!preset) return;
    const defaultModel =
      slot === "extract"
        ? preset.default_extract_model
        : preset.default_embed_model ?? "";
    applyDraft((d) => ({
      ...d,
      [slot]: {
        family,
        api_key: d[slot]?.api_key ?? "",
        model: defaultModel,
        base_url_override: null,
      },
    }));
  }

  function updateSlot(slot: "extract" | "embed", patch: Partial<ProviderConfig>) {
    applyDraft((d) => {
      const cur = d[slot];
      if (!cur) return d;
      return { ...d, [slot]: { ...cur, ...patch } };
    });
  }

  async function save() {
    if (!draft) return;
    setSaving(true);
    setSavedMsg(null);
    try {
      const v = await providersSave(draft);
      setView(v);
      setSavedMsg(v.restart_required ? "saved · restart Cairn to apply" : "saved");
    } catch (e) {
      setSavedMsg(`error: ${(e as Error).message}`);
    } finally {
      setSaving(false);
    }
  }

  async function test(slot: "extract" | "embed") {
    if (!draft) return;
    const cfg = draft[slot];
    if (!cfg) {
      setTestMsg({ slot, ok: false, text: "no provider configured" });
      return;
    }
    setTestMsg({ slot, ok: true, text: "testing…" });
    try {
      const r = await providersTest(slot, cfg);
      setTestMsg({ slot, ok: r.ok, text: r.message });
    } catch (e) {
      setTestMsg({ slot, ok: false, text: (e as Error).message });
    }
  }

  if (!view || !draft) return null;
  const embeddingFamilies = view.presets.filter((p) => p.supports_embedding);

  return (
    <SectionCard
      icon={<CpuIcon />}
      title={t("settings.extraction.title")}
      sub={t("settings.extraction.sub")}
    >
      <div className="px-5 py-4 space-y-4 border-t border-ink-200">
        <SlotEditor
          slot="extract"
          label={t("settings.extraction.active_extract")}
          draft={draft.extract ?? null}
          presets={view.presets}
          onFamily={(f) => setSlotFamily("extract", f)}
          onPatch={(p) => updateSlot("extract", p)}
          onTest={() => test("extract")}
          testing={testMsg?.slot === "extract" ? testMsg : null}
          preset={draft.extract ? presetOf(draft.extract.family) : undefined}
        />
        <div className="border-t border-ink-200 pt-4">
          <SlotEditor
            slot="embed"
            label={t("settings.extraction.active_embed")}
            draft={draft.embed ?? null}
            presets={embeddingFamilies}
            onFamily={(f) => setSlotFamily("embed", f)}
            onPatch={(p) => updateSlot("embed", p)}
            onTest={() => test("embed")}
            testing={testMsg?.slot === "embed" ? testMsg : null}
            preset={draft.embed ? presetOf(draft.embed.family) : undefined}
          />
        </div>
        <div className="flex items-center gap-3 pt-2 border-t border-ink-200">
          <BtnPrimary onClick={save} disabled={saving}>
            {saving ? t("common.saving") : t("common.save")}
          </BtnPrimary>
          {savedMsg && (
            <span
              className={cn(
                "text-2xs",
                savedMsg.startsWith("error") ? "text-danger" : "text-accent-fg",
              )}
            >
              {savedMsg}
            </span>
          )}
          {view.restart_required && !savedMsg && (
            <span className="text-2xs text-warn">
              saved config differs from running instance — restart to apply
            </span>
          )}
        </div>
      </div>
    </SectionCard>
  );
}

function SlotEditor({
  slot,
  label,
  draft,
  presets,
  preset,
  onFamily,
  onPatch,
  onTest,
  testing,
}: {
  slot: "extract" | "embed";
  label: string;
  draft: ProviderConfig | null;
  presets: ProviderPresetView[];
  preset: ProviderPresetView | undefined;
  onFamily: (f: ProviderFamily | "") => void;
  onPatch: (p: Partial<ProviderConfig>) => void;
  onTest: () => void;
  testing: { ok: boolean; text: string } | null;
}) {
  const t = useT();
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-xs font-medium text-ink-900">{label}</div>
          <div className="font-mono text-2xs text-ink-400 mt-0.5">
            {draft?.family ? `${draft.family} · ${draft.model || "—"}` : t("common.disabled")}
          </div>
        </div>
        <select
          value={draft?.family ?? ""}
          onChange={(e) => onFamily(e.target.value as ProviderFamily | "")}
          className="rounded-md border border-ink-200 bg-white px-2 h-8 text-xs focus:outline-none focus:border-ink-900"
        >
          <option value="">— disabled —</option>
          {presets.map((p) => (
            <option key={p.family} value={p.family}>
              {p.display_name}
            </option>
          ))}
        </select>
      </div>

      {draft && (
        <>
          <div className="grid grid-cols-3 gap-2">
            <Field
              label={t("settings.extraction.model")}
              value={draft.model}
              placeholder={preset?.default_extract_model ?? ""}
              onChange={(v) => onPatch({ model: v })}
              mono
            />
            <Field
              label={t("settings.extraction.api_key")}
              value={draft.api_key}
              type="password"
              placeholder={t("settings.extraction.key_paste_hint", { provider: preset?.display_name ?? "" })}
              onChange={(v) => onPatch({ api_key: v })}
              mono
            />
            <Field
              label={t("settings.extraction.base_url")}
              value={draft.base_url_override ?? ""}
              placeholder={preset?.api_base ?? ""}
              onChange={(v) => onPatch({ base_url_override: v || null })}
              mono
            />
          </div>
          <div className="flex items-center gap-2">
            <BtnSecondary onClick={onTest}>{t("settings.extraction.test_btn")}</BtnSecondary>
            {testing && (
              <span
                className={cn(
                  "text-2xs",
                  testing.ok ? "text-accent-fg" : "text-danger",
                )}
              >
                {testing.text}
              </span>
            )}
            {!preset?.supports_embedding && slot === "embed" && (
              <span className="text-2xs text-warn">no embedding API</span>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function Field({
  label,
  value,
  placeholder,
  onChange,
  type,
  mono,
}: {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (v: string) => void;
  type?: string;
  mono?: boolean;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="font-mono text-2xs text-ink-400 uppercase tracking-caps">
        {label}
      </span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className={cn(
          "rounded-md border border-ink-200 bg-white px-2 h-8 text-xs focus:outline-none focus:border-ink-900",
          mono && "font-mono",
        )}
      />
    </label>
  );
}

// ---------- Selection Popover ----------

function SelectionPopoverSection() {
  const t = useT();
  const [status, setStatus] = useState<SelectionStatus | null>(null);
  const [busy, setBusy] = useState(false);

  async function refresh() {
    try {
      setStatus(await selectionStatus());
    } catch {
      setStatus(null);
    }
  }
  useEffect(() => {
    refresh();
    // TCC permission state changes outside our process (user toggles
    // accessibility in System Settings). Re-read whenever:
    //  - the window regains focus,
    //  - the document becomes visible again,
    //  - on a low-rate timer as a fallback (some macOS builds don't fire
    //    focus events reliably for Tauri webviews).
    const onFocus = () => void refresh();
    const onVis = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVis);
    const timer = setInterval(() => void refresh(), 4000);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVis);
      clearInterval(timer);
    };
  }, []);

  async function toggle(next: boolean) {
    setBusy(true);
    try {
      setStatus(await selectionSetEnabled(next));
    } catch {
      /* ignore */
    } finally {
      setBusy(false);
    }
  }

  async function requestPermission() {
    setBusy(true);
    try {
      setStatus(await selectionRequestPermission());
    } finally {
      setBusy(false);
    }
  }

  if (!status) return null;

  return (
    <SectionCard
      icon={<PointerIcon />}
      title={t("settings.selection.title")}
      sub={t("settings.selection.sub")}
      right={
        <ToggleButton
          enabled={status.enabled}
          disabled={busy || !status.supported}
          onClick={() => toggle(!status.enabled)}
        />
      }
    >
      <div className="px-5 py-3 flex items-center justify-between gap-3 border-t border-ink-200">
        <span className="inline-flex items-center gap-2 text-2xs text-ink-500">
          <span
            className={cn(
              "h-1.5 w-1.5 rounded-full",
              status.permission_granted ? "bg-accent" : "bg-warn",
            )}
          />
          {!status.supported
            ? t("settings.selection.macos_only")
            : status.permission_granted
              ? t("settings.selection.permission_granted")
              : t("settings.selection.permission_missing")}
        </span>
        {status.supported && !status.permission_granted && (
          <BtnSecondary onClick={requestPermission} disabled={busy}>
            {t("settings.selection.open_system_settings")}
          </BtnSecondary>
        )}
      </div>
    </SectionCard>
  );
}

// ---------- Remote MCP bridge ----------

function RemoteMcpSection() {
  const t = useT();
  const [status, setStatus] = useState<McpBridgeStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  // "Identify as" — appended to the SSE URL as `?agent_id=<value>` so each
  // client (cursor, windsurf, …) lands in its own row on /agents instead of
  // collapsing into the bridge's default agent.
  const [identifyAs, setIdentifyAs] = useState("");

  async function refresh() {
    try {
      setStatus(await mcpBridgeStatus());
    } catch {
      setStatus(null);
    }
  }
  useEffect(() => {
    refresh();
  }, []);

  const cleanAgentId = identifyAs.trim().replace(/[^a-zA-Z0-9._-]/g, "");
  const displayUrl = (() => {
    if (!status) return "";
    if (!cleanAgentId) return status.local_url;
    const sep = status.local_url.includes("?") ? "&" : "?";
    return `${status.local_url}${sep}agent_id=${encodeURIComponent(cleanAgentId)}`;
  })();

  async function toggle(next: boolean) {
    setBusy(true);
    try {
      setStatus(await mcpBridgeSetEnabled(next));
    } finally {
      setBusy(false);
    }
  }

  async function copyUrl() {
    if (!displayUrl) return;
    try {
      await navigator.clipboard.writeText(displayUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* ignore */
    }
  }

  if (!status) return null;

  return (
    <SectionCard
      icon={<GlobeIcon />}
      title={t("settings.mcp.title")}
      sub={t("settings.mcp.sub")}
      right={
        <ToggleButton
          enabled={status.enabled}
          disabled={busy}
          onClick={() => toggle(!status.enabled)}
        />
      }
    >
      <div className="px-5 py-3 border-t border-ink-200 space-y-3">
        <div className="rounded-md bg-ink-100 px-3 py-2 flex items-center gap-2">
          <span className="font-mono text-2xs uppercase tracking-caps text-ink-500">
            {t("settings.mcp.local")}
          </span>
          <code className="flex-1 font-mono text-xs text-ink-700 truncate">
            {displayUrl}
          </code>
          <span
            className={cn(
              "inline-flex items-center gap-1 text-2xs uppercase tracking-caps",
              status.running ? "text-accent-fg" : "text-ink-500",
            )}
          >
            <span
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                status.running ? "bg-accent" : "bg-ink-300",
              )}
            />
            {status.running ? t("common.live") : t("common.stopped")}
          </span>
          <BtnSecondary onClick={copyUrl}>{copied ? t("common.copied") : t("common.copy")}</BtnSecondary>
        </div>

        <div className="flex items-center gap-2">
          <label className="font-mono text-2xs uppercase tracking-caps text-ink-500 shrink-0">
            {t("settings.mcp.identify_as")}
          </label>
          <input
            value={identifyAs}
            onChange={(e) => setIdentifyAs(e.target.value)}
            placeholder={t("settings.mcp.identify_placeholder")}
            className="flex-1 rounded-md border border-ink-200 bg-white px-3 h-8 text-xs font-mono focus:outline-none focus:border-ink-900"
          />
          <span className="font-mono text-2xs text-ink-400 shrink-0">
            {cleanAgentId ? t("settings.mcp.tag_hint", { id: cleanAgentId }) : t("settings.mcp.no_tag_hint")}
          </span>
        </div>

        {status.enabled && (
          <div className="text-2xs text-ink-500 space-y-1.5 leading-relaxed">
            <p>{t("settings.mcp.tunnel_hint_prefix")}</p>
            <pre className="rounded-md bg-ink-100 px-3 py-2 font-mono text-2xs whitespace-pre overflow-x-auto">
              {`cloudflared tunnel --url http://localhost:${status.port}`}
            </pre>
            <p>{t("settings.mcp.connector_hint")}</p>
          </div>
        )}
      </div>
    </SectionCard>
  );
}

// ---------- Hosted MCP connectors ----------
//
// One-tap config snippets for each known MCP host. The "MCP bridge" section
// above exposes the raw local URL + Identify-as input; this section turns
// that into ready-to-paste config tailored to each host, plus a live
// connected/idle status pill driven by the agents table's `last_seen`.
//
// The status badge resolves on a hardcoded id → host mapping. Anything
// unfamiliar that DOES connect still lands in /agents under its own id, but
// won't get a card here. Add new cards when we observe new hosts in the wild.

const CONNECTOR_PORT_DEFAULT = 7717;

// Live-recent threshold for the green "connected" pill. 5 minutes is loose
// enough that Claude Desktop is "connected" between calls without forcing
// the host to ping just to keep the dot on.
const CONNECTOR_LIVE_THRESHOLD_MS = 5 * 60 * 1000;

function ConnectorsSection() {
  const t = useT();
  const [agents, setAgents] = useState<Agent[]>([]);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  useEffect(() => {
    listAgents().then(setAgents).catch(() => setAgents([]));
    // Re-poll occasionally so the status pill updates as hosts reconnect.
    // 8 s is generous — this is a coarse "did it connect today" signal, not
    // a heartbeat. (Variable is `timer`, not `t`, so it doesn't shadow the
    // i18n hook bound to `t` above.)
    const timer = setInterval(() => {
      listAgents().then(setAgents).catch(() => {});
    }, 8000);
    return () => clearInterval(timer);
  }, []);

  function agentBy(id: string): Agent | undefined {
    return agents.find((a) => a.id === id);
  }

  function copy(key: string, text: string) {
    void navigator.clipboard.writeText(text).catch(() => {});
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 1500);
  }

  const cdSnippet = `"cairn": {
  "command": "/Applications/Cairn.app/Contents/MacOS/cairn-mcp",
  "args": [],
  "env": {
    "CAIRN_AGENT_ID": "claude-desktop"
  }
}`;

  const cursorSnippet = `"cairn": {
  "url": "http://127.0.0.1:${CONNECTOR_PORT_DEFAULT}/sse?agent_id=cursor"
}`;

  const tunnelTemplate = `https://<your-tunnel>/sse?agent_id=chatgpt`;

  return (
    <SectionCard
      icon={<PlugIcon />}
      title={t("settings.connectors.section_title")}
      sub={t("settings.connectors.section_sub")}
    >
      <div className="px-5 py-3 space-y-3 border-t border-ink-200">
        <ConnectorCard
          host="Claude Desktop"
          pathHint={t("settings.connectors.claude_desktop_path")}
          snippet={cdSnippet}
          agent={agentBy("claude-desktop")}
          copied={copiedKey === "cd"}
          onCopy={() => copy("cd", cdSnippet)}
        />
        <ConnectorCard
          host="Cursor"
          pathHint={t("settings.connectors.cursor_path")}
          snippet={cursorSnippet}
          agent={agentBy("cursor")}
          copied={copiedKey === "cur"}
          onCopy={() => copy("cur", cursorSnippet)}
        />
        <ConnectorCard
          host={t("settings.connectors.web_host")}
          pathHint={t("settings.connectors.web_path")}
          snippet={tunnelTemplate}
          extraNote={t("settings.connectors.web_extra")}
          agent={agentBy("chatgpt") ?? agentBy("claude-ai")}
          copied={copiedKey === "web"}
          onCopy={() => copy("web", tunnelTemplate)}
        />
      </div>
    </SectionCard>
  );
}

function ConnectorCard({
  host,
  pathHint,
  snippet,
  extraNote,
  agent,
  copied,
  onCopy,
}: {
  host: string;
  pathHint: string;
  snippet: string;
  extraNote?: string;
  agent?: Agent;
  copied: boolean;
  onCopy: () => void;
}) {
  const t = useT();
  const live =
    !!agent?.last_seen &&
    Date.now() - agent.last_seen < CONNECTOR_LIVE_THRESHOLD_MS;
  const seenAt = agent?.last_seen ? relativeTime(agent.last_seen) : null;

  return (
    <div className="rounded-md border border-ink-200 bg-white p-3 space-y-2">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs font-medium text-ink-900">{host}</div>
          <div className="text-2xs text-ink-500 mt-0.5">{pathHint}</div>
        </div>
        {agent ? (
          <span
            className={cn(
              "inline-flex items-center gap-1.5 text-2xs font-mono",
              live ? "text-accent-fg" : "text-ink-500",
            )}
          >
            <span
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                live ? "bg-accent" : "bg-ink-300",
              )}
            />
            {live
              ? t("settings.connectors.status_connected")
              : seenAt
                ? t("settings.connectors.status_last_seen", { when: seenAt })
                : t("settings.connectors.status_idle")}
          </span>
        ) : (
          <span className="inline-flex items-center gap-1.5 text-2xs font-mono text-ink-400">
            <span className="h-1.5 w-1.5 rounded-full bg-ink-200" />
            {t("settings.connectors.status_not_configured")}
          </span>
        )}
      </div>
      <div className="relative">
        <pre className="rounded bg-ink-100 px-3 py-2 font-mono text-2xs leading-relaxed text-ink-700 overflow-x-auto whitespace-pre">
          {snippet}
        </pre>
        <button
          type="button"
          onClick={onCopy}
          className="absolute top-1.5 right-1.5 rounded border border-ink-200 bg-white px-2 h-6 text-2xs font-medium text-ink-700 hover:bg-ink-100 transition-colors"
        >
          {copied ? t("common.copied") : t("common.copy")}
        </button>
      </div>
      {extraNote && (
        <p className="text-2xs text-ink-500 leading-relaxed">{extraNote}</p>
      )}
    </div>
  );
}

// ---------- Maintenance ----------

function MaintenanceSection() {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [armed, setArmed] = useState(false);
  const [report, setReport] = useState<VacuumOrphanReport | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function run() {
    setBusy(true);
    setErr(null);
    try {
      setReport(await vacuumOrphanVecs());
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
      setArmed(false);
    }
  }

  function handleClick() {
    if (!armed) {
      setArmed(true);
      setTimeout(() => setArmed(false), 4000);
      return;
    }
    void run();
  }

  return (
    <SectionCard
      icon={<BroomIcon />}
      title={t("settings.maintenance.title")}
      sub={t("settings.maintenance.sub")}
    >
      <div className="px-5 py-3 border-t border-ink-200 space-y-3">
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleClick}
            disabled={busy}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md h-8 px-3 text-xs font-medium border disabled:opacity-40 transition-colors",
              armed
                ? "border-danger bg-danger text-white"
                : "border-ink-200 bg-white text-ink-900 hover:bg-ink-100",
            )}
          >
            {busy
              ? t("settings.maintenance.vacuuming")
              : armed
                ? t("settings.maintenance.armed_btn")
                : t("settings.maintenance.vacuum_btn")}
          </button>
          {armed && !busy && (
            <span className="text-2xs text-ink-500">
              {t("settings.maintenance.armed_hint")}
            </span>
          )}
        </div>
        {err && (
          <div className="rounded-md border border-danger bg-danger-dim px-3 py-2 text-xs text-danger">
            {err}
          </div>
        )}
        {report && (
          <div className="rounded-md border border-accent/30 bg-accent-dim px-3 py-2 text-xs text-accent-fg space-y-1">
            <p>
              {t("settings.maintenance.report_template", {
                before: report.vec_rows_before,
                deleted: report.orphan_vecs_deleted,
                meta: report.orphan_meta_deleted,
              })}
            </p>
            <p className="text-ink-500">
              {t("settings.maintenance.report_file_template", {
                before: (report.bytes_before / 1024 / 1024).toFixed(1),
                after: (report.bytes_after / 1024 / 1024).toFixed(1),
                saved: ((report.bytes_before - report.bytes_after) / 1024 / 1024).toFixed(1),
              })}
            </p>
          </div>
        )}
      </div>
    </SectionCard>
  );
}

// ---------- Capture Sources ----------

function CaptureSourcesSection() {
  const [sources, setSources] = useState<CaptureSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [msg, setMsg] = useState<string | null>(null);
  const [showImap, setShowImap] = useState(false);
  const [showIcs, setShowIcs] = useState(false);

  async function refresh() {
    try {
      setSources(await listCaptureSources());
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    refresh();
  }, []);

  return (
    <SectionCard
      icon={<InboxIcon />}
      title="Capture sources"
      sub="Background pollers that feed your memory."
      right={
        <div className="flex items-center gap-2">
          <BtnSecondary onClick={() => setShowImap((v) => !v)}>+ IMAP</BtnSecondary>
          <BtnSecondary onClick={() => setShowIcs((v) => !v)}>+ ICS</BtnSecondary>
        </div>
      }
    >
      <div className="px-5 py-3 border-t border-ink-200 space-y-3">
        {msg && <p className="font-mono text-2xs text-accent-fg">{msg}</p>}

        {showImap && (
          <AddImapForm
            onAdded={(id) => {
              setMsg(`added IMAP source ${id}`);
              setShowImap(false);
              refresh();
            }}
            onCancel={() => setShowImap(false)}
          />
        )}
        {showIcs && (
          <AddIcsForm
            onAdded={(id) => {
              setMsg(`added ICS source ${id}`);
              setShowIcs(false);
              refresh();
            }}
            onCancel={() => setShowIcs(false)}
          />
        )}

        {loading ? (
          <p className="text-sm text-ink-500">loading…</p>
        ) : sources.length === 0 ? (
          <p className="text-sm text-ink-500">no sources yet.</p>
        ) : (
          <div className="rounded-md border border-ink-200 overflow-hidden">
            {sources.map((s, idx) => (
              <SourceRow
                key={s.id}
                source={s}
                first={idx === 0}
                onToggle={async () => {
                  await setCaptureSourceEnabled(s.id, s.enabled === 0);
                  refresh();
                }}
                onDelete={async (armed) => {
                  if (!armed) return;
                  await deleteCaptureSource(s.id);
                  refresh();
                }}
              />
            ))}
          </div>
        )}
      </div>
    </SectionCard>
  );
}

function SourceRow({
  source,
  first,
  onToggle,
  onDelete,
}: {
  source: CaptureSource;
  first: boolean;
  onToggle: () => void;
  onDelete: (armed: boolean) => void;
}) {
  const [delArmed, setDelArmed] = useState(false);
  const cfg = safeJsonParse<Record<string, unknown>>(source.config, {});
  const target =
    source.kind === "imap"
      ? `${cfg.username}@${cfg.host} → ${cfg.folder}`
      : String(cfg.url ?? "");
  const tone: "accent" | "danger" | "muted" =
    source.last_status === "ok"
      ? "accent"
      : source.last_status === "error"
        ? "danger"
        : "muted";

  function handleDelete() {
    if (!delArmed) {
      setDelArmed(true);
      setTimeout(() => setDelArmed(false), 4000);
      onDelete(false);
      return;
    }
    onDelete(true);
    setDelArmed(false);
  }

  return (
    <div
      className={cn(
        "flex items-center gap-3 px-3 py-3 text-xs",
        !first && "border-t border-ink-200",
        source.enabled === 0 && "opacity-70",
      )}
    >
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <StatusPill tone="muted">{source.kind}</StatusPill>
          <span className="text-xs font-medium text-ink-900 truncate">
            {source.label}
          </span>
          <StatusPill tone={tone} dot>
            {source.last_status ?? "idle"}
          </StatusPill>
          {source.enabled === 0 && <StatusPill tone="warn">disabled</StatusPill>}
        </div>
        <div className="font-mono text-2xs text-ink-500 truncate mt-1">
          {target}
        </div>
        <div className="text-2xs text-ink-400 mt-0.5">
          captured {source.captured_count} · interval {source.poll_interval_secs}s ·{" "}
          {source.last_polled_at
            ? `last poll ${relativeTime(source.last_polled_at)}`
            : "never polled"}
        </div>
        {source.last_error && (
          <div className="font-mono text-2xs text-danger mt-1 truncate">
            {source.last_error}
          </div>
        )}
      </div>
      <div className="flex flex-col gap-1 items-stretch">
        <BtnSecondary onClick={onToggle}>
          {source.enabled === 0 ? "enable" : "disable"}
        </BtnSecondary>
        <button
          type="button"
          onClick={handleDelete}
          className={cn(
            "inline-flex items-center justify-center rounded-md border h-7 px-2.5 text-2xs font-medium transition-colors",
            delArmed
              ? "border-danger bg-danger text-white"
              : "border-ink-200 bg-white text-danger hover:bg-danger-dim",
          )}
        >
          {delArmed ? "confirm" : "delete"}
        </button>
      </div>
    </div>
  );
}

function AddImapForm({
  onAdded,
  onCancel,
}: {
  onAdded: (id: string) => void;
  onCancel: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [host, setHost] = useState("imap.gmail.com");
  const [port, setPort] = useState(993);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [folder, setFolder] = useState("Cairn");

  return (
    <form
      className="rounded-md border border-ink-200 bg-ink-100/50 p-3 space-y-2"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        setErr(null);
        try {
          const id = await addImapSource({ label, host, port, username, password, folder });
          setPassword("");
          onAdded(id);
        } catch (e) {
          setErr((e as Error).message);
        } finally {
          setBusy(false);
        }
      }}
    >
      <div className="font-mono text-2xs uppercase tracking-caps text-ink-500">
        Add IMAP mailbox
      </div>
      <FormInput required value={label} onChange={setLabel} placeholder="label (e.g. Gmail personal)" />
      <div className="grid grid-cols-2 gap-2">
        <FormInput required value={host} onChange={setHost} placeholder="host" />
        <FormInput
          required
          type="number"
          value={String(port)}
          onChange={(v) => setPort(parseInt(v, 10) || 993)}
          placeholder="port"
        />
      </div>
      <FormInput required value={username} onChange={setUsername} placeholder="username (e.g. you@gmail.com)" />
      <FormInput
        required
        type="password"
        value={password}
        onChange={setPassword}
        placeholder="app password (stored in OS keychain)"
      />
      <FormInput value={folder} onChange={setFolder} placeholder="folder/label (default Cairn)" />
      {err && <p className="font-mono text-2xs text-danger">{err}</p>}
      <div className="flex gap-2">
        <BtnPrimary disabled={busy}>{busy ? "adding…" : "Add"}</BtnPrimary>
        <BtnSecondary onClick={onCancel} type="button">
          Cancel
        </BtnSecondary>
      </div>
    </form>
  );
}

function AddIcsForm({
  onAdded,
  onCancel,
}: {
  onAdded: (id: string) => void;
  onCancel: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [url, setUrl] = useState("");

  return (
    <form
      className="rounded-md border border-ink-200 bg-ink-100/50 p-3 space-y-2"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        setErr(null);
        try {
          const id = await addIcsSource({ label, url });
          setUrl("");
          onAdded(id);
        } catch (e) {
          setErr((e as Error).message);
        } finally {
          setBusy(false);
        }
      }}
    >
      <div className="font-mono text-2xs uppercase tracking-caps text-ink-500">
        Add calendar feed
      </div>
      <FormInput required value={label} onChange={setLabel} placeholder="label (e.g. Personal Google Calendar)" />
      <FormInput
        required
        type="url"
        value={url}
        onChange={setUrl}
        placeholder="https://calendar.google.com/calendar/ical/.../basic.ics"
      />
      <p className="text-2xs text-ink-400">
        Google: settings → Integrate calendar → &ldquo;Secret address in iCal format&rdquo;.
        iCloud: share calendar → make public → copy URL.
      </p>
      {err && <p className="font-mono text-2xs text-danger">{err}</p>}
      <div className="flex gap-2">
        <BtnPrimary disabled={busy}>{busy ? "adding…" : "Add"}</BtnPrimary>
        <BtnSecondary onClick={onCancel} type="button">
          Cancel
        </BtnSecondary>
      </div>
    </form>
  );
}

function FormInput({
  value,
  onChange,
  placeholder,
  type,
  required,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  required?: boolean;
}) {
  return (
    <input
      type={type}
      required={required}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      className="w-full rounded-md border border-ink-200 bg-white px-3 h-8 text-xs focus:outline-none focus:border-ink-900"
    />
  );
}

// ---------- Escape Hatch ----------

function EscapeHatchSection() {
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    exportPreview({})
      .then((p) => setCount(p.entity_count))
      .catch(() => setCount(null));
  }, []);

  const label = count != null ? `Preview ${count} entries` : "Open export panel";

  return (
    <div className="rounded-lg border border-accent/30 bg-accent-dim p-6 flex items-center justify-between gap-4">
      <div className="space-y-1.5 max-w-xl">
        <div className="flex items-center gap-2 text-accent-fg">
          <DoorIcon />
          <h2 className="font-display text-md font-semibold">
            Escape hatch · export everything
          </h2>
        </div>
        <p className="text-xs leading-relaxed text-accent-fg/80">
          Your data is always yours. The Export panel in{" "}
          <Link className="underline" href="/import">
            Import &amp; sync
          </Link>{" "}
          writes a human-readable markdown bundle. SQLite + audit chain remain on
          disk under <code className="font-mono">~/Library/Application Support/cairn</code>.
        </p>
      </div>
      <Link
        href="/import"
        className={cn(
          "inline-flex items-center gap-1.5 whitespace-nowrap rounded-md bg-accent px-3 h-8 text-xs font-medium text-white",
          "hover:bg-accent-fg transition-colors",
        )}
      >
        <DownloadIcon /> {label}
      </Link>
    </div>
  );
}

// ---------- Shared primitives ----------

function SectionCard({
  icon,
  title,
  sub,
  right,
  children,
}: {
  icon?: React.ReactNode;
  title: string;
  sub?: string;
  right?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Card padded={false} className="overflow-hidden">
      <div className="flex items-start justify-between gap-3 px-5 py-3">
        <div className="flex items-start gap-2.5 min-w-0">
          {icon && <span className="text-ink-900 mt-0.5">{icon}</span>}
          <div>
            <h2 className="font-display text-md font-semibold text-ink-900">
              {title}
            </h2>
            {sub && <p className="text-2xs text-ink-400 mt-0.5">{sub}</p>}
          </div>
        </div>
        {right}
      </div>
      {children}
    </Card>
  );
}

function Row({
  label,
  value,
  right,
}: {
  label: string;
  value: React.ReactNode;
  right?: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 px-5 py-3 border-t border-ink-200">
      <div className="min-w-0">
        <p className="text-xs font-medium text-ink-900">{label}</p>
        <div className="mt-0.5">{value}</div>
      </div>
      {right}
    </div>
  );
}

function ToggleButton({
  enabled,
  disabled,
  onClick,
}: {
  enabled: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md border h-7 px-2.5 text-2xs font-medium disabled:opacity-40 transition-colors",
        enabled
          ? "border-accent bg-accent-dim text-accent-fg"
          : "border-ink-200 bg-white text-ink-900 hover:bg-ink-100",
      )}
    >
      <span
        className={cn(
          "h-1.5 w-1.5 rounded-full",
          enabled ? "bg-accent" : "bg-ink-300",
        )}
      />
      {enabled ? "enabled" : "disabled"}
    </button>
  );
}

function KeyIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="15" r="4" />
      <path d="M21 2l-9 9 M15.5 6.5l3 3 M11 13l3 3" />
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

function ShieldIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z M9 12l2 2 4-4" />
    </svg>
  );
}

function CpuIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <rect x="9" y="9" width="6" height="6" />
      <path d="M9 2v2 M15 2v2 M9 20v2 M15 20v2 M2 9h2 M2 15h2 M20 9h2 M20 15h2" />
    </svg>
  );
}

function PointerIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 3l7.07 17 2.51-7.39L20 10.07z" />
    </svg>
  );
}

function GlobeIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" />
      <path d="M2 12h20 M12 2a15 15 0 0 1 0 20 M12 2a15 15 0 0 0 0 20" />
    </svg>
  );
}

function LanguageIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 5h12 M9 3v2 M5 8c0 5 5 8 8 8 M11 8c0 4-5 8-8 8 M14 19l4-9 4 9 M15.5 16h5" />
    </svg>
  );
}

function PlugIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 7V3 M15 7V3 M7 7h10v6a5 5 0 0 1-10 0V7z M12 18v3" />
    </svg>
  );
}

function BroomIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M19 3l-9 9 M17 5l2 2 M5 21l4-4 M9 17l-2-2-2 4 4-2" />
    </svg>
  );
}

function InboxIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M22 12h-6l-2 3h-4l-2-3H2 M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" />
    </svg>
  );
}

function DoorIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M13 4h3a2 2 0 0 1 2 2v14H5V4h3 M11 4v16 M14 12h.01" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M7 10l5 5 5-5 M12 15V3" />
    </svg>
  );
}

