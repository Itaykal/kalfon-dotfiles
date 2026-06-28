//! Color palette and styles. The accent mirrors `vars/fzf.zsh` so the Rust
//! pickers feel like the rest of the dotfiles, with a more modern frame on top.

use ratatui::style::{Color, Modifier, Style};

/// Fuchsia — the signature accent color.
pub const ACCENT: Color = Color::Rgb(0xf0, 0xab, 0xfc);
/// Dark purple selection background.
pub const SEL_BG: Color = Color::Rgb(0x3a, 0x1f, 0x37);
/// Default row foreground.
pub const FG: Color = Color::Rgb(0xc6, 0xc6, 0xc6);
/// Bright foreground for the selected row / live query.
pub const FG_BRIGHT: Color = Color::Rgb(0xff, 0xff, 0xff);
/// Frame/border color — a subtle purple-tinted gray.
pub const BORDER: Color = Color::Rgb(0x4a, 0x40, 0x55);
/// Muted secondary text.
pub const MUTED: Color = Color::Rgb(0x8a, 0x8a, 0x8a);
/// Dim tertiary text.
pub const DIM: Color = Color::Rgb(0x5a, 0x5a, 0x5a);

/// The query prompt (`❯`).
pub fn prompt() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// The live query text the user is typing.
pub fn query() -> Style {
    Style::default().fg(FG_BRIGHT)
}

/// A normal, unselected row.
pub fn row() -> Style {
    Style::default().fg(FG)
}

/// Patched onto the selected row (background only, so per-char match colors
/// survive).
pub fn selected() -> Style {
    Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD)
}

/// The accent bar marking the selected row.
pub fn bar() -> Style {
    Style::default().fg(ACCENT)
}

/// Highlight for fuzzy-matched characters (fzf's `hl`).
pub fn match_hl() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// The rounded border frame.
pub fn border() -> Style {
    Style::default().fg(BORDER)
}

/// The box title (left) — the prompt label.
pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// A key name in the footer hint.
pub fn key() -> Style {
    Style::default().fg(ACCENT)
}

/// Footer / status / counts / scrollbar track.
pub fn footer() -> Style {
    Style::default().fg(DIM)
}

/// Muted helper text.
pub fn muted() -> Style {
    Style::default().fg(MUTED)
}
