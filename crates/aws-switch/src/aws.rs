//! Everything that touches AWS SSO, the token cache, and `~/.aws/config`.
//!
//! Account/role listing hits the SSO portal REST API directly over HTTPS using
//! the cached bearer token — the same endpoints `aws sso list-accounts` calls,
//! but without paying the ~400ms `aws` CLI (Python) startup on each one. When a
//! token expires we first try a silent refresh against the SSO-OIDC `/token`
//! endpoint (the same `refresh_token` grant the AWS SDK uses), and only shell
//! out to the browser/device flow `aws sso login` if that refresh fails.
//! `~/.aws/config` is written in the AWS CLI's exact `key = value` / `[default]`
//! format so both the real `aws` CLI and the pure-zsh `aws-sync-prompt` parser
//! keep working.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// An expired/invalid SSO token (the portal answered 401/403). The caller
/// detects this with `downcast_ref` and responds by logging in and retrying.
#[derive(Debug)]
pub struct Unauthorized;

impl std::fmt::Display for Unauthorized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SSO token is unauthorized (expired)")
    }
}

impl std::error::Error for Unauthorized {}

/// An AWS account from the SSO portal `list-accounts` API. `Serialize` so the
/// list can be cached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "accountName")]
    pub account_name: String,
}

#[derive(Deserialize)]
struct AccountsResponse {
    #[serde(rename = "accountList", default)]
    account_list: Vec<Account>,
    #[serde(rename = "nextToken", default)]
    next_token: Option<String>,
}

#[derive(Deserialize)]
struct RolesResponse {
    #[serde(rename = "roleList", default)]
    role_list: Vec<Role>,
    #[serde(rename = "nextToken", default)]
    next_token: Option<String>,
}

#[derive(Deserialize)]
struct Role {
    #[serde(rename = "roleName")]
    role_name: String,
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("could not determine home directory")
}

/// Read `sso_region` and `sso_start_url` from the `[sso-session <session>]`
/// block of `~/.aws/config`. Either may be absent.
pub fn read_sso_session(session: &str) -> Result<(Option<String>, Option<String>)> {
    let path = home()?.join(".aws/config");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    let header = format!("[sso-session {session}]");
    let (mut region, mut start_url) = (None, None);
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == header;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "sso_region" => region = Some(v.trim().to_string()),
                "sso_start_url" => start_url = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    Ok((region, start_url))
}

/// A cached SSO token file (`~/.aws/sso/cache/*.json`), parsed into the fields
/// we care about. Everything but `path`/`access_token`/`expires_at` is optional
/// because client-registration files (which we skip) and older token formats
/// may omit them; a refresh needs `refresh_token` + `client_id`/`client_secret`.
pub struct CachedToken {
    /// The cache file this came from, so a refresh can rewrite it in place.
    pub path: PathBuf,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// SSO region recorded in the file, used to pick the OIDC endpoint.
    pub region: Option<String>,
    /// RFC3339 UTC; lexically comparable, so used to pick the freshest entry.
    pub expires_at: String,
}

/// Return the cached SSO token record for `start_url` (or any session if
/// `start_url` is `None`), choosing the one with the latest `expiresAt`.
pub fn read_token_record(start_url: Option<&str>) -> Result<Option<CachedToken>> {
    let dir = home()?.join(".aws/sso/cache");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };

    let mut best: Option<CachedToken> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(token) = value.get("accessToken").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(want) = start_url {
            let got = value.get("startUrl").and_then(|v| v.as_str());
            if got != Some(want) {
                continue;
            }
        }
        let str_field = |k: &str| {
            value
                .get(k)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        // expiresAt is RFC3339 UTC, so lexical comparison orders it correctly.
        let expires = str_field("expiresAt").unwrap_or_default();
        if best.as_ref().is_none_or(|b| expires > b.expires_at) {
            best = Some(CachedToken {
                path: path.clone(),
                access_token: token.to_string(),
                refresh_token: str_field("refreshToken"),
                client_id: str_field("clientId"),
                client_secret: str_field("clientSecret"),
                region: str_field("region"),
                expires_at: expires,
            });
        }
    }
    Ok(best)
}

/// Return just the cached SSO access token for `start_url`, choosing the one
/// with the latest `expiresAt`. Thin wrapper over [`read_token_record`].
pub fn read_token(start_url: Option<&str>) -> Result<Option<String>> {
    Ok(read_token_record(start_url)?.map(|t| t.access_token))
}

