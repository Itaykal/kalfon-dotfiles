//! Render Markdown to themed `ratatui::Text` for the preview pane.
//!
//! Replaces the Go tool's glamour dependency. This is intentionally the only
//! place that knows how Markdown looks on screen — swapping the renderer is a
//! localized change. Output is meant to be shown in a `Paragraph` with wrapping.

use common::theme;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

/// Near-black text for the H1 fuchsia bar (mirrors the Go tool's glamour H1).
const INK: Color = Color::Rgb(0x1c, 0x14, 0x20);

/// Convert Markdown into styled, wrappable terminal text.
pub fn render(markdown: &str) -> Text<'static> {
    let mut r = Renderer::new();
    let parser = Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH);
    for ev in parser {
        r.event(ev);
    }
    r.finish()
}

struct Renderer {
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    /// Inline style stack; the top applies to text spans.
    styles: Vec<Style>,
    /// One slot per open list; `Some(n)` = ordered (next number), `None` = bullet.
    lists: Vec<Option<u64>>,
    in_code_block: bool,
}

impl Renderer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            cur: Vec::new(),
            styles: vec![theme::row()],
            lists: Vec::new(),
            in_code_block: false,
        }
    }

    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&Style::new())
    }

    fn push(&mut self, s: &str, style: Style) {
        self.cur.push(Span::styled(s.to_string(), style));
    }

    fn flush(&mut self) {
        if !self.cur.is_empty() {
            self.lines.push(Line::from(std::mem::take(&mut self.cur)));
        }
    }

    /// Add a blank separator line, collapsing consecutive blanks and leading ones.
    fn blank(&mut self) {
        match self.lines.last() {
            None => {}
            Some(l) if l.spans.is_empty() => {}
            Some(_) => self.lines.push(Line::from("")),
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if self.in_code_block {
                    for (i, part) in t.split('\n').enumerate() {
                        if i > 0 {
                            self.flush();
                        }
                        self.push(part, theme::muted());
                    }
                } else {
                    let style = self.style();
                    self.push(&t, style);
                }
            }
            Event::Code(t) => {
                let style = Style::default().fg(theme::ACCENT);
                self.push(&t, style);
            }
            Event::SoftBreak => {
                let style = self.style();
                self.push(" ", style);
            }
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.blank();
                self.lines
                    .push(Line::styled("────────────────", theme::footer()));
                self.blank();
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush();
                self.blank();
                // H1 is a filled fuchsia bar with dark text (like the title);
                // deeper headings are plain accent.
                let style = if level == HeadingLevel::H1 {
                    Style::default()
                        .bg(theme::ACCENT)
                        .fg(INK)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD)
                };
                self.styles.push(style);
                // Leading pad so the H1 bar reads as a band.
                if style.bg.is_some() {
                    self.push(" ", style);
                }
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.blank();
                self.in_code_block = true;
            }
            Tag::List(start) => self.lists.push(start),
            Tag::Item => {
                self.flush();
                let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{indent}{n}. ");
                        *n += 1;
                        m
                    }
                    _ => format!("{indent}• "),
                };
                self.push(&marker, theme::muted());
            }
            Tag::Emphasis => self
                .styles
                .push(self.style().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.styles.push(self.style().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .styles
                .push(self.style().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.styles.push(
                self.style()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                self.blank();
            }
            TagEnd::Heading(_) => {
                // Trailing pad to close the H1 bar before dropping its style.
                if let Some(style) = self.styles.last() {
                    if style.bg.is_some() {
                        let s = *style;
                        self.push(" ", s);
                    }
                }
                self.styles.pop();
                self.flush();
                self.blank();
            }
            TagEnd::CodeBlock => {
                self.flush();
                self.in_code_block = false;
                self.blank();
            }
            TagEnd::List(_) => {
                self.lists.pop();
                self.blank();
            }
            TagEnd::Item => self.flush(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.styles.pop();
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Text<'static> {
        self.flush();
        while matches!(self.lines.last(), Some(l) if l.spans.is_empty()) {
            self.lines.pop();
        }
        Text::from(self.lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_without_panicking() {
        let text = render("# Title\n\nSome **bold** and `code`.\n\n- one\n- two\n");
        // Heading + paragraph + two list items, blanks collapsed.
        assert!(text.lines.len() >= 4);
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(render("").lines.len(), 0);
    }
}
