//! Shared building blocks for the dotfiles Rust CLIs.
//!
//! The complex shell tools in this repo are migrating to standalone Rust
//! binaries. This crate holds the pieces every tool wants: the fuchsia color
//! [`theme`] that matches the rest of the dotfiles, a generic fuzzy [`picker`]
//! widget, an XDG-aware [`config`] loader, and a [`term`] guard that always
//! restores the terminal on exit.

pub mod cache;
pub mod config;
pub mod picker;
pub mod refresh;
pub mod spinner;
pub mod term;
pub mod theme;
pub mod xdg;

pub use picker::select;
