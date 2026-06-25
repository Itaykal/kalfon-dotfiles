//! The git side: turn an issue into a branch name and create it. Ported from
//! the Go `vcs.go`, deliberately tracker-agnostic.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Lowercase `summary` and collapse any run of non-alphanumeric characters into
/// a single hyphen, trimming leading/trailing hyphens.
pub fn slug(summary: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in summary.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            pending_dash = false;
        } else if !pending_dash {
            out.push('-');
            pending_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Build `KEY-slug(summary)` (just `KEY` when the slug is empty).
pub fn branch(key: &str, summary: &str) -> String {
    let slug = slug(summary);
    if slug.is_empty() {
        key.to_string()
    } else {
        format!("{key}-{slug}")
    }
}

/// Switch to `branch`, creating it if it doesn't exist. Re-running `feature` on
/// the same issue just checks out the branch you already made, rather than
/// failing the way a bare `git checkout -b` would. Inherits the terminal so
/// git's output shows.
pub fn checkout(branch: &str) -> Result<()> {
    let args: Vec<&str> = if branch_exists(branch)? {
        vec!["checkout", branch]
    } else {
        vec!["checkout", "-b", branch]
    };
    let status = Command::new("git")
        .args(&args)
        .status()
        .context("running git checkout")?;
    if !status.success() {
        bail!("git checkout {branch} failed");
    }
    Ok(())
}

/// Spin `branch` out into its own git worktree and return the worktree path.
/// Like [`checkout`], this is idempotent: re-running on the same issue returns
/// the existing worktree rather than failing. Inherits the terminal so git's
/// output shows. Creates the branch if it doesn't exist yet.
pub fn worktree_add(branch: &str, base_dir: Option<&str>) -> Result<PathBuf> {
    if let Some(existing) = worktree_for_branch(branch)? {
        return Ok(existing);
    }
    let base = worktree_base(base_dir)?;
    std::fs::create_dir_all(&base)
        .with_context(|| format!("creating worktree base dir {}", base.display()))?;
    let path = base.join(branch);
    let path_str = path.to_string_lossy().to_string();
    let args: Vec<&str> = if branch_exists(branch)? {
        vec!["worktree", "add", &path_str, branch]
    } else {
        vec!["worktree", "add", "-b", branch, &path_str]
    };
    let status = Command::new("git")
        .args(&args)
        .status()
        .context("running git worktree add")?;
    if !status.success() {
        bail!("git worktree add for {branch} failed");
    }
    Ok(path)
}

/// The directory worktrees are created under. A non-empty `base_dir` (from
/// config) is treated as a root and grouped by repo — `<base_dir>/<repo-name>`,
/// with a leading `~/` expanded — so a shared root like `~/dev/worktrees` keeps
/// each repo's worktrees together. Empty falls back to a sibling of the repo:
/// `<repo>/../<repo-name>-worktrees`.
fn worktree_base(base_dir: Option<&str>) -> Result<PathBuf> {
    Ok(worktree_base_for(base_dir, &repo_toplevel()?))
}

/// Pure path composition for [`worktree_base`] given the repo `top`level.
fn worktree_base_for(base_dir: Option<&str>, top: &Path) -> PathBuf {
    let name = top
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".into());
    if let Some(dir) = base_dir.filter(|d| !d.is_empty()) {
        return expand_tilde(dir).join(name);
    }
    let parent = top
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| top.to_path_buf());
    parent.join(format!("{name}-worktrees"))
}

/// Expand a leading `~/` to `$HOME`; leave everything else untouched.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// The repository root (`git rev-parse --show-toplevel`).
fn repo_toplevel() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("finding the repo root")?;
    if !out.status.success() {
        bail!("not inside a git repository");
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// If a worktree is already checked out on `branch`, return its path. Parses
/// `git worktree list --porcelain`, whose records pair a `worktree <path>` line
/// with a `branch refs/heads/<name>` line.
fn worktree_for_branch(branch: &str) -> Result<Option<PathBuf>> {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("listing worktrees")?;
    if !out.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let target = format!("refs/heads/{branch}");
    let mut current: Option<PathBuf> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current = Some(PathBuf::from(p));
        } else if line.strip_prefix("branch ") == Some(target.as_str()) {
            return Ok(current);
        }
    }
    Ok(None)
}

