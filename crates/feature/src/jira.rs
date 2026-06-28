//! `Tracker` backed by the `jira` CLI (ankitpokhrel/jira-cli). The only module
//! that knows Jira exists; everything else talks to `tracker::Tracker`.

use std::process::{Command, Stdio};
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::adf::adf_to_markdown;
use crate::config::Config;
use crate::tracker::{kind, CreateRequest, Issue, Tracker};

const COLUMNS: &str = "TYPE,KEY,SUMMARY,STATUS,ASSIGNEE";
const DELIM: &str = "|";

pub struct Jira {
    cfg: Config,
    /// Current-user account, resolved lazily via `jira me` and cached.
    me: Mutex<Option<String>>,
}

impl Jira {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            me: Mutex::new(None),
        }
    }

    /// The account to list/create for: configured `assignee`, else `jira me`.
    fn assignee(&self) -> Result<String> {
        if !self.cfg.assignee.is_empty() {
            return Ok(self.cfg.assignee.clone());
        }
        if let Some(me) = self.me.lock().unwrap().clone() {
            return Ok(me);
        }
        let out = self.run(false, &["me"])?;
        if !out.success {
            bail!(
                "could not resolve current user via `jira me` (is the jira CLI set up? run `jira init`):\n{}",
                out.combined()
            );
        }
        let me = out.stdout.trim().to_string();
        *self.me.lock().unwrap() = Some(me.clone());
        Ok(me)
    }

    /// Run the jira CLI. `Err` only on spawn failure; a non-zero exit is
    /// reported via `Output::success` so callers can include the output.
    fn run(&self, devnull_stdin: bool, args: &[&str]) -> Result<Output> {
        let bin = if self.cfg.jira_bin.is_empty() {
            "jira"
        } else {
            &self.cfg.jira_bin
        };
        let stdin = if devnull_stdin {
            Stdio::null()
        } else {
            Stdio::inherit()
        };
        let out = Command::new(bin)
            .args(args)
            .stdin(stdin)
            .output()
            .with_context(|| format!("running `{bin} {}`", args.join(" ")))?;
        Ok(Output {
            stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
            success: out.status.success(),
        })
    }
}

struct Output {
    stdout: String,
    stderr: String,
    success: bool,
}

impl Output {
    fn combined(&self) -> String {
        match (self.stdout.is_empty(), self.stderr.is_empty()) {
            (true, _) => self.stderr.clone(),
            (_, true) => self.stdout.clone(),
            _ => format!("{}\n{}", self.stdout, self.stderr),
        }
    }
}

