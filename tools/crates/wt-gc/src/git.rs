//! Thin wrappers over the `git` CLI: discover main repos, enumerate their linked
//! worktrees, and gather the facts the GC keys on (staleness, cleanliness, push
//! state). Everything shells out to `git` — no git library, no extra deps.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::model::PushState;

/// A linked worktree as reported by `git worktree list --porcelain`.
pub struct LinkedWorktree {
    pub path: PathBuf,
    pub branch: String,
}

/// Run `git -C <dir> <args...>` and return trimmed stdout, erroring on failure.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        anyhow::bail!("git {} failed in {}", args.join(" "), dir.display());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether `git -C <dir> <args...>` exits zero (for existence probes).
fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Find main repositories: immediate children of each root that contain a `.git`
/// **directory** (linked worktrees have a `.git` *file*, so they're excluded).
/// Deduplicated and sorted.
pub fn find_main_repos(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut repos = BTreeSet::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join(".git").is_dir() {
                repos.insert(std::fs::canonicalize(&path).unwrap_or(path));
            }
        }
    }
    repos.into_iter().collect()
}

/// List `repo`'s linked worktrees (excluding the main working tree, locked ones,
/// detached HEADs, and any whose directory has vanished).
pub fn linked_worktrees(repo: &Path) -> Result<Vec<LinkedWorktree>> {
    let text = git(repo, &["worktree", "list", "--porcelain"])?;
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    let mut locked = false;
    // Records are separated by blank lines; flush on the blank line.
    let mut flush = |path: &mut Option<PathBuf>, branch: &mut Option<String>, locked: &mut bool| {
        if let (Some(p), Some(b)) = (path.take(), branch.take()) {
            if !*locked && p != repo && p.exists() {
                out.push(LinkedWorktree { path: p, branch: b });
            }
        }
        *locked = false;
    };
    for line in text.lines() {
        if line.is_empty() {
            flush(&mut path, &mut branch, &mut locked);
        } else if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_string());
        } else if line == "locked" || line.starts_with("locked ") {
            locked = true;
        }
    }
    flush(&mut path, &mut branch, &mut locked);
    Ok(out)
}

/// Committer time of `wt`'s HEAD, in unix seconds.
pub fn head_commit_time(wt: &Path) -> Result<u64> {
    git(wt, &["log", "-1", "--format=%ct", "HEAD"])?
        .parse()
        .context("parsing commit timestamp")
}

/// Whether `wt` has no uncommitted changes (tracked or untracked).
pub fn is_clean(wt: &Path) -> Result<bool> {
    Ok(git(wt, &["status", "--porcelain"])?.is_empty())
}

/// How `wt`'s HEAD relates to its upstream.
pub fn push_state(wt: &Path) -> PushState {
    if !git_ok(wt, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]) {
        return PushState::NoUpstream;
    }
    match git(wt, &["rev-list", "--count", "@{u}..HEAD"]).ok().and_then(|s| s.parse::<u64>().ok()) {
        Some(0) => PushState::Pushed,
        Some(n) => PushState::Ahead(n),
        None => PushState::NoUpstream,
    }
}

/// Drop registrations for worktrees whose directories no longer exist.
pub fn prune(repo: &Path) -> Result<()> {
    git(repo, &["worktree", "prune"]).map(|_| ())
}

/// Whether `path` is older than `days` days (based on a unix-seconds timestamp).
pub fn older_than(ts: u64, days: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(ts) >= days.saturating_mul(86_400)
}
