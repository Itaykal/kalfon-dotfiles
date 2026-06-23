//! The interactive picker: a two-pane TUI (issue list + Markdown preview) with
//! type-alias filtering, async preview loading, stale-while-revalidate refresh
//! (`ctrl-r`), and inline create (`ctrl-n`).
//!
//! Feature-specific composition lives here; the reusable mechanics come from
//! `common` (theme, term guard, spinner frames, background `Refresh`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nucleo_matcher::{Config as NucleoConfig, Matcher};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::Frame;

use common::refresh::Refresh;
use common::term::TermGuard;
use common::{spinner, theme};

use crate::filter::{filter_issues, parse_query, RowMatch};
use crate::markdown;
use crate::tracker::{kind, CreateRequest, Issue};

/// What the picker returns. A created issue is reported as `Selected` too — the
/// caller branches from it either way.
pub enum Outcome {
    Selected { key: String, summary: String },
    Cancelled,
}

type DescribeFn = Arc<dyn Fn(String) -> Result<String> + Send + Sync>;
type CreateFn = Arc<dyn Fn(CreateRequest) -> Result<String> + Send + Sync>;

/// Run the picker. `items` is the (possibly stale) initial list; when
/// `auto_refresh` is set, a background refresh kicks off immediately (SWR).
#[allow(clippy::too_many_arguments)]
pub fn run(
    aliases: BTreeMap<String, String>,
    items: Vec<Issue>,
    branches: HashSet<String>,
    refresh_job: impl Fn() -> Result<Vec<Issue>> + Send + Sync + 'static,
    describe: impl Fn(String) -> Result<String> + Send + Sync + 'static,
    create: impl Fn(CreateRequest) -> Result<String> + Send + Sync + 'static,
    auto_refresh: bool,
) -> Result<Outcome> {
    let mut app = App::new(
        aliases,
        items,
        branches,
        Refresh::new(refresh_job),
        Arc::new(describe),
        Arc::new(create),
    );

    let mut guard = TermGuard::inline(viewport_height())?;
    if auto_refresh {
        app.refresh.trigger();
    }
    app.ensure_preview();

    while !app.done {
        guard.terminal().draw(|f| app.render(f))?;
        app.poll();
        if app.done {
            break;
        }
        if event::poll(spinner::TICK)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press || k.kind == KeyEventKind::Repeat {
                    app.handle_key(k);
                }
            }
        }
        app.tick = app.tick.wrapping_add(1);
    }

    guard.cleanup();
    drop(guard);
    Ok(app.result)
}

fn viewport_height() -> u16 {
    let rows = crossterm::terminal::size().map(|(_, r)| r).unwrap_or(24);
    let want = (rows as f32 * 0.4) as u16;
    want.clamp(12.min(rows), rows.max(8))
}

struct App {
    aliases: BTreeMap<String, String>,
    /// Local branch names, for marking issues that already have a branch.
    branches: HashSet<String>,
    matcher: Matcher,
    issues: Vec<Issue>,
    query: String,
    /// Cursor position within `query`, as a char index in `0..=query char count`.
    cursor: usize,
    rows: Vec<RowMatch>,
    selected: usize,
    list_state: ListState,

    refresh: Refresh<Vec<Issue>>,

    describe: DescribeFn,
    preview_cache: HashMap<String, Text<'static>>,
    preview_loading: Option<String>,
    preview_rx: Option<Receiver<(String, Result<String>)>>,
    preview_scroll: u16,

    create_fn: CreateFn,
    creating: bool,
    create_summary: String,
    create_rx: Option<Receiver<Result<String>>>,

    help: bool,
    status: String,
    tick: usize,
    done: bool,
    result: Outcome,
}

impl App {
    fn new(
        aliases: BTreeMap<String, String>,
        issues: Vec<Issue>,
        branches: HashSet<String>,
        refresh: Refresh<Vec<Issue>>,
        describe: DescribeFn,
        create_fn: CreateFn,
    ) -> Self {
        let mut app = App {
            aliases,
            branches,
            matcher: Matcher::new(NucleoConfig::DEFAULT),
            issues,
            query: String::new(),
            cursor: 0,
            rows: Vec::new(),
            selected: 0,
            list_state: ListState::default(),
            refresh,
            describe,
            preview_cache: HashMap::new(),
            preview_loading: None,
            preview_rx: None,
            preview_scroll: 0,
            create_fn,
            creating: false,
            create_summary: String::new(),
            create_rx: None,
            help: false,
            status: String::new(),
            tick: 0,
            done: false,
            result: Outcome::Cancelled,
        };
        app.refilter();
        app
    }

