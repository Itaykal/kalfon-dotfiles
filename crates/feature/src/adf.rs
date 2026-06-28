//! Atlassian Document Format (Jira's rich description tree) → Markdown.
//!
//! Ported from the Go tool's `adf.go`: walk the `fields.description` JSON and
//! flatten it to Markdown so the preview renderer can style it. A `null`
//! description yields the empty string (callers substitute a placeholder).

use serde_json::{Map, Value};

/// Flatten an ADF node to Markdown, trimming trailing newlines.
pub fn adf_to_markdown(node: &Value) -> String {
    walk(node).trim_end_matches('\n').to_string()
}

fn walk(node: &Value) -> String {
    let Some(obj) = node.as_object() else {
        return String::new();
    };
    match obj.get("type").and_then(Value::as_str).unwrap_or("") {
        "text" => apply_marks(text_of(obj), obj.get("marks")),
        "hardBreak" => "\n".to_string(),
        "paragraph" => format!("{}\n\n", children(obj)),
        "heading" => {
            let level = obj
                .get("attrs")
                .and_then(|a| a.get("level"))
                .and_then(Value::as_u64)
                .filter(|l| (1..=6).contains(l))
                .unwrap_or(1) as usize;
            format!("{} {}\n\n", "#".repeat(level), children(obj))
        }
        "bulletList" => list(obj, |_| "- ".to_string()),
        "orderedList" => list(obj, |i| format!("{}. ", i + 1)),
        // List items hold block content; trim block spacing to one line.
        "listItem" => children(obj).trim().to_string(),
        "codeBlock" => format!("```\n{}\n```\n\n", children(obj).trim_end_matches('\n')),
        "blockquote" => format!("> {}\n\n", children(obj).trim()),
        "inlineCard" => attr(obj, "url").to_string(),
        "mention" => {
            let t = attr(obj, "text");
            if t.is_empty() {
                "@?".to_string()
            } else {
                t.to_string()
            }
        }
        "rule" => "\n---\n\n".to_string(),
        // doc and any unknown node: descend into children.
        _ => children(obj),
    }
}

fn list(obj: &Map<String, Value>, marker: impl Fn(usize) -> String) -> String {
    let mut out = String::new();
    if let Some(content) = obj.get("content").and_then(Value::as_array) {
        for (i, item) in content.iter().enumerate() {
            out.push_str(&marker(i));
            out.push_str(&walk(item));
            out.push('\n');
        }
    }
    out.push('\n');
    out
}

fn children(obj: &Map<String, Value>) -> String {
    obj.get("content")
        .and_then(Value::as_array)
        .map(|content| content.iter().map(walk).collect())
        .unwrap_or_default()
}

fn apply_marks(text: &str, marks: Option<&Value>) -> String {
    let Some(marks) = marks.and_then(Value::as_array) else {
        return text.to_string();
    };
    if text.is_empty() {
        return text.to_string();
    }
    let mut s = text.to_string();
    for m in marks {
        let Some(mark) = m.as_object() else { continue };
        match mark.get("type").and_then(Value::as_str).unwrap_or("") {
            "strong" => s = format!("**{s}**"),
            "em" => s = format!("*{s}*"),
            "code" => s = format!("`{s}`"),
            "strike" => s = format!("~~{s}~~"),
            "link" => {
                let href = mark
                    .get("attrs")
                    .and_then(|a| a.get("href"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !href.is_empty() {
                    s = format!("[{s}]({href})");
                }
            }
            _ => {}
        }
    }
    s
}

fn text_of(obj: &Map<String, Value>) -> &str {
    obj.get("text").and_then(Value::as_str).unwrap_or("")
}

fn attr<'a>(obj: &'a Map<String, Value>, key: &str) -> &'a str {
    obj.get("attrs")
        .and_then(|a| a.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(adf: &str) -> String {
        let v: Value = serde_json::from_str(adf).unwrap();
        adf_to_markdown(&v)
    }

    #[test]
    fn cases() {
        assert_eq!(md("null"), "");
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Hello world"}]}]}"#
            ),
            "Hello world"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Goals"}]},{"type":"paragraph","content":[{"type":"text","text":"Body."}]}]}"#
            ),
            "## Goals\n\nBody."
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}"#
            ),
            "- one\n- two"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"orderedList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}"#
            ),
            "1. first\n2. second"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"bold","marks":[{"type":"strong"}]},{"type":"text","text":" and "},{"type":"text","text":"code","marks":[{"type":"code"}]}]}]}"#
            ),
            "**bold** and `code`"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"site","marks":[{"type":"link","attrs":{"href":"https://example.com"}}]}]}]}"#
            ),
            "[site](https://example.com)"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"codeBlock","content":[{"type":"text","text":"go build ./..."}]}]}"#
            ),
            "```\ngo build ./...\n```"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]},{"type":"rule"},{"type":"paragraph","content":[{"type":"text","text":"b"}]}]}"#
            ),
            "a\n\n\n---\n\nb"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"mention","attrs":{"text":"@Itay"}},{"type":"text","text":" see "},{"type":"inlineCard","attrs":{"url":"https://jira/X-1"}}]}]}"#
            ),
            "@Itay see https://jira/X-1"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"line1"},{"type":"hardBreak"},{"type":"text","text":"line2"}]}]}"#
            ),
            "line1\nline2"
        );
        assert_eq!(
            md(
                r#"{"type":"doc","content":[{"type":"panel","content":[{"type":"paragraph","content":[{"type":"text","text":"inside panel"}]}]}]}"#
            ),
            "inside panel"
        );
    }
}
