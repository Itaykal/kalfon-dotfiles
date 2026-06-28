//! A generic, fuzzy single-select picker rendered inline (fzf-style).
//!
//! [`select`] takes any list plus a label closure, shows a fuzzy-filterable
//! list inside a rounded frame with match highlighting in the dotfiles fuchsia
//! [`theme`], and returns the chosen item (or `None` on Esc/Ctrl-C).

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState,
};

use crate::term::TermGuard;
use crate::theme;

/// Fraction of the terminal height the picker may grow to (like fzf --height).
const HEIGHT_FRACTION: f32 = 0.45;
/// Border (2) + query line (1) + footer (1).
const CHROME: u16 = 4;

/// One item that survived the current query, with its matched char positions.
struct Match {
    item: usize,
    indices: Vec<u32>,
}

/// Show the picker. `title` labels the box (e.g. `"AWS account"`). Returns the
/// chosen item, or `None` on cancel. Auto-selecting single-element lists is the
/// caller's choice, not the picker's — this always prompts.
pub fn select<T>(
    title: &str,
    mut items: Vec<T>,
    label: impl Fn(&T) -> String,
) -> Result<Option<T>> {
    if items.is_empty() {
        return Ok(None);
    }

    let labels: Vec<String> = items.iter().map(&label).collect();
    let height = viewport_height(labels.len());

    let mut picker = Picker::new(&labels);
    let mut guard = TermGuard::inline(height)?;

    let chosen: Option<usize> = loop {
        guard.terminal().draw(|frame| picker.render(frame, title))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            KeyCode::Esc => break None,
            KeyCode::Char('c') if ctrl => break None,
            KeyCode::Enter => match picker.current() {
                Some(item) => break Some(item),
                None => continue,
            },
            KeyCode::Up => picker.up(),
            KeyCode::Char('p') | KeyCode::Char('k') if ctrl => picker.up(),
            KeyCode::Down => picker.down(),
            KeyCode::Char('n') | KeyCode::Char('j') if ctrl => picker.down(),
            KeyCode::Backspace => picker.delete_char(),
            KeyCode::Char('u') if ctrl => picker.clear_query(),
            KeyCode::Char('w') if ctrl => picker.trim_word(),
            KeyCode::Char(c) if !ctrl && !alt => picker.push_char(c),
            _ => {}
        }
    };

    guard.cleanup();
    drop(guard);

    Ok(chosen.map(|i| items.swap_remove(i)))
}

/// Live state of one picker session: the query the user is typing, the matches
/// that survive it, and the cursor. Borrows the immutable `labels` it filters.
///
/// Key handling in [`select`] stays a flat dispatch table; the mutation logic
/// (and the one rule that *any* query edit recomputes matches and resets the
/// cursor) lives here as named methods, so a new ratatui tool can drive the
/// same widget without re-threading state through a pile of arguments.
struct Picker<'a> {
    matcher: Matcher,
    labels: &'a [String],
    query: String,
    matches: Vec<Match>,
    selected: usize,
    list_state: ListState,
}

impl<'a> Picker<'a> {
    fn new(labels: &'a [String]) -> Self {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let matches = compute(&mut matcher, "", labels);
        Self {
            matcher,
            labels,
            query: String::new(),
            matches,
            selected: 0,
            list_state: ListState::default(),
        }
    }

    /// The item index under the cursor, or `None` when the query matched nothing.
    fn current(&self) -> Option<usize> {
        self.matches.get(self.selected).map(|m| m.item)
    }

    /// Re-score against the current query and snap the cursor back to the top.
    /// Every query edit goes through here so the cursor can't point past the
    /// new match list.
    fn on_query_change(&mut self) {
        self.matches = compute(&mut self.matcher, &self.query, self.labels);
        self.selected = 0;
    }

    fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn down(&mut self) {
        self.selected = next(self.selected, self.matches.len());
    }

    fn delete_char(&mut self) {
        self.query.pop();
        self.on_query_change();
    }

    fn clear_query(&mut self) {
        self.query.clear();
        self.on_query_change();
    }

    fn trim_word(&mut self) {
        trim_word(&mut self.query);
        self.on_query_change();
    }

    fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.on_query_change();
    }
}

/// Grow the box to fit the content, but cap it at a fraction of the screen so
/// it has a stable size and scrolls when there are many items.
fn viewport_height(item_count: usize) -> u16 {
    let term_rows = crossterm::terminal::size().map(|(_, r)| r).unwrap_or(24);
    let cap =
        ((term_rows as f32 * HEIGHT_FRACTION) as u16).clamp(CHROME + 3, term_rows.max(CHROME + 3));
    (item_count as u16 + CHROME).clamp(CHROME + 1, cap)
}

