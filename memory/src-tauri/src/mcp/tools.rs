//! MCP tool definitions and implementations.

use crate::audit::AuditLogger;
use crate::db::Db;
use crate::retrieval::Retriever;
use crate::schema::EntityType;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub fn list_tools() -> Value {
    json!({
        "tools": [
            {
                "name": "search_memory",
                "description": "Search the owner's personal memory for anything they've previously captured — people they know, preferences they've stated, beliefs they hold, goals they're working on, places they care about, projects they're running, and freeform notes. Returns entities + notes ranked by relevance and how often the owner refers to them.\n\nCall this FIRST whenever a user message hints that the answer depends on the owner's life, preferences, or relationships, before answering from general knowledge.\n\nExample triggers (user messages that should fire this tool):\n  • \"What did I say about <topic>?\"\n  • \"Who do I know at <company>?\"\n  • \"What's my preference for <thing>?\"\n  • \"Remind me about my <project / goal / event>.\"\n  • \"Have I met <name>?\"\n  • Any pronoun referring to the owner (\"my\", \"I\", \"我的\") combined with a noun.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Natural-language query. Whitespace-separated tokens are OR'd."
                        },
                        "types": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["Person","Event","Preference","Belief","Goal","Asset","Skill","Location"]
                            },
                            "description": "Optional filter limiting result entity types."
                        },
                        "limit": {
                            "type": "integer",
                            "default": 10,
                            "minimum": 1,
                            "maximum": 50
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_preferences",
                "description": "List preferences the owner has explicitly stated (food choices, coffee order, communication style, work setup, tools they like, etc.). Optionally narrow by domain.\n\nUse this when the user's question is specifically and only about preferences. For broader questions where preferences are one of several relevant things, prefer `search_memory` — it returns fuller context.\n\nExample triggers:\n  • \"What kind of <X> do I like?\"\n  • \"What's my preferred <Y>?\"\n  • \"How do I usually <Z>?\"\n  • \"List my <domain> preferences.\"",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "domain": {
                            "type": "string",
                            "description": "Optional preference domain to filter by (e.g. 'food', 'coffee', 'work')."
                        }
                    }
                }
            },
            {
                "name": "list_recent_notes",
                "description": "Return the most recent freeform notes the owner captured (newest first). Each note is the raw text they wrote, not the structured entities extracted from it.\n\nUse this when the user asks about *recent* activity (\"what was I working on yesterday\", \"what did I capture this week\"), or as a fallback when `search_memory` returns nothing relevant and you want to scan raw recent context.\n\nExample triggers:\n  • \"What was I just thinking about?\"\n  • \"What did I capture today / this week?\"\n  • \"Show me my latest notes.\"\n  • \"What's on my mind lately?\"",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {
                            "type": "integer",
                            "default": 10,
                            "minimum": 1,
                            "maximum": 100
                        }
                    }
                }
            },
            {
                "name": "record_observation",
                "description": "Save a new fact or observation about the owner into their memory. Cairn will auto-structure it into typed entities (people, preferences, goals, beliefs, etc.) so future conversations and other agents can recall it.\n\nCall this whenever the owner states something about themselves that you don't already have in memory — a new preference, a new project, a person they've met, a fact about their setup, a belief, a goal they're forming. Capturing these as they come up grows the memory and makes future answers sharper.\n\nExample triggers (after the owner says something worth remembering):\n  • User: \"I'm allergic to peanuts.\" → record: \"Owner is allergic to peanuts.\"\n  • User: \"I'm starting a side project called X.\" → record: \"Owner is starting side project X.\"\n  • User: \"I prefer mornings for meetings.\" → record: \"Owner prefers meetings in the morning.\"\n  • User: \"I met Alice at the conference.\" → record: \"Owner met Alice at <conference name>.\"\n\nWrite-scoped: requires the agent to have write permission on the relevant entity type.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The observation text. Should be phrased as a note the owner could have written (first-person or about-the-owner)."
                        }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "get_themes",
                "description": "Return summary clusters of the owner's recurring topics — people they reference often, preference domains they care about, ongoing goals. Each theme has a title + summary covering what's been going on with that topic across many notes.\n\nPrefer this over `search_memory` when the user asks about *patterns* or \"what's been happening with X\" rather than a specific fact. Specific facts → search_memory. Recent vibes / aggregated context → get_themes.\n\nExample triggers:\n  • \"What has <person> been up to in my notes?\"\n  • \"What have I been thinking about <topic> lately?\"\n  • \"What are the ongoing themes in my work?\"\n  • \"What's the latest on <project>?\"\n  • \"Summarize what I've been focused on.\"",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {
                            "type": "integer",
                            "default": 20,
                            "minimum": 1,
                            "maximum": 100
                        },
                        "topic_prefix": {
                            "type": "string",
                            "description": "Optional filter, e.g. 'person:' or 'domain:coffee'"
                        }
                    }
                }
            }
        ]
    })
}

