//! The tracker abstraction — the seam that keeps Jira specifics out of the TUI.
//!
//! Anything that can list/describe/create issues implements [`Tracker`]; the UI
//! talks only to this trait. Porting to a different tracker (GitHub issues, …)
//! is a new `Tracker` impl and one line in `main`.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Well-known issue type names. Issues may carry any type string the server
/// returns; these are just the ones the tool reasons about.
pub mod kind {
    pub const TASK: &str = "Task";
    pub const SUBTASK: &str = "Sub-task";
}

/// One issue, with the cheap fields the list shows. The description is fetched
/// separately via [`Tracker::describe`] so listing stays fast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub key: String,
    pub summary: String,
    pub status: String,
    #[serde(default)]
    pub assignee: String,
    /// Issue type name (e.g. "Bug", "Task").
    pub kind: String,
}

/// A request to create a new issue.
#[derive(Debug, Clone)]
pub struct CreateRequest {
    pub kind: String,
    pub summary: String,
}

pub trait Tracker {
    /// The user's open issues (cheap fields only).
    fn list(&self) -> Result<Vec<Issue>>;
    /// One issue rendered to Markdown, ready for the preview renderer.
    fn describe(&self, key: &str) -> Result<String>;
    /// Create an issue and return its key.
    fn create(&self, req: &CreateRequest) -> Result<String>;
}
