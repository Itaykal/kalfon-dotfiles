//! A best-effort TTL file cache.
//!
//! Used to avoid slow network calls for data that changes rarely (e.g. the AWS
//! account list). Reads and writes are best-effort: any IO/parse error, or a
//! stale entry, yields `None` so the caller falls through to the live source.
//! Nothing here ever fails the tool.
//!
//! Entries live at `$XDG_CACHE_HOME/<tool>/<key>.json` (falling back to
//! `~/.cache/<tool>/<key>.json`), wrapped in a `{ stored_at, value }` envelope.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Cached<T> {
    stored_at: u64,
    value: T,
}

#[derive(Serialize)]
struct CachedRef<'a, T: Serialize> {
    stored_at: u64,
    value: &'a T,
}

/// Return the cached value for `(tool, key)` if present and newer than `ttl`.
/// Any problem (missing/unreadable/corrupt file, or a stale entry) → `None`.
pub fn load<T: DeserializeOwned>(tool: &str, key: &str, ttl: Duration) -> Option<T> {
    let path = cache_path(tool, key)?;
    let text = std::fs::read_to_string(path).ok()?;
    let cached: Cached<T> = serde_json::from_str(&text).ok()?;
    is_fresh(cached.stored_at, now_secs(), ttl).then_some(cached.value)
}

/// Write `value` for `(tool, key)`, stamping it with the current time.
/// Best-effort: the caller should ignore the error.
pub fn store<T: Serialize>(tool: &str, key: &str, value: &T) -> Result<()> {
    let path = cache_path(tool, key).context("resolving cache path")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let envelope = CachedRef {
        stored_at: now_secs(),
        value,
    };
    let json = serde_json::to_string(&envelope).context("serializing cache")?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))
}

/// An entry stamped at `stored_at` is fresh if it's younger than `ttl`.
/// Saturating subtraction tolerates clock skew (a future stamp counts as fresh).
fn is_fresh(stored_at: u64, now: u64, ttl: Duration) -> bool {
    now.saturating_sub(stored_at) < ttl.as_secs()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path(tool: &str, key: &str) -> Option<PathBuf> {
    let base = crate::xdg::base_dir("XDG_CACHE_HOME", ".cache")?;
    Some(base.join(tool).join(format!("{}.json", sanitize(key))))
}

/// Keep `key` a safe single path segment.
fn sanitize(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_respects_ttl() {
        let ttl = Duration::from_secs(60);
        assert!(is_fresh(100, 150, ttl)); // 50s old < 60s
        assert!(!is_fresh(100, 160, ttl)); // 60s old, not < 60s
        assert!(!is_fresh(100, 200, ttl)); // stale
    }

    #[test]
    fn future_stamp_is_fresh() {
        // Clock skew: stamped "in the future" must not be treated as stale.
        assert!(is_fresh(200, 100, Duration::from_secs(1)));
    }

    #[test]
    fn zero_ttl_is_never_fresh() {
        assert!(!is_fresh(100, 100, Duration::from_secs(0)));
    }

    #[test]
    fn envelope_round_trips() {
        let value = vec!["a".to_string(), "b".to_string()];
        let json = serde_json::to_string(&CachedRef {
            stored_at: 42,
            value: &value,
        })
        .unwrap();
        let back: Cached<Vec<String>> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.stored_at, 42);
        assert_eq!(back.value, value);
    }

    #[test]
    fn sanitize_makes_safe_segments() {
        assert_eq!(sanitize("accounts-session"), "accounts-session");
        assert_eq!(sanitize("a/b c"), "a_b_c");
    }
}
