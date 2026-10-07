//! `gws` subprocess client. Every call is one `gws tasks ...` invocation with
//! JSON on stdout; `--page-all` yields NDJSON pages.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::{BackendError, BackendResult, TasksBackend};
use crate::model::{Task, TaskList, TaskPatch, TaskStatus};

pub struct GwsBackend {
    bin: PathBuf,
    timeout: Duration,
}

impl GwsBackend {
    pub fn new(bin: PathBuf, timeout: Duration) -> Self {
        Self { bin, timeout }
    }

    async fn exec(&self, args: Vec<String>) -> BackendResult<Vec<u8>> {
        let spawn_err = |source: std::io::Error| BackendError::Spawn {
            bin: self.bin.display().to_string(),
            source,
        };
        let child = tokio::process::Command::new(&self.bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(spawn_err)?;
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| BackendError::Timeout(self.timeout))?
            .map_err(spawn_err)?;
        classify(output.status.code(), output.stdout, &output.stderr)
    }

    /// Clear `due` by reading the task and writing it back whole without it,
    /// since `tasks.patch` can't carry the `null` that would clear it via gws.
    async fn update_clearing_due(&self, params: &str, patch: &TaskPatch) -> BackendResult<Task> {
        let out = self
            .exec(args(&["tasks", "tasks", "get", "--params", params]))
            .await?;
        let current: Map<String, Value> = parse_single(&out)?;
        let body = update_body(current, patch).to_string();
        let out = self
            .exec(args(&[
                "tasks", "tasks", "update", "--params", params, "--json", &body,
            ]))
            .await?;
        parse_single(&out)
    }
}

/// Full `tasks.update` body: the current resource with the patch merged in and
/// the fields it clears left out (an omitted field is cleared by a PUT).
fn update_body(mut task: Map<String, Value>, patch: &TaskPatch) -> Value {
    if patch.clears_due() {
        task.remove("due");
    }
    if patch.status == Some(TaskStatus::NeedsAction) {
        task.remove("completed");
    }
    if let Value::Object(fields) = patch.to_json() {
        task.extend(fields);
    }
    Value::Object(task)
}

#[derive(Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    code: u16,
    #[serde(default)]
    message: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    status: String,
}

impl From<ApiErrorEnvelope> for BackendError {
    fn from(env: ApiErrorEnvelope) -> Self {
        let reason = if env.error.reason.is_empty() {
            env.error.status
        } else {
            env.error.reason
        };
        BackendError::Api {
            code: env.error.code,
            reason,
            message: env.error.message,
        }
    }
}

/// Turn a finished `gws` process into stdout bytes or a typed error.
pub(crate) fn classify(
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: &[u8],
) -> BackendResult<Vec<u8>> {
    if code == Some(0) {
        return Ok(stdout);
    }
    if let Ok(env) = serde_json::from_slice::<ApiErrorEnvelope>(&stdout) {
        return Err(env.into());
    }
    Err(BackendError::Exit {
        code,
        stderr: clean_stderr(stderr),
    })
}

