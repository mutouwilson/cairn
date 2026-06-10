//! Phase 7 · V6 M1 — enumerate user-level dev-tool memory files on disk.
//!
//! Per P-2 ("user-level memory only"), the scanner deliberately restricts
//! itself to locations under `$HOME`. In-repo files like `<repo>/CLAUDE.md`,
//! `<repo>/.cursor/rules/`, and `<repo>/AGENTS.md` are project-level and out
//! of scope — Cairn flattens everything into one user memory store.
//!
//! Currently covered:
//!   * `~/.claude/CLAUDE.md`             — global Claude Code memory
//!   * `~/.claude/projects/*/memory/*.md` — Claude Code auto-memory
//!
//! Adding a new source kind = one new `if path.is_file() { found.push(…) }`
//! branch and a new arm in `parsers::dispatch`.

use super::SourceKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub kind: SourceKind,
}

/// How the watcher should observe a [`WatchTarget`]. Mirrors notify's
/// `RecursiveMode` but keeps scanner free of fs-watcher deps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMode {
    /// Only direct children fire callbacks.
    Direct,
    /// Subtree fires callbacks. We avoid this on noisy roots (e.g.
    /// `~/.claude/projects/` contains hundreds of jsonl transcripts).
    Recursive,
}

#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub path: PathBuf,
    pub mode: WatchMode,
}

fn resolve_home(home: Option<&Path>) -> PathBuf {
    home.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        directories::UserDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/tmp/cairn-no-home"))
    })
}

/// Find every user-level dev-tool memory file rooted at `home`. Pass `None`
/// to use the current user's home directory.
pub fn scan_user_sources(home: Option<&Path>) -> Vec<DiscoveredFile> {
    let home = resolve_home(home);
    let mut found = Vec::new();

    // 1. ~/.claude/CLAUDE.md — global memory
    let claude_md = home.join(".claude").join("CLAUDE.md");
    if claude_md.is_file() {
        found.push(DiscoveredFile {
            path: claude_md,
            kind: SourceKind::ClaudeMdGlobal,
        });
    }

    // 2. ~/.claude/projects/<encoded>/memory/*.md — auto memory
    let auto_root = home.join(".claude").join("projects");
    if auto_root.exists() {
        for entry in walkdir::WalkDir::new(&auto_root)
            .max_depth(3)
            .into_iter()
            .filter_map(|r| r.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let in_memory_dir = entry
                .path()
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                == Some("memory");
            if !in_memory_dir {
                continue;
            }
            found.push(DiscoveredFile {
                path: entry.into_path(),
                kind: SourceKind::ClaudeAutoMemory,
            });
        }
    }

    // 3. ~/.cursor/skills-cursor/<skill>/SKILL.md — Cursor user skills.
    //    Each skill is a sub-directory; we look for an exact SKILL.md inside.
    let skills_root = home.join(".cursor").join("skills-cursor");
    if skills_root.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&skills_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let skill_md = path.join("SKILL.md");
                if skill_md.is_file() {
                    found.push(DiscoveredFile {
                        path: skill_md,
                        kind: SourceKind::CursorSkill,
                    });
                }
            }
        }
    }

    // 4. ~/.cursor/plans/*.plan.md — Cursor user-saved plans.
    let plans_root = home.join(".cursor").join("plans");
    if plans_root.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&plans_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                // Either `*.plan.md` (Cursor's current convention) or any `.md`
                // file in this dir — be permissive in case the convention drifts.
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !name.ends_with(".md") {
                    continue;
                }
                found.push(DiscoveredFile {
                    path,
                    kind: SourceKind::CursorPlan,
                });
            }
        }
    }

    // 5. ~/.codex/memories/*.md — Codex CLI user memory.
    //    Currently empty in the wild but we discover anything that lands there.
    let codex_root = home.join(".codex").join("memories");
    if codex_root.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&codex_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                if path.extension().and_then(|s| s.to_str()) != Some("md") {
                    continue;
                }
                found.push(DiscoveredFile {
                    path,
                    kind: SourceKind::CodexMemory,
                });
            }
        }
    }

    found
}

