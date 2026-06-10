//! JSON Schema validation for LLM output.

use crate::extract::prompt::EXTRACTION_SCHEMA;
use anyhow::{anyhow, Result};
use jsonschema::Validator;
use once_cell::sync::Lazy;
use serde_json::Value;

static VALIDATOR: Lazy<Validator> = Lazy::new(|| {
    jsonschema::draft202012::new(&EXTRACTION_SCHEMA).expect("extraction schema is well-formed")
});

/// Validate a JSON value against the extraction schema.
/// Returns Err with all collected error paths on failure.
pub fn validate(value: &Value) -> Result<()> {
    let errors: Vec<String> = VALIDATOR
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("schema validation failed: {}", errors.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_result_valid() {
        validate(&json!({ "entities": [], "relations": [] })).unwrap();
    }

    #[test]
    fn missing_confidence_rejected() {
        let r = validate(&json!({
            "entities": [{ "type": "Person", "name": "x" }],
            "relations": []
        }));
        assert!(r.is_err());
    }

    #[test]
    fn unknown_entity_type_rejected() {
        let r = validate(&json!({
            "entities": [{
                "type": "Vehicle",
                "name": "car",
                "confidence": 0.9
            }],
            "relations": []
        }));
        assert!(r.is_err());
    }
}
