// Thin wrappers around Tauri's invoke() with proper typing.

import type {
  Agent,
  AliasRow,
  AuditEntry,
  AuditVerification,
  CaptureResult,
  ConsolidationRun,
  ConsolidationStats,
  DedupCandidate,
  Entity,
  EntityDetail,
  EntityEditRow,
  MergeReport,
  Note,
  Permission,
  RankedEntity,
  SearchPayload,
  SemanticMemory,
} from "./types";

// `@tauri-apps/api` only exists at runtime inside Tauri's webview. In Next.js
// dev mode (browser), the module imports still succeed but invoke()/listen()
// will throw. We guard with a runtime check so the UI can stub-render outside
// of Tauri.

type InvokeFn = <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type ListenFn = <T = unknown>(
  event: string,
  cb: (e: { payload: T }) => void,
) => Promise<() => void>;

let _invoke: InvokeFn | null = null;
let _listen: ListenFn | null = null;

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function getInvoke(): Promise<InvokeFn> {
  if (_invoke) return _invoke;
  if (isTauri()) {
    const mod = await import("@tauri-apps/api/core");
    _invoke = mod.invoke as unknown as InvokeFn;
  } else {
    _invoke = async () => {
      throw new Error("Tauri runtime not available. Run `pnpm tauri:dev`.");
    };
  }
  return _invoke;
}

async function getListen(): Promise<ListenFn> {
  if (_listen) return _listen;
  if (isTauri()) {
    const mod = await import("@tauri-apps/api/event");
    _listen = mod.listen as unknown as ListenFn;
  } else {
    _listen = async () => () => {};
  }
  return _listen;
}

export async function captureNote(text: string, source?: string): Promise<CaptureResult> {
  return (await getInvoke())<CaptureResult>("capture_note", { text, source });
}

export async function reprocessNote(noteId: string): Promise<void> {
  return (await getInvoke())<void>("reprocess_note", { noteId });
}

export async function listNotes(limit = 50): Promise<Note[]> {
  return (await getInvoke())<Note[]>("list_notes", { limit });
}

export async function deleteNote(noteId: string): Promise<void> {
  return (await getInvoke())<void>("delete_note", { noteId });
}

export async function listEntities(entityType?: string, limit = 100): Promise<Entity[]> {
  return (await getInvoke())<Entity[]>("list_entities", { entityType, limit });
}

export async function getEntity(id: string): Promise<EntityDetail> {
  return (await getInvoke())<EntityDetail>("get_entity", { id });
}

// Editable memory (Phase 7 · #81)

export async function updateEntity(args: {
  id: string;
  name?: string;
  entity_type?: string;
  properties?: string;
}): Promise<Entity> {
  return (await getInvoke())<Entity>("update_entity", { args });
}

export async function updateNote(args: {
  id: string;
  text: string;
  reextract?: boolean;
}): Promise<void> {
  return (await getInvoke())<void>("update_note", { args });
}

export async function createEntity(args: {
  entity_type: string;
  name: string;
  detail?: string;
}): Promise<Entity> {
  return (await getInvoke())<Entity>("create_entity", { args });
}

// Edit history (Phase 7 · #82)

export async function listEntityEdits(args: {
  entity_id: string;
  limit?: number;
}): Promise<EntityEditRow[]> {
  return (await getInvoke())<EntityEditRow[]>("list_entity_edits", {
    entityId: args.entity_id,
    limit: args.limit,
  });
}

export async function entityEditCounts(args: {
  ids: string[];
}): Promise<Record<string, number>> {
  if (args.ids.length === 0) return {};
  return (await getInvoke())<Record<string, number>>("entity_edit_counts", {
    ids: args.ids,
  });
}

export async function search(
  query: string,
  types?: string[],
  limit = 20,
): Promise<SearchPayload> {
  return (await getInvoke())<SearchPayload>("search", { query, types, limit });
}

export async function listAgents(): Promise<Agent[]> {
  return (await getInvoke())<Agent[]>("list_agents");
}

export async function addAgent(id: string, displayName: string): Promise<void> {
  return (await getInvoke())<void>("add_agent", { id, displayName });
}

