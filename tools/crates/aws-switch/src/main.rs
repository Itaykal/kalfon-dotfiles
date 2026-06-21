//! aws-switch — pick an AWS SSO account + role and write it to the fixed
//! `default` profile in `~/.aws/config`.
//!
//! It re-authenticates the SSO session if needed, shows fuzzy pickers for the
//! account and role, then writes the choice into `~/.aws/config`. The account
//! list is cached (it rarely changes), so warm runs skip the network entirely.
//! It does **not** export anything: the pure-zsh `aws-sync-prompt` precmd hook
//! reads `~/.aws/config` on the next prompt and exports the `AWS_*` vars into
//! every shell. State lives in the file; this tool just writes it and exits.

mod aws;

use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(about = "Pick an AWS SSO account + role and switch the default profile")]
struct Args {
    /// Path to a config file (overrides $AWS_SWITCH_CONFIG and ~/.config/aws-switch/config.toml).
    #[arg(long)]
    config: Option<String>,

    /// Ignore the cached account list and refetch it from AWS.
    #[arg(long)]
    refresh: bool,
}

/// Tunable surface of the tool. Defaults match the dotfiles invariants
/// (CLAUDE.md): the SSO session is always `session`, the active profile is
/// always `default`.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct Config {
    /// SSO session name in `~/.aws/config`.
    sso_session: String,
    /// Profile to overwrite.
    profile: String,
    /// Fallback SSO region if `~/.aws/config` doesn't specify one.
    region: String,
    /// Account names to hide from the picker.
    exclude_accounts: Vec<String>,
    /// If set and available, auto-pick this role instead of prompting.
    default_role: Option<String>,
    /// How long a cached account list stays fresh, in seconds (0 disables caching).
    cache_ttl_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sso_session: "session".into(),
            profile: "default".into(),
            region: "eu-west-1".into(),
            exclude_accounts: Vec::new(),
            default_role: None,
            cache_ttl_secs: 43_200, // 12h
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("aws-switch: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Returns the desired process exit code: 0 on success, 1 if the user cancelled
/// a picker (so the calling shell sees a non-zero status and nothing changed).
fn run() -> Result<u8> {
    let args = Args::parse();
    let cfg: Config =
        common::config::load("aws-switch", "AWS_SWITCH_CONFIG", args.config.as_deref())?;

    let (cfg_region, start_url) = aws::read_sso_session(&cfg.sso_session)?;
    let region = cfg_region.unwrap_or_else(|| cfg.region.clone());
    let start_url = start_url.as_deref();

    // The token is only needed for a network call; it's fetched lazily and
    // refreshed (via login) on demand. read_token returns whatever's cached
    // without checking expiry — an expired token surfaces as Unauthorized.
    let mut token = aws::read_token(start_url)?;

    // Accounts: serve from cache when fresh; otherwise fetch (logging in and
    // retrying if the token is expired) and re-cache.
    let cache_key = format!("accounts-{}", cfg.sso_session);
    let ttl = Duration::from_secs(cfg.cache_ttl_secs);
    let cached = if args.refresh || cfg.cache_ttl_secs == 0 {
        None
    } else {
        common::cache::load::<Vec<aws::Account>>("aws-switch", &cache_key, ttl)
            .filter(|a| !a.is_empty())
    };
    let accounts = match cached {
        Some(accounts) => accounts,
        None => {
            let fetched = with_login_retry(
                &cfg,
                start_url,
                &region,
                &mut token,
                "Loading accounts…",
                |t| aws::list_accounts(t, &region),
            )?;
            let _ = common::cache::store("aws-switch", &cache_key, &fetched);
            fetched
        }
    };

    // Filter excludes after loading, so config edits apply even to cached data.
    let accounts: Vec<aws::Account> = accounts
        .into_iter()
        .filter(|a| !cfg.exclude_accounts.contains(&a.account_name))
        .collect();
    if accounts.is_empty() {
        bail!("no accounts available");
    }

    let Some(account) = common::select("AWS account", accounts, |a| {
        format!("{:<35} {}", a.account_name, a.account_id)
    })?
    else {
        return Ok(1); // cancelled
    };

    let roles = with_login_retry(&cfg, start_url, &region, &mut token, "Loading roles…", |t| {
        aws::list_roles(t, &region, &account.account_id)
    })?;
    let role = match select_role(&cfg, roles)? {
        Some(role) => role,
        None => return Ok(1), // cancelled or no roles
    };

    // Overwrite the fixed profile in one file write. `sso_account_name` is
    // non-standard, but the AWS CLI ignores unknown keys and aws-sync-prompt
    // reads it for the prompt.
    aws::write_profile(
        &cfg.profile,
        &[
            ("sso_session", cfg.sso_session.as_str()),
            ("sso_account_id", account.account_id.as_str()),
            ("sso_account_name", account.account_name.as_str()),
            ("sso_role_name", role.as_str()),
            ("region", region.as_str()),
        ],
    )?;

    eprintln!(
        "{}  {}  {}  {}",
        account.account_name, account.account_id, role, region
    );
    Ok(0)
}

/// Run an SSO call behind a spinner, ensuring there's a token first and, if the
/// token turns out to be expired (`Unauthorized`), renewing it and retrying once.
fn with_login_retry<T, F>(
    cfg: &Config,
    start_url: Option<&str>,
    region: &str,
    token: &mut Option<String>,
    label: &str,
    call: F,
) -> Result<T>
where
    T: Send,
    F: Fn(&str) -> Result<T> + Sync,
{
    if token.is_none() {
        refresh_or_login(cfg, start_url, region, token)?;
    }
    let t = token.clone().context("no SSO token available")?;
    match common::spinner::run(label, || call(&t)) {
        Err(e) if is_unauthorized(&e) => {
            refresh_or_login(cfg, start_url, region, token)?;
            let t = token
                .clone()
                .context("could not read SSO token after renewal")?;
            common::spinner::run(label, || call(&t))
        }
        other => other,
    }
}

/// Renew the SSO token in `*token`. Prefer a silent refresh using the cached
/// refresh token; only fall back to the browser flow `aws sso login` when no
/// refresh is possible (no cached refresh token, or it has itself expired).
fn refresh_or_login(
    cfg: &Config,
    start_url: Option<&str>,
    region: &str,
    token: &mut Option<String>,
) -> Result<()> {
    if let Some(rec) = aws::read_token_record(start_url)? {
        if let Some(new) = aws::try_refresh(&rec, region)? {
            *token = Some(new);
            return Ok(());
        }
    }
    eprintln!("SSO session expired — logging in…");
    aws::login(&cfg.sso_session)?;
    *token = aws::read_token(start_url)?;
    Ok(())
}

fn is_unauthorized(e: &anyhow::Error) -> bool {
    e.downcast_ref::<aws::Unauthorized>().is_some()
}

/// Auto-select when there's a single role or a configured `default_role` that
/// exists; otherwise prompt. Returns `None` on cancel or when there are no roles.
fn select_role(cfg: &Config, roles: Vec<String>) -> Result<Option<String>> {
    match roles.len() {
        0 => Ok(None),
        1 => Ok(roles.into_iter().next()),
        _ => {
            if let Some(want) = &cfg.default_role {
                if roles.iter().any(|r| r == want) {
                    return Ok(Some(want.clone()));
                }
            }
            common::select("Role", roles, |r| r.clone())
        }
    }
}