/// `aws sso login --sso-session <session>`, inheriting the terminal so the
/// device-code prompt is visible.
pub fn login(session: &str) -> Result<()> {
    let status = Command::new("aws")
        .args(["sso", "login", "--sso-session", session])
        .status()
        .context("running `aws sso login`")?;
    if !status.success() {
        bail!("`aws sso login` failed");
    }
    Ok(())
}

#[derive(Deserialize)]
struct RefreshResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresIn")]
    expires_in: i64,
    /// The refresh token may be rotated; if present we persist the new one.
    #[serde(rename = "refreshToken", default)]
    refresh_token: Option<String>,
}

/// Try to silently renew `rec`'s access token via the SSO-OIDC `refresh_token`
/// grant — the same call the AWS SDK makes under the hood, so we never have to
/// open the browser while the refresh token is still valid.
///
/// Returns `Ok(Some(new_token))` on success (and rewrites the cache file in
/// place so the AWS CLI and future runs see it too). Returns `Ok(None)` when a
/// refresh isn't possible or is rejected (missing refresh material, or the
/// refresh token itself expired → HTTP 400 `invalid_grant`), signalling the
/// caller to fall back to `aws sso login`. Only genuinely unexpected I/O errors
/// propagate as `Err`.
pub fn try_refresh(rec: &CachedToken, region: &str) -> Result<Option<String>> {
    let (Some(refresh_token), Some(client_id), Some(client_secret)) =
        (&rec.refresh_token, &rec.client_id, &rec.client_secret)
    else {
        return Ok(None);
    };
    let region = rec.region.as_deref().unwrap_or(region);
    let url = format!("https://oidc.{region}.amazonaws.com/token");

    let resp = match ureq::post(&url).send_json(serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "grantType": "refresh_token",
        "refreshToken": refresh_token,
    })) {
        Ok(resp) => resp,
        // Any refusal (expired/revoked refresh token, etc.) → fall back to login.
        Err(ureq::Error::Status(_, _)) => return Ok(None),
        Err(e) => return Err(anyhow::Error::new(e)).context("SSO-OIDC refresh request"),
    };
    let refreshed: RefreshResponse = resp
        .into_json()
        .context("parsing SSO-OIDC refresh response")?;

    // Persist the new token so the AWS CLI and our next run benefit. A wrong
    // expiresAt is self-correcting (a 401 just triggers another refresh), so a
    // failure to rewrite the cache is non-fatal — we still return the token.
    let new_refresh = refreshed.refresh_token.as_deref();
    let expires_at = unix_to_rfc3339(now_secs() + refreshed.expires_in);
    if let Ok(original) = std::fs::read_to_string(&rec.path) {
        let updated =
            update_token_json(&original, &refreshed.access_token, &expires_at, new_refresh);
        let _ = write_atomic(&rec.path, &updated);
    }

    Ok(Some(refreshed.access_token))
}