pub struct ToolCallOutcome {
    pub result: Value,
    pub result_count: Option<i64>,
    pub touched_types: Vec<String>,
    pub wrote: bool,
}

pub async fn call_tool(
    db: &Db,
    retriever: &Retriever,
    audit: &AuditLogger,
    agent_id: &str,
    name: &str,
    arguments: &Value,
) -> Result<ToolCallOutcome> {
    let _ = audit; // audit log is appended by the caller after permission check
    match name {
        "search_memory" => search_memory(db, retriever, agent_id, arguments).await,
        "get_preferences" => get_preferences(db, agent_id, arguments).await,
        "list_recent_notes" => list_recent_notes(db, agent_id, arguments).await,
        "record_observation" => record_observation(db, agent_id, arguments).await,
        "get_themes" => get_themes(db, agent_id, arguments).await,
        other => Err(anyhow!("unknown tool: {}", other)),
    }
}

async fn get_themes(db: &Db, agent_id: &str, args: &Value) -> Result<ToolCallOutcome> {
    // Themes are derived from all entity types, so we require '*' read OR
    // require the agent to have read on at least Person/Belief/Goal/Preference.
    // Simplest: gate on the same '*' rule used for raw notes.
    let allowed = db.has_read(agent_id, "*").await?
        || (db.has_read(agent_id, "Person").await? && db.has_read(agent_id, "Goal").await?);
    if !allowed {
        return Ok(ToolCallOutcome {
            result: serde_json::json!({
                "themes": [], "denied": true,
                "reason": "needs '*' read scope or read on Person + Goal"
            }),
            result_count: Some(0),
            touched_types: vec![],
            wrote: false,
        });
    }
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);
    let prefix = args.get("topic_prefix").and_then(|v| v.as_str());

    let all = db.list_active_themes(limit * 2).await?;
    let filtered: Vec<_> = all
        .into_iter()
        .filter(|t| match prefix {
            Some(p) => t.topic.starts_with(p),
            None => true,
        })
        .take(limit as usize)
        .collect();

    for t in &filtered {
        let _ = db.touch_theme(&t.id).await;
    }

    let count = filtered.len() as i64;
    let themes_json: Vec<Value> = filtered
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "topic": t.topic,
                "title": t.title,
                "summary": t.summary,
                "confidence": t.confidence,
                "evidence_ids": serde_json::from_str::<Vec<String>>(&t.evidence).unwrap_or_default(),
                "last_consolidated_at": t.last_consolidated_at,
                "access_count": t.access_count
            })
        })
        .collect();

    Ok(ToolCallOutcome {
        result: serde_json::json!({ "themes": themes_json }),
        result_count: Some(count),
        touched_types: vec!["theme".into()],
        wrote: false,
    })
}

async fn search_memory(
    db: &Db,
    retriever: &Retriever,
    agent_id: &str,
    args: &Value,
) -> Result<ToolCallOutcome> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing query"))?;
    let types: Option<Vec<String>> = args.get("types").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    });
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);

    // Permission check: filter requested types down to those the agent can read.
    let allowed_types: Vec<String> = match &types {
        Some(ts) => {
            let mut out = Vec::new();
            for t in ts {
                if db.has_read(agent_id, t).await? {
                    out.push(t.clone());
                }
            }
            out
        }
        None => {
            // No filter requested: enumerate types the agent can read.
            let mut out = Vec::new();
            for t in EntityType::ALL {
                if db.has_read(agent_id, t.as_str()).await? {
                    out.push(t.as_str().to_string());
                }
            }
            out
        }
    };

    if allowed_types.is_empty() {
        return Ok(ToolCallOutcome {
            result: json!({
                "entities": [],
                "notes": [],
                "denied": true,
                "reason": "no readable entity types for this agent"
            }),
            result_count: Some(0),
            touched_types: vec![],
            wrote: false,
        });
    }

    let res = retriever
        .search(query, Some(allowed_types.clone()), limit, limit)
        .await?;
    let count = (res.entities.len() + res.notes.len()) as i64;

    // Reinforce touched entities.
    for re in &res.entities {
        let _ = db.touch_entity(&re.entity.id).await;
    }

    let formatted = format_search_result(&res);
    Ok(ToolCallOutcome {
        result: formatted,
        result_count: Some(count),
        touched_types: allowed_types,
        wrote: false,
    })
}