    fn current_issue(&self) -> Option<&Issue> {
        self.rows
            .get(self.selected)
            .map(|r| &self.issues[r.issue_idx])
    }

    fn refilter(&mut self) {
        let q = parse_query(&self.query, &self.aliases);
        self.rows = filter_issues(&self.issues, &q, &mut self.matcher);
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    fn on_query_change(&mut self) {
        self.refilter();
        self.selected = 0;
        self.preview_scroll = 0;
        self.ensure_preview();
    }

    fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.preview_scroll = 0;
        self.ensure_preview();
    }

    fn down(&mut self) {
        if !self.rows.is_empty() {
            self.selected = (self.selected + 1).min(self.rows.len() - 1);
        }
        self.preview_scroll = 0;
        self.ensure_preview();
    }

    /// Start loading the selected issue's description if it isn't cached or
    /// already loading.
    fn ensure_preview(&mut self) {
        let Some(key) = self.current_issue().map(|i| i.key.clone()) else {
            return;
        };
        if self.preview_cache.contains_key(&key) || self.preview_loading.as_deref() == Some(&key) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let describe = Arc::clone(&self.describe);
        let k = key.clone();
        std::thread::spawn(move || {
            let md = describe(k.clone());
            let _ = tx.send((k, md));
        });
        self.preview_loading = Some(key);
        self.preview_rx = Some(rx);
    }

    fn poll(&mut self) {
        self.poll_refresh();
        self.poll_preview();
        self.poll_create();
    }

    fn poll_refresh(&mut self) {
        if let Some(res) = self.refresh.poll() {
            match res {
                Ok(items) => {
                    let key = self.current_issue().map(|i| i.key.clone());
                    self.issues = items;
                    self.refilter();
                    if let Some(k) = key {
                        if let Some(pos) = self
                            .rows
                            .iter()
                            .position(|r| self.issues[r.issue_idx].key == k)
                        {
                            self.selected = pos;
                        }
                    }
                    self.status.clear();
                    self.ensure_preview();
                }
                Err(_) => self.status = "refresh failed".into(),
            }
        }
    }

    fn poll_preview(&mut self) {
        let Some(rx) = &self.preview_rx else {
            return;
        };
        match rx.try_recv() {
            Ok((key, res)) => {
                let text = match res {
                    Ok(md) => markdown::render(&md),
                    Err(e) => Text::from(format!("failed to load {key}\n\n{e}")),
                };
                self.preview_cache.insert(key.clone(), text);
                if self.preview_loading.as_deref() == Some(key.as_str()) {
                    self.preview_loading = None;
                }
                self.preview_rx = None;
                self.ensure_preview(); // selection may have moved on
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.preview_rx = None;
                self.preview_loading = None;
            }
        }
    }