/// Seconds since the Unix epoch (same clock source as `common::cache`).
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Format Unix `secs` as `YYYY-MM-DDTHH:MM:SSZ` (the RFC3339 UTC form the AWS
/// CLI writes for `expiresAt`), using the civil-from-days algorithm so we need
/// no date crate.
fn unix_to_rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Howard Hinnant's civil_from_days: days since 1970-01-01 → (y, m, d).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = yoe + era * 400 + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// The SSO portal endpoint for a region (the host the AWS CLI talks to under
/// the hood for `sso list-accounts` / `list-account-roles`).
fn portal(region: &str) -> String {
    format!("https://portal.sso.{region}.amazonaws.com")
}

const BEARER_HEADER: &str = "x-amz-sso_bearer_token";
const PAGE_SIZE: &str = "100";

/// Send an SSO portal request, turning a 401/403 into [`Unauthorized`] (so the
/// caller can log in and retry) and any other failure into a contextual error.
fn send(req: ureq::Request, what: &str) -> Result<ureq::Response> {
    match req.call() {
        Ok(resp) => Ok(resp),
        Err(ureq::Error::Status(401 | 403, _)) => Err(anyhow::Error::new(Unauthorized)),
        Err(e) => Err(anyhow::Error::new(e)).with_context(|| format!("{what} request")),
    }
}

/// List accounts the token can access, following pagination. A non-2xx
/// response (e.g. an expired token → 401) surfaces as an `Err`, which the
/// caller treats as "log in and retry".
pub fn list_accounts(token: &str, region: &str) -> Result<Vec<Account>> {
    let url = format!("{}/assignment/accounts", portal(region));
    let mut accounts = Vec::new();
    let mut next: Option<String> = None;
    loop {
        let mut req = ureq::get(&url)
            .set(BEARER_HEADER, token)
            .query("max_result", PAGE_SIZE);
        if let Some(n) = &next {
            req = req.query("next_token", n);
        }
        let page: AccountsResponse = send(req, "SSO list-accounts")?
            .into_json()
            .context("parsing list-accounts response")?;
        accounts.extend(page.account_list);
        match page.next_token {
            Some(n) if !n.is_empty() => next = Some(n),
            _ => break,
        }
    }
    Ok(accounts)
}

/// List the role names available for an account, following pagination.
pub fn list_roles(token: &str, region: &str, account_id: &str) -> Result<Vec<String>> {
    let url = format!("{}/assignment/roles", portal(region));
    let mut roles = Vec::new();
    let mut next: Option<String> = None;
    loop {
        let mut req = ureq::get(&url)
            .set(BEARER_HEADER, token)
            .query("account_id", account_id)
            .query("max_result", PAGE_SIZE);
        if let Some(n) = &next {
            req = req.query("next_token", n);
        }
        let page: RolesResponse = send(req, "SSO list-account-roles")?
            .into_json()
            .context("parsing list-account-roles response")?;
        roles.extend(page.role_list.into_iter().map(|r| r.role_name));
        match page.next_token {
            Some(n) if !n.is_empty() => next = Some(n),
            _ => break,
        }
    }
    Ok(roles)
}

/// Write the given key/values into `profile`'s section of `~/.aws/config`,
/// preserving every other line.
///
/// This replaces five sequential `aws configure set` calls — each of which pays
/// the full `aws` CLI (Python) startup cost (~1s total). We update the file
/// directly instead, matching the exact `key = value` / `[default]` format the
/// AWS CLI writes, so both the real `aws` CLI and the pure-zsh `aws-sync-prompt`
/// parser keep working. Existing keys are updated in place; missing ones are
/// appended to the section; unrelated profiles are untouched.
///
/// The new contents are written atomically (temp file + rename), so a crash or
/// full disk mid-write can never truncate the user's `~/.aws/config` — they
/// keep the previous file intact.
pub fn write_profile(profile: &str, entries: &[(&str, &str)]) -> Result<()> {
    let path = home()?.join(".aws/config");
    let original = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    // The default profile is `[default]`; others are `[profile NAME]`.
    let header = if profile == "default" {
        "[default]".to_string()
    } else {
        format!("[profile {profile}]")
    };

    let out = update_ini(&original, &header, entries);
    write_atomic(&path, &out)
}

/// Replace `path`'s contents with `contents` atomically: write a sibling temp
/// file, then `rename` it over the target (an atomic swap on the same
/// filesystem). The temp file is removed if the rename fails. The temp lives in
/// the target's own directory so the rename never crosses a filesystem boundary
/// (a cross-device rename isn't atomic and would fail).
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    // Per-pid temp name so concurrent invocations don't clobber each other.
    let tmp = dir.join(format!(".{name}.tmp.{}", std::process::id()));

    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("replacing {}", path.display()))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
}

/// Pure core of [`write_profile`]: return `original` with `entries` set under
/// `header`, preserving all other lines. Existing keys are updated in place,
/// missing keys are appended to the section, and the section is created at EOF
/// if absent. Output always ends with a trailing newline.
fn update_ini(original: &str, header: &str, entries: &[(&str, &str)]) -> String {
    let mut lines: Vec<String> = original.lines().map(str::to_string).collect();

    match lines.iter().position(|l| l.trim() == header) {
        Some(start) => {
            // Section runs until the next section header (or EOF).
            let end = lines[start + 1..]
                .iter()
                .position(|l| l.trim_start().starts_with('['))
                .map(|i| start + 1 + i)
                .unwrap_or(lines.len());

            let mut append = Vec::new();
            for &(k, v) in entries {
                match (start + 1..end).find(|&i| line_key(&lines[i]) == Some(k)) {
                    Some(i) => lines[i] = format!("{k} = {v}"),
                    None => append.push(format!("{k} = {v}")),
                }
            }
            for (offset, line) in append.into_iter().enumerate() {
                lines.insert(end + offset, line);
            }
        }
        None => {
            lines.push(header.to_string());
            for &(k, v) in entries {
                lines.push(format!("{k} = {v}"));
            }
        }
    }

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The key of an `ini` `key = value` line, or `None` for headers/blanks.
fn line_key(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('[') {
        return None;
    }
    line.split_once('=').map(|(k, _)| k.trim())
}

/// Pure core of the cache rewrite in [`try_refresh`]: return `original` (an SSO
/// token cache JSON object) with `accessToken` and `expiresAt` replaced and
/// `refreshToken` set when `refresh` is `Some`, preserving every other key. If
/// `original` isn't a JSON object it's returned unchanged.
fn update_token_json(
    original: &str,
    access_token: &str,
    expires_at: &str,
    refresh: Option<&str>,
) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(original) else {
        return original.to_string();
    };
    let Some(obj) = value.as_object_mut() else {
        return original.to_string();
    };
    obj.insert("accessToken".into(), access_token.into());
    obj.insert("expiresAt".into(), expires_at.into());
    if let Some(refresh) = refresh {
        obj.insert("refreshToken".into(), refresh.into());
    }
    serde_json::to_string(&value).unwrap_or_else(|_| original.to_string())
}