/// Paths the import watcher should subscribe to. Designed for **low event
/// noise** — Claude Code writes hundreds of jsonl transcripts under
/// `~/.claude/projects/<encoded>/` during normal use, so we avoid blanket
/// recursive watches on `projects/` and instead enumerate each `memory/`
/// subdirectory non-recursively.
///
/// Topology (today):
///   * `~/.claude`                          — Direct, catches `CLAUDE.md` writes
///   * `~/.claude/projects`                 — Direct, catches new project dirs
///   * `~/.claude/projects/<enc>/memory`    — Direct, per existing memory dir
///
/// The watcher re-calls this after every signal so a new project's
/// `memory/` directory is picked up without restart.
pub fn watch_targets(home: Option<&Path>) -> Vec<WatchTarget> {
    let home = resolve_home(home);
    let mut targets = Vec::new();

    let claude_dir = home.join(".claude");
    if claude_dir.is_dir() {
        targets.push(WatchTarget {
            path: claude_dir.clone(),
            mode: WatchMode::Direct,
        });

        let projects = claude_dir.join("projects");
        if projects.is_dir() {
            targets.push(WatchTarget {
                path: projects.clone(),
                mode: WatchMode::Direct,
            });
            if let Ok(entries) = std::fs::read_dir(&projects) {
                for entry in entries.flatten() {
                    let mem = entry.path().join("memory");
                    if mem.is_dir() {
                        targets.push(WatchTarget {
                            path: mem,
                            mode: WatchMode::Direct,
                        });
                    }
                }
            }
        }
    }

    // Cursor — skills + plans. `skills-cursor/` has subdirs we watch
    // recursively (since the SKILL.md sits one level deep). Plans are flat.
    let skills_root = home.join(".cursor").join("skills-cursor");
    if skills_root.is_dir() {
        targets.push(WatchTarget {
            path: skills_root,
            mode: WatchMode::Recursive,
        });
    }
    let plans_root = home.join(".cursor").join("plans");
    if plans_root.is_dir() {
        targets.push(WatchTarget {
            path: plans_root,
            mode: WatchMode::Direct,
        });
    }

    // Codex memories directory (empty in the wild but we subscribe anyway
    // so future writes get picked up).
    let codex_root = home.join(".codex").join("memories");
    if codex_root.is_dir() {
        targets.push(WatchTarget {
            path: codex_root,
            mode: WatchMode::Direct,
        });
    }

    targets
}

/// Predicate the watcher uses to decide whether a path needs an import
/// rerun. Pure string / component check — no syscalls, safe to invoke per
/// fs event in the debouncer callback.
pub fn is_relevant_path(path: &Path) -> bool {
    if path.extension().and_then(|s| s.to_str()) != Some("md") {
        return false;
    }
    let name = path.file_name().and_then(|s| s.to_str());
    let parent_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());

    // ~/.claude/CLAUDE.md
    if name.is_some_and(|n| n.eq_ignore_ascii_case("CLAUDE.md")) && parent_name == Some(".claude") {
        return true;
    }

    // Cursor skills: `<...>/.cursor/skills-cursor/<skill>/SKILL.md`
    if name == Some("SKILL.md") && path.components().any(|c| c.as_os_str() == "skills-cursor") {
        return true;
    }

    // Cursor plans: `<...>/.cursor/plans/<anything>.md`
    if parent_name == Some("plans") && path.components().any(|c| c.as_os_str() == ".cursor") {
        return true;
    }

    // Codex memories: `<...>/.codex/memories/<anything>.md`
    if parent_name == Some("memories") && path.components().any(|c| c.as_os_str() == ".codex") {
        return true;
    }

    // ~/.claude/projects/<encoded>/memory/<file>.md
    if parent_name != Some("memory") {
        return false;
    }
    let comps = path.components();
    let mut saw_dot_claude = false;
    let mut saw_projects = false;
    for c in comps {
        let s = c.as_os_str();
        if s == ".claude" {
            saw_dot_claude = true;
        } else if saw_dot_claude && s == "projects" {
            saw_projects = true;
            break;
        }
    }
    saw_projects
}