    fn poll_create(&mut self) {
        let Some(rx) = &self.create_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(key)) => {
                self.result = Outcome::Selected {
                    key,
                    summary: std::mem::take(&mut self.create_summary),
                };
                self.done = true;
            }
            Ok(Err(e)) => {
                self.status = format!("create failed: {e}");
                self.creating = false;
                self.create_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.status = "create failed".into();
                self.creating = false;
                self.create_rx = None;
            }
        }
    }

    fn start_create(&mut self) {
        let q = parse_query(&self.query, &self.aliases);
        let summary = q.search.trim().to_string();
        if summary.is_empty() {
            self.status = "type a summary in the query first, then ctrl-n".into();
            return;
        }
        let kind_name = if q.active_type.is_empty() {
            kind::TASK.to_string()
        } else {
            q.active_type
        };
        if kind_name == kind::SUBTASK {
            self.status = "cannot create a Sub-task without a parent".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        let create = Arc::clone(&self.create_fn);
        let req = CreateRequest {
            kind: kind_name,
            summary: summary.clone(),
        };
        std::thread::spawn(move || {
            let _ = tx.send(create(req));
        });
        self.creating = true;
        self.create_summary = summary;
        self.create_rx = Some(rx);
        self.status.clear();
    }

    /// Byte offset into `query` for the current `cursor` char index.
    fn cursor_byte(&self) -> usize {
        self.query
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.query.len())
    }

    /// Delete the whole word to the left of the cursor (opt/alt-backspace,
    /// ctrl-w): skip any whitespace, then the run of non-whitespace.
    fn delete_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.query.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        chars.drain(i..self.cursor);
        self.query = chars.into_iter().collect();
        self.cursor = i;
        self.on_query_change();
    }

    /// Delete everything from the start of the line to the cursor (cmd-backspace).
    fn delete_to_start(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.query.chars().collect();
        chars.drain(0..self.cursor);
        self.query = chars.into_iter().collect();
        self.cursor = 0;
        self.on_query_change();
    }

    fn handle_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let sup = k.modifiers.contains(KeyModifiers::SUPER);

        // While a create is in flight, swallow everything but a hard quit.
        if self.creating {
            if ctrl && matches!(k.code, KeyCode::Char('c')) {
                self.done = true;
            }
            return;
        }

        match k.code {
            KeyCode::Esc => {
                if self.help {
                    self.help = false;
                } else {
                    self.done = true;
                }
            }
            KeyCode::Char('c') if ctrl => self.done = true,
            KeyCode::Enter => {
                if let Some(iss) = self.current_issue() {
                    self.result = Outcome::Selected {
                        key: iss.key.clone(),
                        summary: iss.summary.clone(),
                    };
                    self.done = true;
                }
            }
            KeyCode::Up => self.up(),
            KeyCode::Char('k' | 'p') if ctrl => self.up(),
            KeyCode::Down => self.down(),
            KeyCode::Char('j') if ctrl => self.down(),
            KeyCode::Char('n') if ctrl => self.start_create(),
            KeyCode::Char('r') if ctrl => self.refresh.trigger(),
            KeyCode::Char('d') if ctrl => {
                self.preview_scroll = self.preview_scroll.saturating_add(8)
            }
            KeyCode::Char('u') if ctrl => {
                self.preview_scroll = self.preview_scroll.saturating_sub(8)
            }
            KeyCode::Char('f') if ctrl => {
                self.preview_scroll = self.preview_scroll.saturating_add(16)
            }
            KeyCode::Char('b') if ctrl => {
                self.preview_scroll = self.preview_scroll.saturating_sub(16)
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.query.chars().count())
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::End => self.cursor = self.query.chars().count(),
            KeyCode::Char('e') if ctrl => self.cursor = self.query.chars().count(),
            // cmd-backspace: delete to start of line.
            KeyCode::Backspace if sup => self.delete_to_start(),
            // opt/alt-backspace and ctrl-w: delete previous word.
            KeyCode::Backspace if alt => self.delete_word_back(),
            KeyCode::Char('w') if ctrl => self.delete_word_back(),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let b = self.cursor_byte();
                    self.query.remove(b);
                    self.on_query_change();
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.query.chars().count() {
                    let b = self.cursor_byte();
                    self.query.remove(b);
                    self.on_query_change();
                }
            }
            KeyCode::Char('?') if self.query.is_empty() => self.help = !self.help,
            KeyCode::Char(c) if !ctrl && !alt => {
                let b = self.cursor_byte();
                self.query.insert(b, c);
                self.cursor += 1;
                self.help = false;
                self.on_query_change();
            }
            _ => {}
        }
    }

    fn render(&mut self, f: &mut Frame) {
        let [panes, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .areas(panes);
        self.render_left(f, left);
        self.render_right(f, right);
        self.render_footer(f, footer);
    }

    fn render_left(&mut self, f: &mut Frame, area: Rect) {
        let status = if self.refresh.in_flight() {
            Line::from(Span::styled(
                format!(" {} updating… ", spinner::frame(self.tick)),
                theme::muted(),
            ))
        } else {
            Line::from(Span::styled(
                format!(" {}/{} ", self.rows.len(), self.issues.len()),
                theme::footer(),
            ))
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme::border())
            .title_top(Line::from(Span::styled(" feature ", theme::title())))
            .title_top(status.right_aligned());
        let inner = block.inner(area);
        f.render_widget(block, area);

        let [query_area, list_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);

        let qline = Line::from(vec![
            Span::styled("❯ ", theme::prompt()),
            Span::styled(self.query.clone(), theme::query()),
        ]);
        f.render_widget(Paragraph::new(qline), query_area);
        f.set_cursor_position(Position::new(
            query_area.x + 2 + self.cursor as u16,
            query_area.y,
        ));

        let width = list_area.width as usize;
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let iss = &self.issues[row.issue_idx];
                let has_branch = self
                    .branches
                    .contains(&crate::vcs::branch(&iss.key, &iss.summary));
                row_item(i == self.selected, iss, &row.sum_matched, has_branch, width)
            })
            .collect();
        self.list_state.select(Some(self.selected));
        let list = List::new(items).highlight_style(theme::selected());
        f.render_stateful_widget(list, list_area, &mut self.list_state);

        // Scroll affordance: a subtle thumb on the right edge when the list
        // overflows (no track, muted thumb — just enough to show there's more).
        if self.rows.len() > list_area.height as usize {
            let mut sb = ScrollbarState::new(self.rows.len()).position(self.selected);
            let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(None)
                .thumb_symbol("▐")
                .thumb_style(theme::muted());
            f.render_stateful_widget(bar, list_area, &mut sb);
        }
    }

    fn render_right(&mut self, f: &mut Frame, area: Rect) {
        let label = if self.help { " help " } else { " issue " };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme::border())
            .title_top(Line::from(Span::styled(label, theme::title())));
        let inner = block.inner(area);
        f.render_widget(block, area);

        // Body fills the pane; a one-line meta footer shows the issue's
        // type/status/assignee (like the Go tool), hidden in help mode.
        let [body, meta] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

        let text = if self.help {
            help_text()
        } else {
            match self.current_issue() {
                Some(iss) => self
                    .preview_cache
                    .get(&iss.key)
                    .cloned()
                    .unwrap_or_else(|| Text::from(Line::styled("loading…", theme::muted()))),
                None => Text::from(Line::styled("no matching issues", theme::muted())),
            }
        };
        let total = wrapped_height(&text, body.width);
        // Clamp scroll so it can't run past the end into blank space.
        let max_scroll = total.saturating_sub(body.height as usize) as u16;
        self.preview_scroll = self.preview_scroll.min(max_scroll);
        let para = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((self.preview_scroll, 0));
        f.render_widget(para, body);

        // Same subtle scroll thumb as the list, when the preview overflows.
        if total > body.height as usize {
            let mut sb = ScrollbarState::new(total).position(self.preview_scroll as usize);
            let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(None)
                .thumb_symbol("▐")
                .thumb_style(theme::muted());
            f.render_stateful_widget(bar, body, &mut sb);
        }

        if !self.help {
            if let Some(iss) = self.current_issue() {
                let mut parts = vec![iss.kind.clone(), iss.status.clone()];
                if !iss.assignee.is_empty() {
                    parts.push(iss.assignee.clone());
                }
                let line = Line::from(Span::styled(parts.join("  •  "), theme::muted()));
                f.render_widget(Paragraph::new(line), meta);
            }
        }
    }

    fn render_footer(&self, f: &mut Frame, area: Rect) {
        let line = if self.creating {
            Line::from(vec![
                Span::styled(format!("{} ", spinner::frame(self.tick)), theme::prompt()),
                Span::styled("creating issue…", theme::muted()),
            ])
        } else if !self.status.is_empty() {
            Line::from(Span::styled(format!(" {}", self.status), theme::muted()))
        } else {
            Line::from(vec![
                Span::styled(" ↑↓", theme::key()),
                Span::styled(" move  ", theme::footer()),
                Span::styled("⏎", theme::key()),
                Span::styled(" branch  ", theme::footer()),
                Span::styled("^n", theme::key()),
                Span::styled(" new  ", theme::footer()),
                Span::styled("^r", theme::key()),
                Span::styled(" refresh  ", theme::footer()),
                Span::styled("/b /t /s", theme::key()),
                Span::styled(" type  ", theme::footer()),
                Span::styled("?", theme::key()),
                Span::styled(" help  ", theme::footer()),
                Span::styled("esc", theme::key()),
                Span::styled(" quit", theme::footer()),
            ])
        };
        f.render_widget(Paragraph::new(line), area);
    }
}