#[cfg(test)]
mod tests {
    use super::{unix_to_rfc3339, update_ini, update_token_json, write_atomic};

    #[test]
    fn unix_to_rfc3339_formats_known_timestamps() {
        // 2026-06-21T08:52:26Z — the format AWS writes for `expiresAt`.
        assert_eq!(unix_to_rfc3339(1_782_031_946), "2026-06-21T08:52:26Z");
        // Epoch and a leap-day boundary to exercise the civil-date math.
        assert_eq!(unix_to_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_to_rfc3339(1_583_020_800), "2020-03-01T00:00:00Z");
    }

    #[test]
    fn update_token_json_replaces_fields_and_preserves_others() {
        let original = r#"{"startUrl":"https://x/start","region":"eu-west-1","accessToken":"OLD","expiresAt":"2020-01-01T00:00:00Z","refreshToken":"OLDR","clientId":"cid"}"#;
        let out = update_token_json(original, "NEW", "2026-06-21T08:52:26Z", Some("NEWR"));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["accessToken"], "NEW");
        assert_eq!(v["expiresAt"], "2026-06-21T08:52:26Z");
        assert_eq!(v["refreshToken"], "NEWR");
        // Untouched keys survive.
        assert_eq!(v["startUrl"], "https://x/start");
        assert_eq!(v["region"], "eu-west-1");
        assert_eq!(v["clientId"], "cid");
    }

    #[test]
    fn update_token_json_keeps_old_refresh_when_none() {
        let original = r#"{"accessToken":"OLD","expiresAt":"2020-01-01T00:00:00Z","refreshToken":"KEEP"}"#;
        let out = update_token_json(original, "NEW", "2026-06-21T08:52:26Z", None);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["accessToken"], "NEW");
        assert_eq!(v["refreshToken"], "KEEP");
    }

    #[test]
    fn update_token_json_passes_through_non_object() {
        assert_eq!(update_token_json("not json", "N", "E", None), "not json");
    }

    #[test]
    fn write_atomic_creates_overwrites_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("aws-switch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config");

        write_atomic(&path, "first\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\n");

        write_atomic(&path, "second\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second\n");

        // The temp file must not linger after a successful rename.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn updates_existing_keys_in_place_and_preserves_others() {
        let original = "\
[profile dev]
sso_account_id = 111
[default]
sso_account_id = 111
region = eu-west-1
[sso-session session]
sso_region = eu-west-1
";
        let got = update_ini(
            original,
            "[default]",
            &[("sso_account_id", "222"), ("region", "us-east-1")],
        );
        assert_eq!(
            got,
            "\
[profile dev]
sso_account_id = 111
[default]
sso_account_id = 222
region = us-east-1
[sso-session session]
sso_region = eu-west-1
"
        );
    }

    #[test]
    fn appends_missing_keys_to_the_section() {
        let original = "[default]\nregion = eu-west-1\n[other]\nx = 1\n";
        let got = update_ini(original, "[default]", &[("sso_role_name", "DevOps")]);
        assert_eq!(
            got,
            "[default]\nregion = eu-west-1\nsso_role_name = DevOps\n[other]\nx = 1\n"
        );
    }

    #[test]
    fn creates_section_when_absent() {
        let got = update_ini("[other]\nx = 1\n", "[default]", &[("region", "eu-west-1")]);
        assert_eq!(got, "[other]\nx = 1\n[default]\nregion = eu-west-1\n");
    }

    #[test]
    fn non_default_profile_uses_profile_prefix_section() {
        // write_profile picks the header; here we just confirm update_ini honors it.
        let got = update_ini("", "[profile work]", &[("region", "eu-west-1")]);
        assert_eq!(got, "[profile work]\nregion = eu-west-1\n");
    }
}