/// True when an event on `path` might mean a new project directory
/// appeared under `~/.claude/projects/`. The watcher uses this to know
/// when to re-call [`watch_targets`] and subscribe to any new `memory/`
/// subdir that came with it.
///
/// We can't distinguish "new dir created" from "existing dir mtime
/// updated" without event-kind info, so the watcher just runs a cheap
/// rescan on any matching event — adding a watch is a hashset miss, so
/// no-op rescans are free.
pub fn affects_watch_topology(path: &Path) -> bool {
    let parent_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    parent_name == Some("projects") && path.components().any(|c| c.as_os_str() == ".claude")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn finds_global_claude_md_and_auto_memory() {
        let td = TempDir::new().unwrap();
        let home = td.path();
        fs::create_dir_all(home.join(".claude/projects/encoded-a/memory")).unwrap();
        fs::create_dir_all(home.join(".claude/projects/encoded-b/memory")).unwrap();
        fs::write(home.join(".claude/CLAUDE.md"), "global mem").unwrap();
        fs::write(
            home.join(".claude/projects/encoded-a/memory/one.md"),
            "auto",
        )
        .unwrap();
        fs::write(
            home.join(".claude/projects/encoded-b/memory/two.md"),
            "auto",
        )
        .unwrap();

        let found = scan_user_sources(Some(home));
        assert_eq!(found.len(), 3);
        assert!(
            found
                .iter()
                .any(|f| f.kind == SourceKind::ClaudeMdGlobal
                    && f.path.ends_with(".claude/CLAUDE.md"))
        );
        let auto = found
            .iter()
            .filter(|f| f.kind == SourceKind::ClaudeAutoMemory)
            .count();
        assert_eq!(auto, 2);
    }

    #[test]
    fn skips_non_md_files_in_memory_dir() {
        let td = TempDir::new().unwrap();
        let home = td.path();
        fs::create_dir_all(home.join(".claude/projects/p/memory")).unwrap();
        fs::write(home.join(".claude/projects/p/memory/ignore.txt"), "x").unwrap();
        fs::write(home.join(".claude/projects/p/memory/keep.md"), "x").unwrap();
        let found = scan_user_sources(Some(home));
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("keep.md"));
    }

    #[test]
    fn skips_md_files_outside_memory_dir() {
        let td = TempDir::new().unwrap();
        let home = td.path();
        fs::create_dir_all(home.join(".claude/projects/p/notes")).unwrap();
        fs::write(home.join(".claude/projects/p/notes/random.md"), "x").unwrap();
        let found = scan_user_sources(Some(home));
        assert!(found.is_empty(), "files outside memory/ must be skipped");
    }

    #[test]
    fn returns_empty_when_nothing_exists() {
        let td = TempDir::new().unwrap();
        let found = scan_user_sources(Some(td.path()));
        assert!(found.is_empty());
    }

    #[test]
    fn is_relevant_path_accepts_known_layouts() {
        for p in [
            "/h/.claude/CLAUDE.md",
            "/h/.claude/projects/abc/memory/note.md",
            "/h/.claude/projects/encoded-x/memory/feedback.md",
        ] {
            assert!(is_relevant_path(Path::new(p)), "should match: {p}");
        }
    }

    #[test]
    fn is_relevant_path_rejects_unrelated() {
        for p in [
            "/h/.claude/settings.json",
            "/h/.claude/projects/abc/transcripts/2026-05-17.jsonl",
            "/h/.claude/projects/abc/memory/note.txt",
            "/h/.claude/projects/abc/notes/random.md",
            "/h/Documents/CLAUDE.md", // not under ~/.claude
        ] {
            assert!(!is_relevant_path(Path::new(p)), "should skip: {p}");
        }
    }

    #[test]
    fn watch_targets_covers_existing_memory_dirs() {
        let td = TempDir::new().unwrap();
        let home = td.path();
        fs::create_dir_all(home.join(".claude/projects/a/memory")).unwrap();
        fs::create_dir_all(home.join(".claude/projects/b/memory")).unwrap();
        // c/ exists but has no memory/ yet — must be skipped, watcher
        // will pick it up via the projects/ watch when memory/ appears.
        fs::create_dir_all(home.join(".claude/projects/c")).unwrap();

        let targets = watch_targets(Some(home));
        let paths: Vec<_> = targets.iter().map(|t| t.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with(".claude")));
        assert!(paths.iter().any(|p| p.ends_with(".claude/projects")));
        assert!(paths.iter().any(|p| p.ends_with("a/memory")));
        assert!(paths.iter().any(|p| p.ends_with("b/memory")));
        assert!(
            !paths.iter().any(|p| p.ends_with("c/memory")),
            "c has no memory/ yet, must not be watched"
        );
        for t in &targets {
            assert_eq!(
                t.mode,
                WatchMode::Direct,
                "all targets must be non-recursive to avoid transcript noise"
            );
        }
    }

    #[test]
    fn watch_targets_empty_when_no_claude_dir() {
        let td = TempDir::new().unwrap();
        assert!(watch_targets(Some(td.path())).is_empty());
    }

    #[test]
    fn affects_watch_topology_matches_new_project_dirs() {
        assert!(affects_watch_topology(Path::new(
            "/h/.claude/projects/some-new-encoded-dir"
        )));
        // Files inside memory/ are import events, not topology events.
        assert!(!affects_watch_topology(Path::new(
            "/h/.claude/projects/encoded/memory/note.md"
        )));
        assert!(!affects_watch_topology(Path::new("/h/.claude/CLAUDE.md")));
        // Without .claude in the path we don't treat it as our concern.
        assert!(!affects_watch_topology(Path::new(
            "/h/projects/random/file"
        )));
    }
}
