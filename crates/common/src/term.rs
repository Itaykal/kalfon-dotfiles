//! Terminal setup with guaranteed restore.
//!
//! ratatui (unlike bubbletea) does not clean up after itself, so a panic or
//! Ctrl-C mid-picker would otherwise leave the shell in raw mode. [`TermGuard`]
//! restores raw mode and the cursor on drop, no matter how we leave the scope.
//!
//! The picker renders *inline* (it reserves a few lines below the prompt rather
//! than taking over the whole screen, like fzf), and draws to stderr so stdout
//! stays free for machine-readable output.

use std::io::{self, Stderr};

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, Clear, ClearType, ScrollUp,
};
use ratatui::layout::Rect;
use ratatui::{backend::CrosstermBackend, Terminal, TerminalOptions, Viewport};

pub type Tui = Terminal<CrosstermBackend<Stderr>>;

/// Owns an inline ratatui terminal and restores cooked mode on drop.
pub struct TermGuard {
    terminal: Tui,
    /// Whether we pushed the keyboard-enhancement flags (so we only pop when we
    /// pushed). Enables disambiguating keys like ctrl-enter from plain enter on
    /// terminals that support the Kitty keyboard protocol.
    kbd_enhanced: bool,
    /// Top row of the bottom-anchored viewport, so `cleanup` can park the cursor
    /// there and let the next shell prompt reclaim the reserved space.
    top: u16,
}

impl TermGuard {
    /// Enter raw mode and create an inline viewport `height` lines tall.
    pub fn inline(height: u16) -> Result<Self> {
        enable_raw_mode()?;
        let kbd_enhanced = supports_keyboard_enhancement().unwrap_or(false);
        if kbd_enhanced {
            let _ = execute!(
                io::stderr(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            );
        }
        // Anchor a fixed viewport at the bottom of the screen (fzf-style).
        // `Viewport::Inline` would make ratatui probe the cursor position via
        // crossterm, which writes its DSR query to *stdout* — but our tools draw
        // to stderr and leave stdout for machine-readable output (often a
        // captured pipe, e.g. `out="$(feature)"`), so the probe never reaches
        // the terminal and init times out with "The cursor position could not be
        // read within a normal duration". A fixed viewport needs no probe;
        // `terminal::size()` reads the tty via ioctl, so it works even when
        // stdout is a pipe.
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let height = height.min(rows.max(1));
        let top = rows.saturating_sub(height);
        // Unlike Inline, a fixed viewport draws at absolute rows and won't
        // reserve space, so it would paint over the shell prompt. Scroll the
        // existing content up by `height` first (Inline does the same once it
        // knows the cursor row) so the bottom rows are blank before we draw.
        let _ = execute!(io::stderr(), ScrollUp(height));
        let backend = CrosstermBackend::new(io::stderr());
        let area = Rect::new(0, top, cols, height);
        // If terminal setup fails after raw mode is on, disable it here: the
        // `Drop` guard only runs once `Self` exists, so an early return would
        // otherwise strand the shell in raw mode.
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )
        .inspect_err(|_| {
            if kbd_enhanced {
                let _ = execute!(io::stderr(), PopKeyboardEnhancementFlags);
            }
            let _ = disable_raw_mode();
        })?;
        Ok(Self {
            terminal,
            kbd_enhanced,
            top,
        })
    }

    pub fn terminal(&mut self) -> &mut Tui {
        &mut self.terminal
    }

    /// Wipe the viewport and park the cursor at its top so the next shell prompt
    /// reclaims the reserved rows cleanly. Call on the normal exit path; the Drop
    /// guard still runs if you don't.
    pub fn cleanup(&mut self) {
        let _ = execute!(
            io::stderr(),
            MoveTo(0, self.top),
            Clear(ClearType::FromCursorDown)
        );
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        if self.kbd_enhanced {
            let _ = execute!(io::stderr(), PopKeyboardEnhancementFlags);
        }
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
    }
}
