//! Recoverable trashing: move a reclaimed worktree into the trash dir, drop its
//! git registration, and record the move in a manifest so a later run can purge
//! it once the grace period lapses (and so it could be restored in between).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::git;

/// One trashed worktree, as persisted in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    pub repo: String,
    pub repo_name: String,
    pub branch: String,
    pub original_path: String,
    pub trashed_path: String,
    pub trashed_at: u64,
}

/// The on-disk record of everything currently in the trash.
#[derive(Default)]
pub struct Manifest {
    pub entries: Vec<TrashEntry>,
    path: PathBuf,
}

impl Manifest {
    /// Load the manifest from `~/.cache/wt-gc/trash.json` (empty if absent).
    pub fn load() -> Manifest {
        let path = manifest_path();
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Manifest { entries, path }
    }

    /// Persist the manifest, creating the cache dir as needed.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.entries).context("serializing manifest")?;
        std::fs::write(&self.path, json).with_context(|| format!("writing {}", self.path.display()))
    }
}

/// Move `wt_path` into `trash_dir/<repo_name>/<branch>` (suffixing on collision),
/// then `git worktree prune` so the registration is dropped. Returns the record.
pub fn trash(
    repo: &Path,
    repo_name: &str,
    branch: &str,
    wt_path: &Path,
    trash_dir: &Path,
) -> Result<TrashEntry> {
    let now = now_secs();
    let mut dest = trash_dir.join(repo_name).join(branch);
    if dest.exists() {
        dest = trash_dir.join(repo_name).join(format!("{branch}-{now}"));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::rename(wt_path, &dest)
        .with_context(|| format!("moving {} to {}", wt_path.display(), dest.display()))?;
    git::prune(repo)?;
    Ok(TrashEntry {
        repo: repo.to_string_lossy().into_owned(),
        repo_name: repo_name.to_string(),
        branch: branch.to_string(),
        original_path: wt_path.to_string_lossy().into_owned(),
        trashed_path: dest.to_string_lossy().into_owned(),
        trashed_at: now,
    })
}

/// Permanently delete trash entries older than `retention_days`, and drop any
/// whose directory has already vanished. Returns the entries that were removed.
pub fn purge_expired(manifest: &mut Manifest, retention_days: u64) -> Vec<TrashEntry> {
    let now = now_secs();
    let cutoff = retention_days.saturating_mul(86_400);
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for entry in std::mem::take(&mut manifest.entries) {
        let path = PathBuf::from(&entry.trashed_path);
        let expired = now.saturating_sub(entry.trashed_at) >= cutoff;
        if !path.exists() {
            // Already gone (user emptied trash manually) — just forget it.
            continue;
        }
        if expired {
            let _ = std::fs::remove_dir_all(&path);
            removed.push(entry);
        } else {
            kept.push(entry);
        }
    }
    manifest.entries = kept;
    removed
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn manifest_path() -> PathBuf {
    common::xdg::base_dir("XDG_CACHE_HOME", ".cache")
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("wt-gc")
        .join("trash.json")
}