impl Tracker for Jira {
    fn list(&self) -> Result<Vec<Issue>> {
        let assignee = self.assignee()?;
        let mut args: Vec<String> = vec!["issue".into(), "list".into()];
        for s in &self.cfg.list.exclude_statuses {
            args.push(format!("-s~{s}"));
        }
        for t in &self.cfg.list.exclude_types {
            args.push(format!("-t~{t}"));
        }
        if !assignee.is_empty() {
            args.push("-a".into());
            args.push(assignee);
        }
        for a in ["--plain", "--columns", COLUMNS, "--delimiter", DELIM] {
            args.push(a.into());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = self.run(false, &refs)?;
        if !out.success {
            bail!("`jira issue list` failed:\n{}", out.combined());
        }
        Ok(parse_list(&out.stdout))
    }

    fn describe(&self, key: &str) -> Result<String> {
        let out = self.run(false, &["issue", "view", key, "--raw"])?;
        if !out.success {
            bail!("`jira issue view {key}` failed:\n{}", out.combined());
        }
        render_issue(&out.stdout)
    }

    fn create(&self, req: &CreateRequest) -> Result<String> {
        if req.kind == kind::SUBTASK {
            bail!("cannot create a Sub-task without a parent");
        }
        let summary = req.summary.trim();
        if summary.is_empty() {
            bail!("empty summary");
        }
        let typ = if req.kind.is_empty() {
            kind::TASK
        } else {
            req.kind.as_str()
        };
        let assignee = self.assignee()?;

        let mut args: Vec<String> = vec![
            "issue".into(),
            "create".into(),
            "--no-input".into(),
            "-t".into(),
            typ.into(),
            "-s".into(),
            summary.into(),
        ];
        if !assignee.is_empty() {
            args.push("-a".into());
            args.push(assignee);
        }
        // BTreeMap iterates sorted, giving deterministic --custom order.
        for (k, v) in &self.cfg.create.custom {
            args.push("--custom".into());
            args.push(format!("{k}={v}"));
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = self.run(true, &refs)?;
        if !out.success {
            bail!("`jira issue create` failed:\n{}", out.combined());
        }
        let key = scan_key(&out.combined()).ok_or_else(|| {
            anyhow!(
                "could not parse issue key from create output:\n{}",
                out.combined()
            )
        })?;

        // Best-effort transition (a created issue is usable even if move fails).
        if !self.cfg.create.move_to.is_empty() {
            let _ = self.run(true, &["issue", "move", &key, &self.cfg.create.move_to]);
        }
        Ok(key)
    }
}

/// Parse the `--plain --delimiter "|"` table (header row skipped). Summaries
/// are kept whole; display truncation is the TUI's job.
fn parse_list(raw: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (i, line) in raw.split('\n').enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(DELIM).map(str::trim).collect();
        if cols.len() < 4 {
            continue;
        }
        if i == 0 && cols[0] == "TYPE" {
            continue;
        }
        issues.push(Issue {
            kind: cols[0].to_string(),
            key: cols[1].to_string(),
            summary: cols[2].to_string(),
            status: cols[3].to_string(),
            assignee: cols.get(4).map(|s| s.to_string()).unwrap_or_default(),
        });
    }
    issues
}

#[derive(Deserialize)]
struct RawIssue {
    fields: RawFields,
}

#[derive(Deserialize)]
struct RawFields {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: Value,
}

/// Build the Markdown preview: the summary as H1, then the ADF description
/// (or a placeholder).
fn render_issue(raw_json: &str) -> Result<String> {
    let iss: RawIssue = serde_json::from_str(raw_json).context("decode issue JSON")?;
    let body = if iss.fields.description.is_null() {
        "_No description_".to_string()
    } else {
        let md = normalize_wiki(&adf_to_markdown(&iss.fields.description));
        if md.is_empty() {
            "_No description_".to_string()
        } else {
            md
        }
    };
    Ok(format!("# {}\n\n{}", iss.fields.summary, body))
}

/// Many Jira descriptions are authored in wiki markup that lands in ADF as
/// literal text (e.g. a paragraph reading "h3. Impact"). Convert the common
/// leading `hN.` heading marker to a Markdown heading so it renders as one.
fn normalize_wiki(md: &str) -> String {
    md.lines()
        .map(|line| match wiki_heading(line) {
            Some((level, text)) => format!("{} {}", "#".repeat(level), text),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// If `line` is `hN. <text>` (N in 1..=6), return `(N, text)`.
fn wiki_heading(line: &str) -> Option<(usize, &str)> {
    let rest = line.strip_prefix('h')?;
    let digit = rest.chars().next()?;
    let level = digit.to_digit(10).filter(|n| (1..=6).contains(n))? as usize;
    let text = rest[1..].strip_prefix(". ")?;
    Some((level, text))
}

/// Scrape a Jira key (e.g. `DRM-1234`) from create output: `[A-Z][A-Z0-9_]+-[0-9]+`.
fn scan_key(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n {
        if !b[i].is_ascii_uppercase() {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i + 1;
        while j < n && (b[j].is_ascii_uppercase() || b[j].is_ascii_digit() || b[j] == b'_') {
            j += 1;
        }
        // Need >=2 chars before '-', then '-', then >=1 digit.
        if j > start + 1 && j < n && b[j] == b'-' {
            let mut k = j + 1;
            while k < n && b[k].is_ascii_digit() {
                k += 1;
            }
            if k > j + 1 {
                return Some(s[start..k].to_string());
            }
        }
        i = j.max(start + 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_skips_header_and_keeps_spaces() {
        let raw = "TYPE|KEY|SUMMARY|STATUS|ASSIGNEE\n\
                   Bug|DRM-43930|Backup timeouts|In Review|Itay Kalfon\n\
                   Story|DRM-43616|Research ClickHouse|Selected for Development|Itay Kalfon\n";
        let issues = parse_list(raw);
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].kind, "Bug");
        assert_eq!(issues[0].key, "DRM-43930");
        assert_eq!(issues[0].status, "In Review");
        assert_eq!(issues[1].status, "Selected for Development");
        assert_eq!(issues[1].assignee, "Itay Kalfon");
    }

    #[test]
    fn scan_key_finds_key() {
        assert_eq!(
            scan_key("Created issue ABC-123 ok").as_deref(),
            Some("ABC-123")
        );
        assert_eq!(scan_key("DRM-1234").as_deref(), Some("DRM-1234"));
        assert_eq!(scan_key("no key here"), None);
        assert_eq!(scan_key("A-1"), None); // needs >=2 chars before '-'
    }

    #[test]
    fn render_issue_uses_placeholder_when_no_description() {
        let json = r#"{"fields":{"summary":"Fix it","description":null}}"#;
        assert_eq!(render_issue(json).unwrap(), "# Fix it\n\n_No description_");
    }

    #[test]
    fn normalize_wiki_converts_headings() {
        assert_eq!(normalize_wiki("h3. Impact\nbody"), "### Impact\nbody");
        assert_eq!(normalize_wiki("h1. Top"), "# Top");
        assert_eq!(normalize_wiki("plain text"), "plain text");
        assert_eq!(normalize_wiki("h7. out of range"), "h7. out of range");
        assert_eq!(normalize_wiki("hello"), "hello");
    }

    #[test]
    fn render_issue_renders_adf() {
        let json = r#"{"fields":{"summary":"S","description":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}}}"#;
        assert_eq!(render_issue(json).unwrap(), "# S\n\nhi");
    }
}
