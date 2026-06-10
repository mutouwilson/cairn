//! Parser for `~/.claude/CLAUDE.md` — Claude Code's global user-level
//! memory file. The format is plain markdown; the user typically writes
//! durable facts about themselves ("I'm based in Beijing", "Use `pnpm` not
//! `npm`", etc.) and Claude Code never auto-edits this file.
//!
//! No frontmatter is expected in the wild (Mar–May 2026), but we still tolerate
//! one — same convention as `claude_auto.rs`. The body comes back whitespace-
//! normalised so a trailing blank line doesn't show up as a Cairn note diff.

use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub struct ParsedClaudeMd {
    pub frontmatter_title: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize, Default)]
struct Frontmatter {
    #[serde(default)]
    title: Option<String>,
}

pub fn parse(raw: &str) -> ParsedClaudeMd {
    let (fm, body) = split_frontmatter(raw);
    let mut out = ParsedClaudeMd {
        body: body.trim().to_string(),
        ..Default::default()
    };
    if let Some(yaml) = fm {
        if let Ok(parsed) = serde_yaml::from_str::<Frontmatter>(yaml) {
            out.frontmatter_title = parsed.title;
        }
    }
    out
}

fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    let s = raw.trim_start_matches('\u{FEFF}');
    let s = s.trim_start_matches(['\r', '\n']);
    if !s.starts_with("---") {
        return (None, raw);
    }
    let after_first = &s[3..];
    let after_first = after_first.trim_start_matches(['\r', '\n']);
    let close = after_first.find("\n---");
    let Some(idx) = close else {
        return (None, raw);
    };
    let yaml = &after_first[..idx];
    let rest = &after_first[idx + 4..];
    let rest = rest.trim_start_matches(['\r', '\n']);
    (Some(yaml), rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_markdown() {
        let raw = "I prefer pnpm over npm.\n\nUse pinyin for Chinese names.\n";
        let p = parse(raw);
        assert!(p.frontmatter_title.is_none());
        assert!(p.body.contains("pnpm"));
        assert!(p.body.contains("pinyin"));
    }

    #[test]
    fn handles_empty_file() {
        let p = parse("");
        assert_eq!(p.body, "");
    }

    #[test]
    fn parses_optional_frontmatter() {
        let raw = "---\ntitle: About me\n---\nI'm a developer.\n";
        let p = parse(raw);
        assert_eq!(p.frontmatter_title.as_deref(), Some("About me"));
        assert_eq!(p.body, "I'm a developer.");
    }
}
