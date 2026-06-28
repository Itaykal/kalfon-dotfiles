//! The decision model: turn git facts about a worktree into a GC bucket.

use std::path::PathBuf;

/// What should happen to a discovered worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// Stale, clean, and pushed — safe to reclaim automatically.
    Auto,
    /// Stale but has work that isn't safely throwaway — ask before reclaiming.
    Confirm,
    /// Not stale — leave it alone.
    Skip,
}

/// How a worktree's branch relates to its upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushState {
    /// Upstream exists and HEAD is not ahead of it.
    Pushed,
    /// Upstream exists but HEAD is ahead by this many commits.
    Ahead(u64),
    /// No upstream is configured at all.
    NoUpstream,
}

impl PushState {
    pub fn is_pushed(self) -> bool {
        matches!(self, PushState::Pushed)
    }
}

/// Decide a worktree's bucket from the three facts the GC keys on.
pub fn classify(stale: bool, clean: bool, pushed: bool) -> Bucket {
    if !stale {
        Bucket::Skip
    } else if clean && pushed {
        Bucket::Auto
    } else {
        Bucket::Confirm
    }
}

/// A linked worktree under consideration, with the facts and verdict.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The main repository this worktree belongs to (its toplevel).
    pub repo: PathBuf,
    pub repo_name: String,
    /// The worktree's own directory.
    pub path: PathBuf,
    pub branch: String,
    pub clean: bool,
    pub push: PushState,
    pub bucket: Bucket,
}

impl Candidate {
    /// Human-readable reasons a stale worktree needs confirmation (empty for the
    /// auto bucket).
    pub fn reasons(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.clean {
            out.push("uncommitted changes".into());
        }
        match self.push {
            PushState::Ahead(n) => out.push(format!("{n} unpushed commit(s)")),
            PushState::NoUpstream => out.push("no upstream branch".into()),
            PushState::Pushed => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_buckets() {
        // Not stale → always skipped, regardless of the rest.
        assert_eq!(classify(false, true, true), Bucket::Skip);
        assert_eq!(classify(false, false, false), Bucket::Skip);
        // Stale + clean + pushed → auto.
        assert_eq!(classify(true, true, true), Bucket::Auto);
        // Stale but dirty or unpushed → confirm.
        assert_eq!(classify(true, false, true), Bucket::Confirm);
        assert_eq!(classify(true, true, false), Bucket::Confirm);
        assert_eq!(classify(true, false, false), Bucket::Confirm);
    }

    #[test]
    fn reasons_describe_why() {
        let c = |clean, push| Candidate {
            repo: PathBuf::from("/r"),
            repo_name: "r".into(),
            path: PathBuf::from("/r/wt"),
            branch: "b".into(),
            clean,
            push,
            bucket: Bucket::Confirm,
        };
        assert!(c(true, PushState::Pushed).reasons().is_empty());
        assert_eq!(c(false, PushState::Pushed).reasons(), ["uncommitted changes"]);
        assert_eq!(c(true, PushState::Ahead(2)).reasons(), ["2 unpushed commit(s)"]);
        assert_eq!(c(true, PushState::NoUpstream).reasons(), ["no upstream branch"]);
    }
}