export async function deleteAgent(id: string): Promise<number> {
  return (await getInvoke())<number>("delete_agent", { id });
}

export async function listPermissions(agentId: string): Promise<Permission[]> {
  return (await getInvoke())<Permission[]>("list_permissions", { agentId });
}

export async function grantPermission(
  agentId: string,
  entityType: string,
  scope: "read" | "write" | "none",
): Promise<void> {
  return (await getInvoke())<void>("grant_permission", { agentId, entityType, scope });
}

export async function revokePermission(agentId: string, entityType: string): Promise<void> {
  return (await getInvoke())<void>("revoke_permission", { agentId, entityType });
}

export async function listAudit(limit = 100): Promise<AuditEntry[]> {
  return (await getInvoke())<AuditEntry[]>("list_audit", { limit });
}

export async function verifyAudit(limit = 500): Promise<AuditVerification> {
  return (await getInvoke())<AuditVerification>("verify_audit", { limit });
}

export async function auditPublicKey(): Promise<string> {
  return (await getInvoke())<string>("audit_public_key");
}

// Merge persistence — alias routing + unmerge (Phase 7 · #84)
//
// IPC arg shape: the Rust handlers `list_entity_aliases` and
// `remove_entity_alias` take their args as top-level fields (`entity_id` /
// `alias_id`), not wrapped in an `args: …` envelope. Tauri auto-converts
// camelCase keys from the JS side to snake_case Rust params, matching the
// `listEntityEdits` convention.

export async function listEntityAliases(args: { entity_id: string }): Promise<AliasRow[]> {
  return (await getInvoke())<AliasRow[]>("list_entity_aliases", {
    entityId: args.entity_id,
  });
}

export async function removeEntityAlias(args: { alias_id: string }): Promise<void> {
  return (await getInvoke())<void>("remove_entity_alias", {
    aliasId: args.alias_id,
  });
}

// Dedup + signal-sorted listing + archive (Phase 7 · #83)
//
// IPC arg shape: the Rust `merge_entities` / `reject_dedup_pair` /
// `set_entity_archived` handlers take a single `args: SomethingArgs` param,
// which means Tauri exposes them with a top-level `args` key (same shape as
// the existing `updateEntity`). The `list_*` / `auto_archive_now` handlers
// take their args directly, so we pass them at the top level — Tauri's
// auto-snake-case maps `limit` ↔ `limit` (no transformation needed).

export async function listDedupCandidates(limit: number): Promise<DedupCandidate[]> {
  return (await getInvoke())<DedupCandidate[]>("list_dedup_candidates", { limit });
}

export async function mergeEntities(
  keepId: string,
  dropId: string,
): Promise<MergeReport> {
  return (await getInvoke())<MergeReport>("merge_entities", {
    args: { keep_id: keepId, drop_id: dropId },
  });
}

export async function rejectDedupPair(
  keepId: string,
  dropId: string,
): Promise<void> {
  return (await getInvoke())<void>("reject_dedup_pair", {
    args: { keep_id: keepId, drop_id: dropId },
  });
}

export async function listEntitiesRanked(limit: number): Promise<RankedEntity[]> {
  return (await getInvoke())<RankedEntity[]>("list_entities_ranked", { limit });
}

export async function setEntityArchived(id: string, archived: boolean): Promise<void> {
  return (await getInvoke())<void>("set_entity_archived", {
    args: { id, archived },
  });
}

export async function listArchivedEntities(limit: number): Promise<Entity[]> {
  return (await getInvoke())<Entity[]>("list_archived_entities", { limit });
}

export async function autoArchiveNow(): Promise<number> {
  return (await getInvoke())<number>("auto_archive_now");
}

// Hierarchical memory

export async function listThemes(limit = 200): Promise<SemanticMemory[]> {
  return (await getInvoke())<SemanticMemory[]>("list_themes", { limit });
}

export async function getTheme(id: string): Promise<SemanticMemory> {
  return (await getInvoke())<SemanticMemory>("get_theme", { id });
}