impl Picker<'_> {
    fn render(&mut self, frame: &mut ratatui::Frame, title: &str) {
        self.list_state.select(Some(self.selected));

        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme::border())
            .title_top(Line::from(Span::styled(
                format!(" {title} "),
                theme::title(),
            )))
            .title_top(
                Line::from(Span::styled(
                    format!(" {}/{} ", self.matches.len(), self.labels.len()),
                    theme::footer(),
                ))
                .right_aligned(),
            );
        let inner = block.inner(frame.area());
        frame.render_widget(block, frame.area());

        let [query_area, list_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        // Query line: accent ❯ + the live query, with the cursor at the end.
        let query_line = Line::from(vec![
            Span::styled("❯ ", theme::prompt()),
            Span::styled(self.query.clone(), theme::query()),
        ]);
        frame.render_widget(Paragraph::new(query_line), query_area);
        frame.set_cursor_position(Position::new(
            query_area.x + 2 + self.query.chars().count() as u16,
            query_area.y,
        ));

        // Rows: an accent bar marks the selection; matched chars are highlighted.
        let rows: Vec<ListItem> = self
            .matches
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let mut spans = vec![if i == self.selected {
                    Span::styled("▌ ", theme::bar())
                } else {
                    Span::raw("  ")
                }];
                spans.extend(styled_label(&self.labels[m.item], &m.indices));
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(rows).highlight_style(theme::selected());
        frame.render_stateful_widget(list, list_area, &mut self.list_state);

        // Scrollbar only when the list overflows its area.
        if self.matches.len() > list_area.height as usize {
            let mut sb_state = ScrollbarState::new(self.matches.len()).position(self.selected);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(theme::prompt())
                .track_style(theme::footer());
            frame.render_stateful_widget(scrollbar, list_area, &mut sb_state);
        }

        let footer = Line::from(vec![
            Span::styled("↑↓", theme::key()),
            Span::styled(" move   ", theme::footer()),
            Span::styled("⏎", theme::key()),
            Span::styled(" select   ", theme::footer()),
            Span::styled("esc", theme::key()),
            Span::styled(" cancel", theme::footer()),
        ]);
        frame.render_widget(Paragraph::new(footer), footer_area);
    }
}

fn next(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (selected + 1).min(len - 1)
    }
}

fn trim_word(query: &mut String) {
    let trimmed = query.trim_end_matches(' ');
    match trimmed.rfind(' ') {
        Some(i) => query.truncate(i + 1),
        None => query.clear(),
    }
}

/// Score every label against `query` and return survivors, best first.
fn compute(matcher: &mut Matcher, query: &str, labels: &[String]) -> Vec<Match> {
    if query.is_empty() {
        return labels
            .iter()
            .enumerate()
            .map(|(item, _)| Match {
                item,
                indices: Vec::new(),
            })
            .collect();
    }

    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut scored: Vec<(u32, Match)> = Vec::new();
    let mut buf = Vec::new();
    let mut indices = Vec::new();
    for (item, label) in labels.iter().enumerate() {
        indices.clear();
        let haystack = Utf32Str::new(label, &mut buf);
        if let Some(score) = pattern.indices(haystack, matcher, &mut indices) {
            let mut idx = indices.clone();
            idx.sort_unstable();
            scored.push((score, Match { item, indices: idx }));
        }
    }
    // Stable sort by score, highest first (ties keep input order).
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, m)| m).collect()
}

/// Build a row, accenting the fuzzy-matched characters.
fn styled_label(label: &str, indices: &[u32]) -> Vec<Span<'static>> {
    label
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let style = if indices.binary_search(&(i as u32)).is_ok() {
                theme::match_hl()
            } else {
                theme::row()
            };
            Span::styled(ch.to_string(), style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_keeps_all_in_order() {
        let mut m = Matcher::new(Config::DEFAULT);
        let labels = vec!["alpha".to_string(), "beta".to_string()];
        let got = compute(&mut m, "", &labels);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].item, 0);
        assert_eq!(got[1].item, 1);
    }

    #[test]
    fn fuzzy_filters_and_ranks() {
        let mut m = Matcher::new(Config::DEFAULT);
        let labels = vec![
            "production".to_string(),
            "staging".to_string(),
            "prod-sandbox".to_string(),
        ];
        let got = compute(&mut m, "prod", &labels);
        let items: Vec<usize> = got.iter().map(|x| x.item).collect();
        assert!(items.contains(&0));
        assert!(items.contains(&2));
        assert!(!items.contains(&1)); // "staging" has no fuzzy match for "prod"
    }

    #[test]
    fn match_indices_are_sorted() {
        let mut m = Matcher::new(Config::DEFAULT);
        let labels = vec!["abcdef".to_string()];
        let got = compute(&mut m, "ace", &labels);
        assert_eq!(got.len(), 1);
        let idx = &got[0].indices;
        assert!(idx.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn trim_word_removes_last_token() {
        let mut q = String::from("foo bar");
        trim_word(&mut q);
        assert_eq!(q, "foo ");
        trim_word(&mut q);
        assert_eq!(q, "");
    }

    #[test]
    fn viewport_caps_and_grows() {
        // Few items: sized to content (items + chrome).
        assert_eq!(viewport_height(3), 3 + CHROME);
        // Many items: capped well under a huge count.
        assert!(viewport_height(1000) < 1000);
    }
}
