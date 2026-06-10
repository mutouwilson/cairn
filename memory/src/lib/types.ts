// Mirrors src-tauri/src/schema.rs exactly. Keep in sync.

export const ENTITY_TYPES = [
  "Person",
  "Event",
  "Preference",
  "Belief",
  "Goal",
  "Asset",
  "Skill",
  "Location",
] as const;

export type EntityType = (typeof ENTITY_TYPES)[number];

export interface Note {
  id: string;
  text: string;
  source: string;
  created_at: number;
  processed_at: number | null;
  processing_status: "pending" | "done" | "failed";
  extraction_error: string | null;
}

export interface Entity {
  id: string;
  type: string;
  name: string;
  properties: string; // JSON string
  confidence: number;
  source_note_id: string | null;
  created_at: number;
  updated_at: number;
  strength: number;
  last_accessed: number;
  access_count: number;
  base_importance: number;
  // Soft-archive flag (Phase 7 · #83). null = active; ms timestamp = archived.
  // Older rows that pre-date the column are tolerated as `undefined`.
  archived_at?: number | null;
  // Sticky flag (Phase 7 · #84). 0/1 — pins the entity against import-watcher
  // cascade deletes. Set on manual edits, manual creation, or merge-keep.
  // Cleared by `remove_entity_alias` only when no aliases AND no manual edits
  // remain. Older rows that pre-date the column are tolerated as `undefined`.
  is_sticky?: number;
}

// Alias row (Phase 7 · #84). Mirrors `crate::db::queries::AliasRow`. One row
// per recorded merge — when a fresh extraction matches `name_lower` AND clears
// the cosine threshold against the keep's embedding under the same model id,
// the extracted entity routes onto `keep_id` instead of being inserted.
export interface AliasRow {
  id: string;
  keep_id: string;
  name_lower: string;
  embedding_hash: string | null;
  embedding_model_id: string | null;
  created_at: number;
  routed_count: number;
}

// Dedup + ranked listing (Phase 7 · #83). Mirrors
// `src-tauri/src/db/queries.rs` — `DedupCandidate` / `MergeReport` /
// `RankedEntity`. Field names are snake_case because serde keeps them
// as-is (no `rename_all`).
export interface DedupCandidate {
  keep_id: string;
  drop_id: string;
  /// Cosine similarity in [0, 1]; 1.0 = identical.
  similarity: number;
  keep_entity: Entity;
  drop_entity: Entity;
  keep_manual_edits: number;
  keep_note_count: number;
  keep_relation_count: number;
  drop_manual_edits: number;
  drop_note_count: number;
  drop_relation_count: number;
}

export interface MergeReport {
  relations_migrated: number;
  edits_migrated: number;
  vec_dropped: number;
  alias_dropped: number;
}

export interface RankedEntity {
  entity: Entity;
  final_score: number;
  manual_edits: number;
  theme_id: string | null;
  theme_title: string | null;
}

export interface Relation {
  id: string;
  from_entity: string;
  to_entity: string;
  type: string;
  properties: string | null;
  confidence: number;
  source_note_id: string | null;
  created_at: number;
}

export interface EntityDetail {
  entity: Entity;
  relations: Relation[];
}

export interface Agent {
  id: string;
  display_name: string;
  public_key: string | null;
  added_at: number;
  last_seen: number | null;
  revoked_at: number | null;
}

export interface Permission {
  agent_id: string;
  entity_type: string;
  scope: "read" | "write" | "none";
  granted_at: number;
  expires_at: number | null;
}

export interface AuditEntry {
  seq: number;
  id: string;
  agent_id: string;
  action: string;
  tool_name: string | null;
  arguments: string | null;
  result_hash: string;
  result_count: number | null;
  timestamp: number;
  prev_hash: string;
  hash: string;
  signature: string;
}