async fn get_preferences(db: &Db, agent_id: &str, args: &Value) -> Result<ToolCallOutcome> {
    if !db.has_read(agent_id, "Preference").await? {
        return Ok(ToolCallOutcome {
            result: json!({ "preferences": [], "denied": true }),
            result_count: Some(0),
            touched_types: vec![],
            wrote: false,
        });
    }
    let domain = args.get("domain").and_then(|v| v.as_str());
    let entities = db.list_entities(Some("Preference"), 100).await?;
    let filtered: Vec<Value> = entities
        .into_iter()
        .filter(|e| match domain {
            None => true,
            Some(d) => {
                let p = e.properties_value();
                p.get("domain")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case(d))
                    .unwrap_or(false)
            }
        })
        .map(|e| {
            json!({
                "name": e.name,
                "properties": e.properties_value(),
                "confidence": e.confidence,
                "last_updated": e.updated_at
            })
        })
        .collect();
    let count = filtered.len() as i64;
    Ok(ToolCallOutcome {
        result: json!({ "preferences": filtered }),
        result_count: Some(count),
        touched_types: vec!["Preference".into()],
        wrote: false,
    })
}

async fn list_recent_notes(db: &Db, agent_id: &str, args: &Value) -> Result<ToolCallOutcome> {
    // Reading raw notes requires read on '*' (raw notes can contain anything).
    if !db.has_read(agent_id, "*").await? {
        return Ok(ToolCallOutcome {
            result: json!({ "notes": [], "denied": true, "reason": "raw notes require '*' read scope" }),
            result_count: Some(0),
            touched_types: vec![],
            wrote: false,
        });
    }
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);
    let notes = db.recent_notes(limit).await?;
    let serialised: Vec<Value> = notes
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "text": n.text,
                "source": n.source,
                "created_at": n.created_at
            })
        })
        .collect();
    let count = serialised.len() as i64;
    Ok(ToolCallOutcome {
        result: json!({ "notes": serialised }),
        result_count: Some(count),
        touched_types: vec!["*".into()],
        wrote: false,
    })
}

async fn record_observation(db: &Db, agent_id: &str, args: &Value) -> Result<ToolCallOutcome> {
    // Write permission required.
    let exists: Option<(String,)> = sqlx::query_as(
        "SELECT scope FROM permissions WHERE agent_id = ? AND (entity_type = '*' OR entity_type = 'note') AND scope = 'write' LIMIT 1",
    )
    .bind(agent_id)
    .fetch_optional(db.pool())
    .await?;
    if exists.is_none() {
        return Ok(ToolCallOutcome {
            result: json!({ "ok": false, "denied": true, "reason": "write permission required" }),
            result_count: Some(0),
            touched_types: vec![],
            wrote: false,
        });
    }
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing text"))?;
    let id = db.insert_note(text, &format!("agent:{}", agent_id)).await?;
    Ok(ToolCallOutcome {
        result: json!({ "ok": true, "note_id": id, "status": "queued_for_extraction" }),
        result_count: Some(1),
        touched_types: vec![],
        wrote: true,
    })
}

fn format_search_result(res: &crate::retrieval::RetrievalResult) -> Value {
    json!({
        "entities": res.entities.iter().map(|re| json!({
            "id": re.entity.id,
            "type": re.entity.r#type,
            "name": re.entity.name,
            "properties": re.entity.properties_value(),
            "confidence": re.entity.confidence,
            "score": re.final_score,
            "_diag": {
                "bm25": re.bm25_score,
                "vector_sim": re.vector_score,
                "rrf": re.rrf_score,
                "importance": re.importance_score,
                "recency": re.recency_score
            }
        })).collect::<Vec<_>>(),
        "notes": res.notes.iter().map(|rn| json!({
            "id": rn.note.id,
            "text": rn.note.text,
            "source": rn.note.source,
            "created_at": rn.note.created_at,
            "score": rn.final_score
        })).collect::<Vec<_>>(),
        "diagnostics": {
            "bm25_hits": res.diagnostics.bm25_hits,
            "vector_hits": res.diagnostics.vector_hits,
            "fused_hits": res.diagnostics.fused_hits,
            "vector_skipped": res.diagnostics.vector_path_skipped_reason
        }
    })
}
