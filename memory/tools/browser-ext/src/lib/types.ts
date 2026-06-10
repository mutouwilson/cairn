// Mirrors src-tauri/src/api.rs JSON shapes.

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
  properties: string;
  confidence: number;
  source_note_id: string | null;
  created_at: number;
  updated_at: number;
  strength: number;
  last_accessed: number;
  access_count: number;
  base_importance: number;
}

export interface RetrievedEntity {
  entity: Entity;
  /** Best-effort retained for adapters older than the fused-score split. */
  score?: number;
  bm25_score: number;
  /** Cosine similarity in [0, 1]. NaN if no vector hit. */
  vector_score: number;
  /** Reciprocal-rank-fused score across BM25 + vector. The quality gate
   *  (STRONG_SCORE / WEAK_SCORE) is evaluated against this field. */
  rrf_score: number;
  importance_score: number;
  recency_score: number;
  final_score: number;
}

export interface RetrievedNote {
  note: Note;
  score?: number;
  bm25_score: number;
  recency_score: number;
  final_score: number;
  /** Notes don't go through vector/RRF today, but the passive indicator
   *  evaluates a `rrf_score`-shaped field uniformly, so the picker falls
   *  back to `final_score` when this is missing. */
  rrf_score?: number;
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

export interface CaptureResponse {
  note_id: string;
  source: string;
}

export interface CairnStatus {
  ok: boolean;
  server: string;
  version: string;
}

export interface CaptureMetadata {
  url?: string;
  title?: string;
  agent?: string;
  conversation_id?: string;
  message_id?: string;
  [key: string]: string | undefined;
}