// Entity edit history (Phase 7 · #82). Mirrors
// `crate::db::queries::EntityEditRow`. value_before / value_after are
// pre-truncated server-side to 1024 chars.
export type EntityEditAction = "create" | "update" | "delete";

export interface EntityEditRow {
  id: string;
  entity_id: string;
  action: EntityEditAction | string;
  field_changed: string | null;
  value_before: string | null;
  value_after: string | null;
  edited_by: string;
  edited_at: number;
}

export interface SemanticMemory {
  id: string;
  topic: string;
  title: string;
  summary: string;
  evidence: string; // JSON-encoded array of entity ids
  confidence: number;
  created_at: number;
  last_consolidated_at: number;
  superseded_by: string | null;
  strength: number;
  last_accessed: number;
  access_count: number;
}

export interface ConsolidationRun {
  id: string;
  started_at: number;
  finished_at: number | null;
  trigger: "scheduled" | "manual" | "startup" | string;
  topics_scanned: number;
  semantic_created: number;
  semantic_updated: number;
  error: string | null;
  status: "running" | "done" | "failed" | string;
}

export interface ConsolidationStats {
  run_id: string;
  topics_scanned: number;
  semantic_created: number;
  semantic_updated: number;
  errors: string[];
}

export interface UsageRow {
  id: string;
  operation: string;
  provider: string;
  model: string;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  cost_micro_usd: number | null;
  latency_ms: number;
  success: number;
  error: string | null;
  occurred_at: number;
}

export interface UsageDaySummary {
  day: string;
  operation: string;
  provider: string;
  calls: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_micro_usd: number;
  p50_latency_ms: number;
}

export interface CaptureSource {
  id: string;
  kind: "imap" | "ics" | string;
  label: string;
  config: string;
  secret_account: string | null;
  enabled: number;
  poll_interval_secs: number;
  last_polled_at: number | null;
  last_status: string | null;
  last_error: string | null;
  captured_count: number;
  created_at: number;
  updated_at: number;
}

export interface RetrievedEntity {
  entity: Entity;
  bm25_score: number;
  vector_score: number; // 1 - cosine_distance; NaN if no vector hit
  rrf_score: number; // normalised [0,1]
  importance_score: number;
  recency_score: number;
  final_score: number;
}

export interface RetrievedNote {
  note: Note;
  bm25_score: number;
  recency_score: number;
  final_score: number;
}

export interface RetrievalDiagnostics {
  bm25_hits: number;
  vector_hits: number;
  fused_hits: number;
  vector_path_skipped_reason: string | null;
}

export interface SearchPayload {
  query: string;
  entities: RetrievedEntity[];
  notes: RetrievedNote[];
  diagnostics: RetrievalDiagnostics;
}

export interface AuditVerification {
  ok: boolean;
  message: string;
  entries_checked: number;
}

export interface CaptureResult {
  note_id: string;
}

export interface NoteProcessedEvent {
  note_id: string;
  entity_ids?: string[];
  entity_count?: number;
  relation_count?: number;
  error?: string;
}

// Selection popover (Phase 6a)

export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SelectionEvent {
  text: string;
  rect: ScreenRect;
  source_pid: number;
  source_bundle: string | null;
  source_app_name: string | null;
}

export interface SelectionStatus {
  supported: boolean;
  enabled: boolean;
  permission_granted: boolean;
}

// Remote MCP bridge (Phase 6b)

export interface McpBridgeStatus {
  enabled: boolean;
  running: boolean;
  port: number;
  local_url: string;
}

// AI provider config (Phase 6d)

export type ProviderFamily =
  | "vercel-gateway"
  | "openrouter"
  | "openai"
  | "anthropic"
  | "gemini"
  | "doubao"
  | "minimax"
  | "moonshot"
  | "glm"
  | "qwen";

export interface ProviderConfig {
  family: ProviderFamily;
  api_key: string;
  model: string;
  base_url_override?: string | null;
}

