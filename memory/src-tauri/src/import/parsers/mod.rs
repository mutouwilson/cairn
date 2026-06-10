//! Per-source markdown parsers. Each parser converts a file on disk into
//! a normalised structure (frontmatter + body) that the import pipeline
//! feeds to the extraction LLM.
//!
//! Per P-2 we only ship parsers for user-level locations:
//!   * `claude_auto`  — `~/.claude/projects/*/memory/*.md`
//!   * `claude_md`    — `~/.claude/CLAUDE.md`

pub mod claude_auto;
pub mod claude_md;
