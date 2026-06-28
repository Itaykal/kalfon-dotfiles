//! feature — pick an open Jira issue (or create one) and `git checkout -b
//! KEY-slug`, or spin it out into a git worktree (ctrl-enter). A standalone Rust
//! CLI: the list is cached (stale-while-revalidate) so it shows instantly and
//! refreshes in the background; the description preview loads async; `ctrl-n`
//! creates; `ctrl-r` force-refreshes.

mod adf;
mod config;
mod filter;
mod jira;
mod markdown;
mod tracker;
mod ui;
mod vcs;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;

use common::cache;
use config::Config;
use jira::Jira;
use tracker::{CreateRequest, Issue, Tracker};
use ui::{Action, Outcome};

const CACHE_KEY: &str = "issues";

#[derive(Parser)]
#[command(about = "Pick a Jira issue (or create one) and branch from it")]
struct Args {
    /// Path to a config file (overrides $FEATURE_CONFIG and ~/.config/feature/config.toml).
    #[arg(long)]
    config: Option<String>,
    /// Ignore the cached issue list and refetch it from Jira.
    #[arg(long)]
    refresh: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("feature: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8> {
    let args = Args::parse();
    let cfg: Config = common::config::load("feature", "FEATURE_CONFIG", args.config.as_deref())?;

    let ttl = Duration::from_secs(cfg.cache_ttl_secs);
    let use_cache = !args.refresh && cfg.cache_ttl_secs > 0;
    let aliases = cfg.aliases.clone();
    let worktree_dir = cfg.worktree_dir.clone();
    let tracker = Arc::new(Jira::new(cfg));

    // Stale-while-revalidate: serve the cached list instantly and refresh in the
    // background; a cold start (no cache) fetches once behind a spinner.
    let cached = if use_cache {
        cache::load::<Vec<Issue>>("feature", CACHE_KEY, ttl)
    } else {
        None
    };
    let (items, auto_refresh) = match cached {
        Some(items) => (items, true),
        None => {
            let t = Arc::clone(&tracker);
            let items = common::spinner::run("Loading issues…", move || t.list())?;
            let _ = cache::store("feature", CACHE_KEY, &items);
            (items, false)
        }
    };

    // Closures the picker drives on background threads.
    let refresh_job = {
        let t = Arc::clone(&tracker);
        move || {
            let items = t.list()?;
            let _ = cache::store("feature", CACHE_KEY, &items);
            Ok(items)
        }
    };
    let describe = {
        let t = Arc::clone(&tracker);
        move |key: String| t.describe(&key)
    };
    let create = {
        let t = Arc::clone(&tracker);
        move |req: CreateRequest| t.create(&req)
    };

    // Mark issues that already have a local branch / a separate worktree
    // (best-effort). A worktree means Enter jumps into it instead of branching.
    let branches = vcs::local_branches().unwrap_or_default();
    let worktrees = vcs::worktreed_branches().unwrap_or_default();

    match ui::run(
        aliases,
        items,
        branches,
        worktrees,
        refresh_job,
        describe,
        create,
        auto_refresh,
    )? {
        Outcome::Selected {
            key,
            summary,
            action,
        } => {
            let branch = vcs::branch(&key, &summary);
            match action {
                Action::Branch => vcs::checkout(&branch)?,
                Action::Worktree => {
                    let base = (!worktree_dir.is_empty()).then_some(worktree_dir.as_str());
                    let path = vcs::worktree_add(&branch, base)?;
                    // The TUI draws to stderr, so stdout carries just the path —
                    // copy it or `cd "$(feature)"` into the new worktree.
                    println!("{}", path.display());
                }
            }
            Ok(0)
        }
        Outcome::Cancelled => Ok(0),
    }
}