export async function listConsolidationRuns(limit = 50): Promise<ConsolidationRun[]> {
  return (await getInvoke())<ConsolidationRun[]>("list_consolidation_runs", { limit });
}

export async function triggerConsolidation(): Promise<ConsolidationStats> {
  return (await getInvoke())<ConsolidationStats>("trigger_consolidation");
}

export async function reembedPending(): Promise<number> {
  return (await getInvoke())<number>("reembed_pending");
}

// Retrieval signals (Phase 3d / 4a)

export async function logRetrieval(
  queryText: string,
  types: string[] | undefined,
  returnedIds: string[],
): Promise<string> {
  return (await getInvoke())<string>("log_retrieval", {
    queryText,
    types,
    returnedIds,
  });
}

export async function recordSignalAction(
  signalId: string,
  action: "click" | "dismiss" | "correct",
  entityId: string,
): Promise<void> {
  return (await getInvoke())<void>("record_signal_action", {
    signalId,
    action,
    entityId,
  });
}

// Usage telemetry (Phase 4c)

export async function listUsage(limit = 200): Promise<import("./types").UsageRow[]> {
  return (await getInvoke())<import("./types").UsageRow[]>("list_usage", { limit });
}

export async function usageSummary(
  days = 14,
): Promise<import("./types").UsageDaySummary[]> {
  return (await getInvoke())<import("./types").UsageDaySummary[]>("usage_summary", {
    days,
  });
}

// Capture sources (Phase 5)

export async function listCaptureSources(): Promise<import("./types").CaptureSource[]> {
  return (await getInvoke())<import("./types").CaptureSource[]>("list_capture_sources");
}

export async function addImapSource(args: {
  label: string;
  host: string;
  port?: number;
  username: string;
  password: string;
  folder?: string;
  pollIntervalSecs?: number;
}): Promise<string> {
  return (await getInvoke())<string>("add_imap_source", {
    args: {
      label: args.label,
      host: args.host,
      port: args.port,
      username: args.username,
      password: args.password,
      folder: args.folder,
      poll_interval_secs: args.pollIntervalSecs,
    },
  });
}

export async function addIcsSource(args: {
  label: string;
  url: string;
  pollIntervalSecs?: number;
}): Promise<string> {
  return (await getInvoke())<string>("add_ics_source", {
    args: {
      label: args.label,
      url: args.url,
      poll_interval_secs: args.pollIntervalSecs,
    },
  });
}

export async function setCaptureSourceEnabled(
  id: string,
  enabled: boolean,
): Promise<void> {
  return (await getInvoke())<void>("set_capture_source_enabled", { id, enabled });
}

export async function deleteCaptureSource(id: string): Promise<void> {
  return (await getInvoke())<void>("delete_capture_source", { id });
}

// Selection popover (Phase 6a)

export async function selectionStatus(): Promise<import("./types").SelectionStatus> {
  return (await getInvoke())<import("./types").SelectionStatus>("selection_status");
}

export async function selectionSetEnabled(
  enabled: boolean,
): Promise<import("./types").SelectionStatus> {
  return (await getInvoke())<import("./types").SelectionStatus>("selection_set_enabled", {
    enabled,
  });
}

export async function selectionRequestPermission(): Promise<import("./types").SelectionStatus> {
  return (await getInvoke())<import("./types").SelectionStatus>("selection_request_permission");
}

export async function selectionGetCurrent(): Promise<import("./types").SelectionEvent | null> {
  return (await getInvoke())<import("./types").SelectionEvent | null>("selection_get_current");
}

export async function selectionDismiss(): Promise<void> {
  return (await getInvoke())<void>("selection_dismiss");
}

export async function selectionSaveWithNote(): Promise<void> {
  return (await getInvoke())<void>("selection_save_with_note");
}

// Import — Phase 7 V6

export async function importClaudeAutoRun(args: {
  home?: string;
  dry_run?: boolean;
  /// Absolute paths to skip on Apply (legacy per-run opt-out).
  exclude_paths?: string[];
  /// Unlinked paths the user chose to restore — force-reimported,
  /// bypassing the content-hash dedup.
  force_paths?: string[];
}): Promise<import("./types").ImportSummary> {
  return (await getInvoke())<import("./types").ImportSummary>("import_claude_auto_run", { args });
}

