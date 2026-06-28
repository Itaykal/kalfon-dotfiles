//! wt-gc — reclaim stale git worktrees.
//!
//! Scans the configured repo roots, and for every linked worktree decides:
//! *auto* (stale + clean + pushed) → move to the trash dir; *confirm* (stale but
//! dirty or unpushed) → leave for `wt-gc review`; *skip* (not stale). Trashed
//! worktrees are kept for a grace period, then purged. The default `run` is what
//! a daily LaunchAgent invokes; bare `wt-gc` lists (a safe dry run).

mod config;
mod git;
mod model;
mod trash;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use config::Config;
use model::{classify, Bucket, Candidate};
use trash::{purge_expired, Manifest};

#[derive(Parser)]
#[command(about = "Garbage-collect stale git worktrees")]
struct Args {
    /// Path to a config file (overrides $WT_GC_CONFIG and ~/.config/wt-gc/config.toml).
    #[arg(long)]
    config: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Trash safe stale worktrees, purge expired trash, log the rest. No prompts.
    Run,
    /// Interactively review stale worktrees that have unpushed/uncommitted work.
    Review,
    /// Show how each worktree would be classified; change nothing (the default).
    List,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wt-gc: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let cfg: Config = common::config::load("wt-gc", "WT_GC_CONFIG", args.config.as_deref())?;
    match args.cmd.unwrap_or(Cmd::List) {
        Cmd::Run => cmd_run(&cfg),
        Cmd::Review => cmd_review(&cfg),
        Cmd::List => cmd_list(&cfg),
    }
}

/// Discover every linked worktree under the repo roots and classify it.
fn scan(cfg: &Config) -> Vec<Candidate> {
    let trash = cfg.trash_path();
    let mut out = Vec::new();
    for repo in git::find_main_repos(&cfg.roots()) {
        let repo_name = repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".into());
        let Ok(worktrees) = git::linked_worktrees(&repo) else {
            continue;
        };
        for wt in worktrees {
            if wt.path.starts_with(&trash) {
                continue;
            }
            let stale = git::head_commit_time(&wt.path)
                .map(|ts| git::older_than(ts, cfg.stale_days))
                .unwrap_or(false);
            let clean = git::is_clean(&wt.path).unwrap_or(false);
            let push = git::push_state(&wt.path);
            let bucket = classify(stale, clean, push.is_pushed());
            out.push(Candidate {
                repo: repo.clone(),
                repo_name: repo_name.clone(),
                path: wt.path,
                branch: wt.branch,
                clean,
                push,
                bucket,
            });
        }
    }
    out
}

/// `wt-gc run` — the LaunchAgent entry point.
fn cmd_run(cfg: &Config) -> Result<()> {
    let mut log = Logger::new();
    let mut manifest = Manifest::load();

    for entry in purge_expired(&mut manifest, cfg.trash_retention_days) {
        log.line(&format!(
            "purged (>{}d): {}",
            cfg.trash_retention_days, entry.trashed_path
        ));
    }

    let cands = scan(cfg);
    let trash_dir = cfg.trash_path();
    let mut trashed = 0usize;
    let mut review = 0usize;
    for c in &cands {
        match c.bucket {
            Bucket::Auto => match trash::trash(&c.repo, &c.repo_name, &c.branch, &c.path, &trash_dir)
            {
                Ok(entry) => {
                    log.line(&format!("trashed {} → {}", c.path.display(), entry.trashed_path));
                    manifest.entries.push(entry);
                    trashed += 1;
                }
                Err(e) => log.line(&format!("FAILED to trash {}: {e:#}", c.path.display())),
            },
            Bucket::Confirm => {
                review += 1;
                log.line(&format!(
                    "needs review: {} ({})",
                    c.path.display(),
                    c.reasons().join(", ")
                ));
            }
            Bucket::Skip => {}
        }
    }
    manifest.save()?;
    log.line(&format!(
        "done: {trashed} trashed, {review} need review (run `wt-gc review`)"
    ));
    Ok(())
}

/// `wt-gc review` — confirm each dirty/unpushed stale worktree before trashing.
fn cmd_review(cfg: &Config) -> Result<()> {
    let cands = scan(cfg);
    let pending: Vec<&Candidate> = cands.iter().filter(|c| c.bucket == Bucket::Confirm).collect();
    if pending.is_empty() {
        println!("Nothing to review — no stale worktrees with pending work.");
        return Ok(());
    }
    let mut manifest = Manifest::load();
    let trash_dir = cfg.trash_path();
    for c in pending {
        println!(
            "\n{}\n  repo {}  branch {}\n  {}",
            c.path.display(),
            c.repo_name,
            c.branch,
            c.reasons().join(", ")
        );
        if prompt_yes(&format!("Trash {}?", c.branch))? {
            match trash::trash(&c.repo, &c.repo_name, &c.branch, &c.path, &trash_dir) {
                Ok(entry) => {
                    println!("  trashed → {}", entry.trashed_path);
                    manifest.entries.push(entry);
                    manifest.save()?;
                }
                Err(e) => eprintln!("  failed: {e:#}"),
            }
        } else {
            println!("  kept");
        }
    }
    Ok(())
}

/// `wt-gc list` — dry run; print the classification, change nothing.
fn cmd_list(cfg: &Config) -> Result<()> {
    let cands = scan(cfg);
    if cands.is_empty() {
        println!("No linked worktrees found under the configured roots.");
        return Ok(());
    }
    let show = |bucket: Bucket, header: &str| {
        let rows: Vec<&Candidate> = cands.iter().filter(|c| c.bucket == bucket).collect();
        if rows.is_empty() {
            return;
        }
        println!("\n{header}");
        for c in rows {
            // Reasons only matter for the review bucket; elsewhere they'd mislead.
            let suffix = match (bucket, c.reasons()) {
                (Bucket::Confirm, why) if !why.is_empty() => format!("  ({})", why.join(", ")),
                _ => String::new(),
            };
            println!("  {}/{}{}", c.repo_name, c.branch, suffix);
        }
    };
    show(Bucket::Auto, "would auto-trash (stale, clean, pushed):");
    show(Bucket::Confirm, "needs review (stale, but has pending work):");
    show(Bucket::Skip, "keeping (active):");
    Ok(())
}

/// Prompt on stdin; true only for an explicit yes.
fn prompt_yes(question: &str) -> Result<bool> {
    print!("{question} [y/N] ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let a = line.trim().to_ascii_lowercase();
    Ok(a == "y" || a == "yes")
}

/// Echoes to stdout and appends to `~/.cache/wt-gc/wt-gc.log` for auditing.
struct Logger {
    file: Option<std::fs::File>,
}

impl Logger {
    fn new() -> Self {
        let path = log_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        Logger { file }
    }

    fn line(&mut self, msg: &str) {
        println!("{msg}");
        if let Some(f) = &mut self.file {
            let _ = writeln!(f, "{} {msg}", now_secs());
        }
    }
}

fn log_path() -> PathBuf {
    common::xdg::base_dir("XDG_CACHE_HOME", ".cache")
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("wt-gc")
        .join("wt-gc.log")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