fn clean_stderr(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .filter(|l| !l.starts_with("Using keyring backend"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}

/// Parse NDJSON pages (one JSON object per line, as emitted by `--page-all`).
/// Also accepts a single page object and whitespace-only output.
pub(crate) fn parse_pages<T: DeserializeOwned>(stdout: &[u8]) -> BackendResult<Vec<T>> {
    let mut all = Vec::new();
    for page in serde_json::Deserializer::from_slice(stdout).into_iter::<Page<T>>() {
        let page = page.map_err(|e| BackendError::Parse(format!("{e}: {}", snippet(stdout))))?;
        all.extend(page.items);
    }
    Ok(all)
}

pub(crate) fn parse_single<T: DeserializeOwned>(stdout: &[u8]) -> BackendResult<T> {
    match serde_json::from_slice::<T>(stdout) {
        Ok(value) => Ok(value),
        Err(e) => match serde_json::from_slice::<ApiErrorEnvelope>(stdout) {
            Ok(env) => Err(env.into()),
            Err(_) => Err(BackendError::Parse(format!("{e}: {}", snippet(stdout)))),
        },
    }
}

fn snippet(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    let mut s: String = text.chars().take(120).collect();
    if s.len() < text.len() {
        s.push('…');
    }
    s
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

impl TasksBackend for GwsBackend {
    async fn list_tasklists(&self) -> BackendResult<Vec<TaskList>> {
        let params = json!({ "maxResults": 100 }).to_string();
        let out = self
            .exec(args(&[
                "tasks",
                "tasklists",
                "list",
                "--page-all",
                "--params",
                &params,
            ]))
            .await?;
        parse_pages(&out)
    }

    async fn list_tasks(&self, tasklist: &str) -> BackendResult<Vec<Task>> {
        let params = json!({
            "tasklist": tasklist,
            "showCompleted": true,
            "showHidden": true,
            "maxResults": 100,
        })
        .to_string();
        let out = self
            .exec(args(&[
                "tasks",
                "tasks",
                "list",
                "--page-all",
                "--params",
                &params,
            ]))
            .await?;
        parse_pages(&out)
    }

    async fn patch_task(
        &self,
        tasklist: &str,
        task: &str,
        patch: &TaskPatch,
    ) -> BackendResult<Task> {
        let params = json!({ "tasklist": tasklist, "task": task }).to_string();
        if patch.clears_due() {
            return self.update_clearing_due(&params, patch).await;
        }
        let body = patch.to_json().to_string();
        let out = self
            .exec(args(&[
                "tasks", "tasks", "patch", "--params", &params, "--json", &body,
            ]))
            .await?;
        parse_single(&out)
    }

    async fn insert_task(
        &self,
        tasklist: &str,
        parent: Option<&str>,
        title: &str,
    ) -> BackendResult<Task> {
        let mut params = json!({ "tasklist": tasklist });
        if let Some(parent) = parent {
            params["parent"] = json!(parent);
        }
        let params = params.to_string();
        let body = json!({ "title": title }).to_string();
        let out = self
            .exec(args(&[
                "tasks", "tasks", "insert", "--params", &params, "--json", &body,
            ]))
            .await?;
        parse_single(&out)
    }

    async fn delete_task(&self, tasklist: &str, task: &str) -> BackendResult<()> {
        let params = json!({ "tasklist": tasklist, "task": task }).to_string();
        self.exec(args(&["tasks", "tasks", "delete", "--params", &params]))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASKLISTS: &[u8] = include_bytes!("../../tests/fixtures/tasklists.ndjson");
    const TASKS_PAGES: &[u8] = include_bytes!("../../tests/fixtures/tasks_page_all.ndjson");
    const TASKS_EMPTY: &[u8] = include_bytes!("../../tests/fixtures/tasks_empty.ndjson");
    const ERROR_403: &[u8] = include_bytes!("../../tests/fixtures/error_403.json");

    #[test]
    fn update_body_drops_cleared_fields_and_merges_patch() {
        let current = json!({
            "id": "a", "title": "old", "notes": "n", "status": "completed",
            "completed": "2026-10-01T10:00:00.000Z", "due": "2026-10-02T00:00:00.000Z",
        });
        let Value::Object(current) = current else {
            unreachable!()
        };
        let patch = TaskPatch {
            title: Some("new".into()),
            due: Some(None),
            status: Some(TaskStatus::NeedsAction),
            ..Default::default()
        };
        assert_eq!(
            update_body(current, &patch),
            json!({"id": "a", "title": "new", "notes": "n", "status": "needsAction"})
        );
    }

    #[test]
    fn parses_tasklists() {
        let lists: Vec<TaskList> = parse_pages(TASKLISTS).unwrap();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].title, "Inbox");
    }

    #[test]
    fn concatenates_ndjson_pages() {
        let tasks: Vec<Task> = parse_pages(TASKS_PAGES).unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[2].parent.as_deref(), Some("task-a"));
    }

    #[test]
    fn empty_page_has_no_items() {
        let tasks: Vec<Task> = parse_pages(TASKS_EMPTY).unwrap();
        assert!(tasks.is_empty());
        let none: Vec<Task> = parse_pages(b"  \n").unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn api_error_is_classified_with_hint() {
        let err = classify(
            Some(1),
            ERROR_403.to_vec(),
            b"Using keyring backend: keyring\n",
        )
        .unwrap_err();
        match &err {
            BackendError::Api { code, reason, .. } => {
                assert_eq!(*code, 403);
                assert_eq!(reason, "insufficientPermissions");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            err.user_hint(),
            Some("run: gws auth login --services tasks")
        );
    }

    #[test]
    fn usage_error_keeps_stderr_without_keyring_line() {
        let err = classify(
            Some(3),
            Vec::new(),
            b"Using keyring backend: keyring\nerror: unrecognized subcommand 'lis'\n",
        )
        .unwrap_err();
        match err {
            BackendError::Exit { code, stderr } => {
                assert_eq!(code, Some(3));
                assert_eq!(stderr, "error: unrecognized subcommand 'lis'");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn single_falls_back_to_error_envelope() {
        let err = parse_single::<Task>(ERROR_403).unwrap_err();
        assert!(matches!(err, BackendError::Api { code: 403, .. }));
        let err = parse_single::<Task>(b"not json").unwrap_err();
        assert!(matches!(err, BackendError::Parse(_)));
    }
}