/// Approximate the number of visual rows `text` occupies once wrapped to
/// `width`. Used only to drive the preview scroll thumb, so a char-width
/// estimate (rather than exact word-wrap) is good enough.
fn wrapped_height(text: &Text, width: u16) -> usize {
    let w = width as usize;
    if w == 0 {
        return text.lines.len().max(1);
    }
    text.lines
        .iter()
        .map(|line| {
            let lw = line.width();
            if lw == 0 { 1 } else { lw.div_ceil(w) }
        })
        .sum::<usize>()
        .max(1)
}

/// One issue row: marker, branch indicator, type, key, highlighted summary, status.
fn row_item(
    selected: bool,
    iss: &Issue,
    matched: &[usize],
    has_branch: bool,
    width: usize,
) -> ListItem<'static> {
    let mut spans = vec![if selected {
        Span::styled("▌ ", theme::bar())
    } else {
        Span::raw("  ")
    }];
    // A branch glyph marks issues that already have a local branch (not "fresh").
    spans.push(if has_branch {
        Span::styled("⎇ ", theme::muted())
    } else {
        Span::raw("  ")
    });
    spans.push(Span::styled(col(&iss.kind, 9), theme::muted()));
    spans.push(Span::styled(col(&iss.key, 11), theme::key()));

    // Fixed columns: marker(2) + branch(2) + kind(9+1) + key(11+1) = 26. The
    // summary is padded to a fixed width so the status column always lines up,
    // and status gets a fixed tail (" " + 14) when there's room.
    const PREFIX: usize = 2 + 2 + 10 + 12;
    const STATUS_TAIL: usize = 1 + 14;
    let show_status = width >= PREFIX + 6 + STATUS_TAIL;
    let sum_w = width
        .saturating_sub(PREFIX + if show_status { STATUS_TAIL } else { 0 })
        .max(6);
    spans.extend(highlight(&iss.summary, matched, sum_w));
    if show_status {
        let st: String = iss.status.chars().take(14).collect();
        spans.push(Span::styled(format!(" {st}"), theme::footer()));
    }
    ListItem::new(Line::from(spans))
}

