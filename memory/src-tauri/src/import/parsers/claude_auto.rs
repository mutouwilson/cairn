//! Claude Code "auto memory" file parser — `~/.claude/projects/<encoded-path>/memory/*.md`.
//!
//! Format (observed in the wild, March-May 2026):
//!
//! ```markdown
//! ---
//! name: <short slug>
//! description: <one-line summary>
//! type: project | user | feedback | reference | …
//! originSessionId: <uuid>
//! metadata:                  # optional, free-form
//!   ...
//! ---
//! <markdown body — usually starts with a paragraph then "Why:" / "How to apply:" sections>
//! ```
//!
//! We don't try to be clever about the body: keep it verbatim and let the
//! Cairn extractor turn it into entities downstream. The frontmatter
//! `name` / `description` carry the most reliable signal so we surface
//! them to the import summary + prepend them in the note text.

use serde::Deserialize;

/// Parsed representation of one auto-memory file.
#[derive(Debug, Clone, Default)]
pub struct ParsedClaudeAuto {
    pub frontmatter_name: Option<String>,
    pub frontmatter_description: Option<String>,
    pub frontmatter_type: Option<String>,
    pub origin_session_id: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize, Default)]
struct Frontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "type")]
    ty: Option<String>,
    #[serde(default, rename = "originSessionId")]
    origin_session_id: Option<String>,
}

/// Best-effort parse: never errors. If the file has no frontmatter, the
/// whole content is the body and the frontmatter fields stay `None`.
pub fn parse(raw: &str) -> ParsedClaudeAuto {
    let (frontmatter, body) = split_frontmatter(raw);
    let mut out = ParsedClaudeAuto {
        body: body.to_string(),
        ..Default::default()
    };
    if let Some(yaml) = frontmatter {
        if let Ok(fm) = serde_yaml::from_str::<Frontmatter>(yaml) {
            out.frontmatter_name = fm.name;
            out.frontmatter_description = fm.description;
            out.frontmatter_type = fm.ty;
            out.origin_session_id = fm.origin_session_id;
        }
    }
    out
}

/// Split `---\n…\n---\n…` into (frontmatter_yaml, rest). Tolerates CRLF and
/// missing trailing newline before the closing `---`. Returns `(None,
/// whole)` when no opening `---` is present.
fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    let s = raw.trim_start_matches('\u{FEFF}'); // strip BOM
    let s = s.trim_start_matches(['\r', '\n']);
    if !s.starts_with("---") {
        return (None, raw);
    }
    // Look for the closing `---` on its own line.
    let after_first = &s[3..];
    let after_first = after_first.trim_start_matches(['\r', '\n']);
    let close = after_first.find("\n---");
    let Some(idx) = close else {
        return (None, raw);
    };
    let yaml = &after_first[..idx];
    let rest = &after_first[idx + 4..]; // skip "\n---"
    let rest = rest.trim_start_matches(['\r', '\n']);
    (Some(yaml), rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_frontmatter() {
        let raw = "---\nname: Test Memory\ndescription: A short summary\ntype: project\noriginSessionId: abc-123\n---\nBody text here.\n";
        let p = parse(raw);
        assert_eq!(p.frontmatter_name.as_deref(), Some("Test Memory"));
        assert_eq!(
            p.frontmatter_description.as_deref(),
            Some("A short summary")
        );
        assert_eq!(p.frontmatter_type.as_deref(), Some("project"));
        assert_eq!(p.origin_session_id.as_deref(), Some("abc-123"));
        assert_eq!(p.body.trim(), "Body text here.");
    }

    #[test]
    fn handles_no_frontmatter() {
        let raw = "Just a plain note without YAML.";
        let p = parse(raw);
        assert!(p.frontmatter_name.is_none());
        assert_eq!(p.body, raw);
    }

    #[test]
    fn handles_malformed_frontmatter_gracefully() {
        // Unclosed frontmatter — should fall through as plain body.
        let raw = "---\nname: missing closing\nBody";
        let p = parse(raw);
        assert!(p.frontmatter_name.is_none());
        assert_eq!(p.body, raw);
    }

    #[test]
    fn strips_bom() {
        let raw = "\u{FEFF}---\nname: x\n---\nbody";
        let p = parse(raw);
        assert_eq!(p.frontmatter_name.as_deref(), Some("x"));
        assert_eq!(p.body.trim(), "body");
    }
}