export interface ProvidersConfig {
  extract?: ProviderConfig | null;
  embed?: ProviderConfig | null;
}

export interface ProviderPresetView {
  family: ProviderFamily;
  display_name: string;
  api_base: string;
  default_extract_model: string;
  default_embed_model: string | null;
  supports_embedding: boolean;
}

export interface ProvidersView {
  config: ProvidersConfig;
  presets: ProviderPresetView[];
  restart_required: boolean;
}

export interface ProviderTestResult {
  ok: boolean;
  message: string;
}

// Import — Phase 7 V6 (user-level only per P-2)

export type ImportSourceKind =
  | "claude-md-global"
  | "claude-auto"
  | "cursor-skill"
  | "cursor-plan"
  | "codex-memory";

export type ImportDocStatus =
  | "imported"
  | "unchanged"
  | "updated"
  | "dry-run"
  | "failed"
  | "empty"
  // User marked the file "do not sync" (persistent). Never imported until
  // unskipped via the checkbox.
  | "skipped"
  // Imported before, but the user deleted the note it produced. Kept so the
  // UI can show it as "Unlinked" with a per-file re-sync action.
  | "unlinked";

export interface ImportedDocSummary {
  source_path: string;
  source_kind: ImportSourceKind;
  note_id: string;
  bytes: number;
  display_name: string | null;
  status: ImportDocStatus;
}

export interface ImportSummary {
  scanned: number;
  imported: number;
  updated: number;
  skipped_empty: number;
  skipped_dup: number;
  /** Files the user marked "do not sync" (status "skipped"). */
  skipped: number;
  /** Files whose imported note was deleted by the user (status "unlinked"). */
  unlinked: number;
  failed: number;
  docs: ImportedDocSummary[];
}

export const IMPORT_SOURCE_LABEL: Record<ImportSourceKind, string> = {
  "claude-md-global": "Claude CLAUDE.md",
  "claude-auto": "Claude auto-memory",
  "cursor-skill": "Cursor skill",
  "cursor-plan": "Cursor plan",
  "codex-memory": "Codex memory",
};

// Export — Phase 7 V6 M2 (managed block back to ~/.claude/CLAUDE.md)

export type ExportConflictReason =
  | "user-edited-block"
  | "block-removed"
  | "file-removed";

export interface ExportPreview {
  dest_path: string;
  file_exists: boolean;
  had_block: boolean;
  current_block: string | null;
  proposed_block: string;
  conflict: ExportConflictReason | null;
  entity_count: number;
}

export interface ExportResult {
  written: boolean;
  conflict: ExportConflictReason | null;
  entity_count: number;
  bytes_written: number;
  dest_path: string | null;
}

export const EXPORT_CONFLICT_LABEL: Record<ExportConflictReason, string> = {
  "user-edited-block": "You edited inside the Cairn block",
  "block-removed": "Cairn block was removed from the file",
  "file-removed": "The destination file is missing",
};

// Maintenance

export interface VacuumOrphanReport {
  vec_rows_before: number;
  orphan_vecs_deleted: number;
  orphan_meta_deleted: number;
  bytes_before: number;
  bytes_after: number;
}

export const ENTITY_COLOR_CLASS: Record<string, string> = {
  Person: "bg-entity-person/15 text-entity-person border-entity-person/30",
  Event: "bg-entity-event/15 text-entity-event border-entity-event/30",
  Preference: "bg-entity-preference/15 text-entity-preference border-entity-preference/30",
  Belief: "bg-entity-belief/15 text-entity-belief border-entity-belief/30",
  Goal: "bg-entity-goal/15 text-entity-goal border-entity-goal/30",
  Asset: "bg-entity-asset/15 text-entity-asset border-entity-asset/30",
  Skill: "bg-entity-skill/15 text-entity-skill border-entity-skill/30",
  Location: "bg-entity-location/15 text-entity-location border-entity-location/30",
};
