//! XDG base-directory resolution, shared by the [`config`](crate::config) and
//! [`cache`](crate::cache) loaders.

use std::path::PathBuf;

/// Resolve an XDG base directory: the value of `$env_var` if it's set and
/// non-empty, otherwise `~/<fallback>` (e.g. `~/.config`, `~/.cache`). Returns
/// `None` only when the env var is unset *and* the home directory can't be
/// determined.
///
/// Deliberately not `dirs::config_dir()` / `dirs::cache_dir()`: on macOS those
/// resolve to `~/Library/...`, but the dotfiles convention is the XDG layout
/// (`~/.config`, `~/.cache`) on every platform.
pub fn base_dir(env_var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var(env_var) {
        Ok(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => Some(dirs::home_dir()?.join(fallback)),
    }
}
