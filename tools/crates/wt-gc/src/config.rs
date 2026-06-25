//! `wt-gc`'s configuration — loaded via the shared XDG-aware resolver
//! (`--config` → `$WT_GC_CONFIG` → `~/.config/wt-gc/config.toml` → defaults).

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Directories whose immediate children are scanned for git repos. A child
    /// is a "main repo" when it contains a `.git` directory. `~/` is expanded.
    pub repo_roots: Vec<String>,
    /// Where reclaimed worktrees are moved (a grace area before deletion).
    pub trash_dir: String,
    /// A worktree is stale after this many days without a commit.
    pub stale_days: u64,
    /// Trashed worktrees are deleted for good after this many days.
    pub trash_retention_days: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repo_roots: vec!["~/dev".into(), "~/dev/repos".into()],
            trash_dir: "~/dev/.worktree-trash".into(),
            stale_days: 30,
            trash_retention_days: 7,
        }
    }
}

impl Config {
    /// The configured repo roots, with `~/` expanded.
    pub fn roots(&self) -> Vec<PathBuf> {
        self.repo_roots.iter().map(|r| expand_tilde(r)).collect()
    }

    /// The trash directory, with `~/` expanded.
    pub fn trash_path(&self) -> PathBuf {
        expand_tilde(&self.trash_dir)
    }
}

/// Expand a leading `~/` to `$HOME`; leave everything else untouched.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}
