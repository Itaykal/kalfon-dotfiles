//! `feature`'s configuration — mirrors the Go tool's TOML, loaded via the
//! shared XDG-aware resolver (`--config` → `$FEATURE_CONFIG` →
//! `~/.config/feature/config.toml` → defaults).

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Account whose issues are listed / who new issues are assigned to.
    /// Empty means "the current user" (resolved via `jira me`).
    pub assignee: String,
    /// The jira CLI executable name.
    pub jira_bin: String,
    pub list: ListConfig,
    pub create: CreateConfig,
    /// Query-prefix → issue type (e.g. "b" → "Bug").
    pub aliases: BTreeMap<String, String>,
    /// How long the cached issue list stays fresh, in seconds (0 disables).
    /// Short by default — Jira issues change often, and `ctrl-r` force-refreshes.
    pub cache_ttl_secs: u64,
    /// Directory new worktrees are created under (ctrl-enter). `~` is expanded.
    /// Empty = a sibling of the repo: `<repo>/../<repo-name>-worktrees`.
    pub worktree_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ListConfig {
    pub exclude_statuses: Vec<String>,
    pub exclude_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CreateConfig {
    /// Status to transition a freshly created issue to (best-effort).
    pub move_to: String,
    /// Extra fields applied on create, e.g. {"squad": "Detection"}.
    pub custom: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            assignee: String::new(),
            jira_bin: "jira".into(),
            list: ListConfig::default(),
            create: CreateConfig::default(),
            aliases: default_aliases(),
            cache_ttl_secs: 60,
            worktree_dir: String::new(),
        }
    }
}

impl Default for ListConfig {
    fn default() -> Self {
        Self {
            exclude_statuses: vec!["Done".into(), "Archived".into()],
            exclude_types: vec!["Epic".into()],
        }
    }
}

impl Default for CreateConfig {
    fn default() -> Self {
        Self {
            move_to: "In Progress".into(),
            custom: BTreeMap::new(),
        }
    }
}

fn default_aliases() -> BTreeMap<String, String> {
    [
        ("b", "Bug"),
        ("bug", "Bug"),
        ("t", "Task"),
        ("task", "Task"),
        ("s", "Story"),
        ("story", "Story"),
        ("st", "Sub-task"),
        ("sub", "Sub-task"),
        ("subtask", "Sub-task"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}
