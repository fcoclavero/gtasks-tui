//! Serde types mirroring the Google Tasks API resources, plus the patch type
//! used for local optimistic updates and for building `gws` request bodies.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::dates;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    NeedsAction,
    Completed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub link: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default = "default_status")]
    pub status: TaskStatus,
    /// RFC 3339 timestamp with date-only semantics (`YYYY-MM-DDT00:00:00.000Z`).
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub completed: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub web_view_link: Option<String>,
}

fn default_status() -> TaskStatus {
    TaskStatus::NeedsAction
}

impl Task {
    pub fn new_local(id: String, title: String, parent: Option<String>) -> Self {
        Self {
            id,
            title,
            notes: None,
            status: TaskStatus::NeedsAction,
            due: None,
            completed: None,
            parent,
            position: String::new(),
            hidden: false,
            deleted: false,
            updated: None,
            links: Vec::new(),
            web_view_link: None,
        }
    }

    pub fn is_completed(&self) -> bool {
        self.status == TaskStatus::Completed
    }

    /// Tasks created locally and not yet acknowledged by the server.
    pub fn is_temp(&self) -> bool {
        is_temp_id(&self.id)
    }
}

pub fn is_temp_id(id: &str) -> bool {
    id.starts_with("tmp-")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskList {
    pub id: String,
    #[serde(default)]
    pub title: String,
}

/// A partial update. `None` means "leave untouched"; for `due`, `Some(None)` clears it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub due: Option<Option<NaiveDate>>,
    pub status: Option<TaskStatus>,
}

impl TaskPatch {
    pub fn status(status: TaskStatus) -> Self {
        Self {
            status: Some(status),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// True when the patch removes the due date. `gws` validates request bodies
    /// against the discovery schema and rejects `null`, so `tasks.patch` cannot
    /// express this; the backend has to fall back to a full `tasks.update`.
    pub fn clears_due(&self) -> bool {
        self.due == Some(None)
    }

    /// Request body for `tasks.patch`. Never contains `null` (see `clears_due`).
    /// Un-completing sends only the status; the server clears `completed` itself.
    pub fn to_json(&self) -> Value {
        let mut body = Map::new();
        if let Some(title) = &self.title {
            body.insert("title".into(), json!(title));
        }
        if let Some(notes) = &self.notes {
            body.insert("notes".into(), json!(notes));
        }
        if let Some(Some(due)) = self.due {
            body.insert("due".into(), json!(dates::to_api_due(due)));
        }
        if let Some(status) = self.status {
            body.insert("status".into(), json!(status));
        }
        Value::Object(body)
    }

    /// Apply locally (optimistic update) using the same semantics the server will.
    pub fn apply(&self, task: &mut Task) {
        if let Some(title) = &self.title {
            task.title = title.clone();
        }
        if let Some(notes) = &self.notes {
            task.notes = if notes.is_empty() {
                None
            } else {
                Some(notes.clone())
            };
        }
        if let Some(due) = self.due {
            task.due = due.map(dates::to_api_due);
        }
        if let Some(status) = self.status {
            task.status = status;
            task.completed = match status {
                TaskStatus::Completed => Some(dates::now_rfc3339()),
                TaskStatus::NeedsAction => None,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_minimal_task() {
        let task: Task = serde_json::from_str(r#"{"id":"a","title":"x"}"#).unwrap();
        assert_eq!(task.status, TaskStatus::NeedsAction);
        assert!(task.links.is_empty());
        assert!(!task.hidden);
    }

    #[test]
    fn deserializes_full_task() {
        let raw = r#"{"id":"a","title":"x","status":"completed","completed":"2026-10-01T10:00:00.000Z",
            "due":"2026-10-02T00:00:00.000Z","parent":"p","position":"00000000000000000001","hidden":true,
            "links":[{"type":"email","description":"Mail","link":"https://mail.example/1"}],
            "webViewLink":"https://tasks.google.com/task/a"}"#;
        let task: Task = serde_json::from_str(raw).unwrap();
        assert!(task.is_completed());
        assert_eq!(task.parent.as_deref(), Some("p"));
        assert_eq!(task.links[0].kind, "email");
        assert!(task.web_view_link.is_some());
    }

    #[test]
    fn needs_action_patch_sends_status_only() {
        // gws rejects `"completed": null` client-side; the server clears it on its own.
        let body = TaskPatch::status(TaskStatus::NeedsAction).to_json();
        assert_eq!(body, json!({"status": "needsAction"}));
    }

    #[test]
    fn completed_patch_does_not_touch_completed_field() {
        let body = TaskPatch::status(TaskStatus::Completed).to_json();
        assert_eq!(body, json!({"status": "completed"}));
    }

    #[test]
    fn due_patch_serializes_and_flags_clearing() {
        let set = TaskPatch {
            due: Some(Some(NaiveDate::from_ymd_opt(2026, 10, 7).unwrap())),
            ..Default::default()
        };
        assert_eq!(set.to_json(), json!({"due": "2026-10-07T00:00:00.000Z"}));
        assert!(!set.clears_due());
        let clear = TaskPatch {
            due: Some(None),
            ..Default::default()
        };
        assert_eq!(clear.to_json(), json!({}));
        assert!(clear.clears_due());
    }

    #[test]
    fn apply_mirrors_server_semantics() {
        let mut task = Task::new_local("a".into(), "x".into(), None);
        TaskPatch::status(TaskStatus::Completed).apply(&mut task);
        assert!(task.is_completed());
        assert!(task.completed.is_some());
        TaskPatch::status(TaskStatus::NeedsAction).apply(&mut task);
        assert!(!task.is_completed());
        assert!(task.completed.is_none());
        TaskPatch {
            notes: Some(String::new()),
            ..Default::default()
        }
        .apply(&mut task);
        assert!(task.notes.is_none());
    }
}
