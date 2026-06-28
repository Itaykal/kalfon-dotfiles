//! XDG-aware TOML config loading, shared by every tool.
//!
//! Resolution order, matching the Go `feature` tool:
//!   1. an explicit `--config PATH`
//!   2. the tool's `$<TOOL>_CONFIG` env var
//!   3. `$XDG_CONFIG_HOME/<tool>/config.toml` (falling back to `~/.config/...`)
//!   4. built-in defaults
//!
//! A missing file is **not** an error — defaults are returned. Partial files
//! layer over the defaults, so a tool's config struct should be
//! `#[serde(default)]` with a real `Default` impl (then any key the file omits
//! falls back to that default, exactly like Go's `toml.DecodeFile`).

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

/// Load configuration for `tool` (e.g. `"aws-switch"`), reading the env var
/// `env_var` (e.g. `"AWS_SWITCH_CONFIG"`) and an optional explicit path.
pub fn load<T>(tool: &str, env_var: &str, explicit: Option<&str>) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    let Some(path) = resolve_path(tool, env_var, explicit) else {
        return Ok(T::default());
    };

    match std::fs::read_to_string(&path) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("reading config {}", path.display())),
    }
}

fn resolve_path(tool: &str, env_var: &str, explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var(env_var) {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let base = crate::xdg::base_dir("XDG_CONFIG_HOME", ".config")?;
    Some(base.join(tool).join("config.toml"))
}