/// Read a memory-source file for the Import-page preview. Path is restricted
/// server-side to `~/.claude`, `~/.cursor/skills-cursor`, `~/.cursor/plans`,
/// `~/.codex/memories`. Truncated to 16 KB with a marker.
export async function readImportSourceFile(path: string): Promise<string> {
  return (await getInvoke())<string>("read_import_source_file", { path });
}

/// Force a single discovered file back into Cairn, bypassing content-hash
/// dedup. Backs the per-row "re-sync" button — restores an Unlinked file
/// (note was deleted) or re-runs extraction on an unchanged one. Returns the
/// summary for just that file.
export async function importResyncFile(
  path: string,
): Promise<import("./types").ImportSummary> {
  return (await getInvoke())<import("./types").ImportSummary>("import_resync_file", { path });
}

/// Mark a discovered file "do not sync" (persistent) or clear that flag.
/// Skipped files show as SKIPPED and are never imported until unskipped.
export async function importSetSkip(path: string, skipped: boolean): Promise<void> {
  await (await getInvoke())<void>("import_set_skip", { path, skipped });
}

// Export — Phase 7 V6 M2

export async function exportPreview(args: {
  home?: string;
  max_entities?: number;
}): Promise<import("./types").ExportPreview> {
  return (await getInvoke())<import("./types").ExportPreview>("export_preview", { args });
}

export async function exportRun(args: {
  home?: string;
  force?: boolean;
  max_entities?: number;
}): Promise<import("./types").ExportResult> {
  return (await getInvoke())<import("./types").ExportResult>("export_run", { args });
}

export async function vacuumOrphanVecs(): Promise<import("./types").VacuumOrphanReport> {
  return (await getInvoke())<import("./types").VacuumOrphanReport>("vacuum_orphan_vecs");
}

// AI provider config (Phase 6d)

export async function providersGet(): Promise<import("./types").ProvidersView> {
  return (await getInvoke())<import("./types").ProvidersView>("providers_get");
}

export async function providersSave(
  config: import("./types").ProvidersConfig,
): Promise<import("./types").ProvidersView> {
  return (await getInvoke())<import("./types").ProvidersView>("providers_save", { config });
}

export async function providersTest(
  slot: "extract" | "embed",
  config: import("./types").ProviderConfig,
): Promise<import("./types").ProviderTestResult> {
  return (await getInvoke())<import("./types").ProviderTestResult>("providers_test", {
    slot,
    config,
  });
}

// Remote MCP bridge (Phase 6b)

export async function mcpBridgeStatus(): Promise<import("./types").McpBridgeStatus> {
  return (await getInvoke())<import("./types").McpBridgeStatus>("mcp_bridge_status");
}

export async function mcpBridgeSetEnabled(
  enabled: boolean,
): Promise<import("./types").McpBridgeStatus> {
  return (await getInvoke())<import("./types").McpBridgeStatus>("mcp_bridge_set_enabled", {
    enabled,
  });
}

export async function onSelectionShow(
  cb: (payload: import("./types").SelectionEvent) => void,
): Promise<() => void> {
  const listen = await getListen();
  return listen<import("./types").SelectionEvent>("selection_show", (e) => cb(e.payload));
}

export async function onImportChanged(
  cb: (payload: import("./types").ImportSummary) => void,
): Promise<() => void> {
  const listen = await getListen();
  return listen<import("./types").ImportSummary>("import:changed", (e) => cb(e.payload));
}

// Events

export async function onNoteProcessed(
  cb: (payload: { note_id: string; entity_ids?: string[]; error?: string }) => void,
): Promise<() => void> {
  const listen = await getListen();
  return listen<{ note_id: string; entity_ids?: string[]; error?: string }>(
    "note_processed",
    (e) => cb(e.payload),
  );
}

export async function onNoteCaptured(
  cb: (payload: { note_id: string; text: string }) => void,
): Promise<() => void> {
  const listen = await getListen();
  return listen<{ note_id: string; text: string }>("note_captured", (e) => cb(e.payload));
}