/// The set of local branch names, for marking issues that already have a branch.
/// Returns an empty set outside a git repo (callers treat that as "none").
pub fn local_branches() -> Result<HashSet<String>> {
    let out = Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .output()
        .context("listing git branches")?;
    if !out.status.success() {
        bail!("git for-each-ref failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Branch names that have a *separate* (linked) worktree, for marking issues you
/// can jump straight into. Excludes the main worktree (so the current branch
/// isn't flagged). Returns an empty set outside a git repo.
pub fn worktreed_branches() -> Result<HashSet<String>> {
    let main = repo_toplevel()?;
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("listing worktrees")?;
    if !out.status.success() {
        return Ok(HashSet::new());
    }
    Ok(parse_worktreed_branches(
        &String::from_utf8_lossy(&out.stdout),
        &main,
    ))
}

/// Parse `git worktree list --porcelain`, collecting the branch of every linked
/// worktree except the one living at `main` (the primary working tree).
fn parse_worktreed_branches(porcelain: &str, main: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut is_main = false;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            is_main = Path::new(p) == main;
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if !is_main {
                set.insert(b.to_string());
            }
        }
    }
    set
}

/// Whether a local branch named `branch` already exists.
fn branch_exists(branch: &str) -> Result<bool> {
    let status = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()
        .context("checking for an existing branch")?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_cases() {
        assert_eq!(slug("Fix login redirect"), "fix-login-redirect");
        assert_eq!(
            slug("Postgresql-HA wastes 32GB of RAM"),
            "postgresql-ha-wastes-32gb-of-ram"
        );
        assert_eq!(slug("  Trim & punctuation!! "), "trim-punctuation");
        assert_eq!(slug("Enable CD via Jenkins"), "enable-cd-via-jenkins");
        assert_eq!(slug("DIJO2 — Add AZ to VPC"), "dijo2-add-az-to-vpc");
        assert_eq!(slug(""), "");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn branch_cases() {
        assert_eq!(branch("DRM-1", "Fix login"), "DRM-1-fix-login");
        assert_eq!(branch("DRM-2", "!!!"), "DRM-2");
    }

    #[test]
    fn worktree_base_groups_by_repo_under_root() {
        let top = PathBuf::from("/Users/me/dev/repos/myrepo");
        assert_eq!(
            worktree_base_for(Some("/wt"), &top),
            PathBuf::from("/wt/myrepo")
        );
    }

    #[test]
    fn worktree_base_defaults_to_sibling() {
        let top = PathBuf::from("/Users/me/dev/repos/myrepo");
        assert_eq!(
            worktree_base_for(None, &top),
            PathBuf::from("/Users/me/dev/repos/myrepo-worktrees")
        );
    }

    #[test]
    fn expand_tilde_passthrough() {
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn worktreed_branches_excludes_main() {
        let porcelain = "\
worktree /Users/me/dev/repo
HEAD aaaa
branch refs/heads/main

worktree /Users/me/dev/worktrees/repo/DRM-1-foo
HEAD bbbb
branch refs/heads/DRM-1-foo

worktree /Users/me/dev/worktrees/repo/DRM-2-bar
HEAD cccc
branch refs/heads/DRM-2-bar

worktree /Users/me/dev/detached-wt
HEAD dddd
detached
";
        let set = parse_worktreed_branches(porcelain, Path::new("/Users/me/dev/repo"));
        assert_eq!(set.len(), 2);
        assert!(set.contains("DRM-1-foo"));
        assert!(set.contains("DRM-2-bar"));
        assert!(!set.contains("main")); // the primary working tree is excluded
    }
}
