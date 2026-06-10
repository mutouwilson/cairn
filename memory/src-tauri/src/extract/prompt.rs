//! Prompts and JSON Schema for entity extraction.
//!
//! The schema is enforced two ways:
//!   1. We ship it to the LLM as a tool's `input_schema` (provider-side enforcement)
//!   2. We re-validate the parsed output with the `jsonschema` crate (defence in depth)

use once_cell::sync::Lazy;
use serde_json::{json, Value};

pub const SYSTEM: &str = "You extract structured personal-life entities from notes written by the \
owner of this memory system. Your output is consumed by AI agents (Claude, ChatGPT, Cursor, ...) \
that need to understand the owner's life context.\n\n\
You MUST call the `save_extraction` tool exactly once with all entities and relations you can find.\n\n\
Entity types and typical properties:\n\
  - Person:     { relationship, role, contact, notes }\n\
  - Event:      { date, location, participants, sentiment, summary }\n\
  - Preference: { domain, value, strength, context }\n\
  - Belief:     { topic, position, confidence_level }\n\
  - Goal:       { description, progress, deadline, priority }\n\
  - Asset:      { kind, value, location, acquired_at }\n\
  - Skill:      { domain, proficiency, interest }\n\
  - Location:   { kind, significance, frequency }\n\n\
Rules:\n\
1. Only extract what is EXPLICIT or strongly implied. Do not speculate.\n\
2. `confidence` must honestly reflect your uncertainty (0.0 to 1.0). Below 0.6 = do not output.\n\
3. Names are canonical: keep cultural form (e.g. \"妈\" not \"mom\").\n\
4. `properties` only includes keys for which you have textual evidence.\n\
5. `relations` must reference names that appear in your `entities` list.\n\
6. Empty arrays are valid for notes with nothing extractable.\n\
7. Output JSON only via the tool — never plain text.";

pub const FEW_SHOT_USER_1: &str = "刚和妈打完电话,她说膝盖又疼了,但不愿去医院";
pub static FEW_SHOT_TOOL_1: Lazy<Value> = Lazy::new(|| {
    json!({
        "entities": [
            {
                "type": "Person",
                "name": "妈",
                "properties": { "relationship": "mother" },
                "confidence": 0.99
            },
            {
                "type": "Event",
                "name": "妈的膝盖疼",
                "properties": {
                    "date": null,
                    "summary": "妈又一次提到膝盖疼,拒绝就医",
                    "participants": ["妈"],
                    "sentiment": "concern"
                },
                "confidence": 0.92
            },
            {
                "type": "Belief",
                "name": "妈不愿就医",
                "properties": {
                    "topic": "妈对医疗的态度",
                    "position": "回避就医"
                },
                "confidence": 0.78
            }
        ],
        "relations": [
            { "from": "妈", "to": "妈的膝盖疼", "type": "experienced" },
            { "from": "妈", "to": "妈不愿就医", "type": "holds_belief" }
        ]
    })
});

pub const FEW_SHOT_USER_2: &str = "我喝拿铁不要太热,糖少";
pub static FEW_SHOT_TOOL_2: Lazy<Value> = Lazy::new(|| {
    json!({
        "entities": [
            {
                "type": "Preference",
                "name": "拿铁温度",
                "properties": {
                    "domain": "coffee",
                    "value": "not too hot",
                    "strength": 0.8
                },
                "confidence": 0.95
            },
            {
                "type": "Preference",
                "name": "拿铁糖量",
                "properties": {
                    "domain": "coffee",
                    "value": "low sugar",
                    "strength": 0.85
                },
                "confidence": 0.95
            }
        ],
        "relations": []
    })
});

/// JSON Schema enforced both by the LLM tool and by our validator.
pub static EXTRACTION_SCHEMA: Lazy<Value> = Lazy::new(|| {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "entities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["type", "name", "confidence"],
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": [
                                "Person", "Event", "Preference", "Belief",
                                "Goal", "Asset", "Skill", "Location"
                            ]
                        },
                        "name": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "properties": {
                            "type": "object",
                            "additionalProperties": true
                        },
                        "confidence": {
                            "type": "number",
                            "minimum": 0.0,
                            "maximum": 1.0
                        }
                    }
                }
            },
            "relations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["from", "to", "type"],
                    "properties": {
                        "from": { "type": "string", "minLength": 1 },
                        "to":   { "type": "string", "minLength": 1 },
                        "type": { "type": "string", "minLength": 1 }
                    }
                }
            }
        },
        "required": ["entities", "relations"]
    })
});
