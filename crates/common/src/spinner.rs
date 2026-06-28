//! Run a blocking task while showing an animated spinner, so slow work (a
//! network call before a picker) feels responsive instead of frozen.
//!
//! The task runs on a scoped background thread; the main thread animates an
//! inline spinner line until it finishes, then returns the task's result.

use std::time::Duration;

use anyhow::{anyhow, Result};
use crossterm::event;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::term::TermGuard;
use crate::theme;

/// Braille spinner frames.
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Frame duration — ~12 fps. Reused by TUIs that animate their own indicators.
pub const TICK: Duration = Duration::from_millis(80);

/// The spinner glyph for animation step `tick`, for tools driving their own
/// loop (e.g. an inline "updating…" indicator in a picker).
pub fn frame(tick: usize) -> &'static str {
    FRAMES[tick % FRAMES.len()]
}

/// Run `task` on a background thread, animating a one-line spinner with
/// `message` until it completes, then return its result. Fast tasks finish
/// before the first frame, so there's no flash for instant work.
pub fn run<T, F>(message: &str, task: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send,
    T: Send,
{
    std::thread::scope(|scope| {
        let handle = scope.spawn(task);

        let mut guard = TermGuard::inline(1)?;
        let mut frame = 0usize;
        while !handle.is_finished() {
            let tick = FRAMES[frame % FRAMES.len()];
            guard.terminal().draw(|f| {
                let line = Line::from(vec![
                    Span::styled(format!("{tick} "), theme::prompt()),
                    Span::styled(message.to_string(), theme::muted()),
                ]);
                f.render_widget(Paragraph::new(line), f.area());
            })?;
            // Poll (instead of sleep) so the loop wakes promptly when the task
            // finishes; drain any keypresses so they don't leak into the picker.
            if event::poll(TICK)? {
                let _ = event::read();
            }
            frame += 1;
        }
        guard.cleanup();
        drop(guard);

        match handle.join() {
            Ok(result) => result,
            Err(_) => Err(anyhow!("background task panicked")),
        }
    })
}