/// Truncate to `w` chars (no ellipsis) and right-pad to width `w`.
fn col(s: &str, w: usize) -> String {
    let mut t: String = s.chars().take(w).collect();
    let len = t.chars().count();
    if len < w {
        t.push_str(&" ".repeat(w - len));
    }
    t.push(' ');
    t
}

/// Summary spans, accenting fuzzy-matched chars, truncated to `max_w` with `…`
/// and **padded** to exactly `max_w` columns so the next column lines up.
fn highlight(summary: &str, matched: &[usize], max_w: usize) -> Vec<Span<'static>> {
    let chars: Vec<char> = summary.chars().collect();
    let truncated = chars.len() > max_w;
    let take = if truncated {
        max_w.saturating_sub(1)
    } else {
        chars.len()
    };
    let mut spans: Vec<Span<'static>> = chars
        .iter()
        .take(take)
        .enumerate()
        .map(|(i, &c)| {
            let style = if matched.binary_search(&i).is_ok() {
                theme::match_hl()
            } else {
                theme::row()
            };
            Span::styled(c.to_string(), style)
        })
        .collect();
    let shown = if truncated {
        spans.push(Span::styled("…", theme::row()));
        take + 1
    } else {
        take
    };
    if shown < max_w {
        spans.push(Span::raw(" ".repeat(max_w - shown)));
    }
    spans
}

fn help_text() -> Text<'static> {
    let kv = |k: &str, v: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {k:<16}"), theme::key()),
            Span::styled(v.to_string(), theme::muted()),
        ])
    };
    Text::from(vec![
        Line::styled(
            "Pick an issue and branch from it, or create one with ctrl-n.",
            theme::muted(),
        ),
        Line::from(""),
        Line::styled("navigate", theme::title()),
        kv("↑ / ctrl-k", "up"),
        kv("↓ / ctrl-j", "down"),
        kv("enter", "branch from issue"),
        kv("ctrl-n", "create issue from query"),
        kv("ctrl-r", "refresh list"),
        kv("esc / ctrl-c", "quit"),
        Line::from(""),
        Line::styled("preview", theme::title()),
        kv("ctrl-d / ctrl-u", "scroll half page"),
        kv("ctrl-f / ctrl-b", "scroll page"),
        Line::from(""),
        Line::styled("type filter", theme::title()),
        kv("/b /bug", "Bug"),
        kv("/t /task", "Task"),
        kv("/s /story", "Story"),
        kv("/st /sub", "Sub-task"),
        Line::from(""),
        kv("?", "toggle this help"),
    ])
}
