//! Backend abstraction: the real `gws` subprocess client and an in-memory mock.

pub mod gws;
pub mod mock;

use std::future::Future;
use std::time::Duration;

use crate::model::{Task, TaskList, TaskPatch};

#[derive(thiserror::Error, Debug)]
pub enum BackendError {
    #[error("cannot run {bin}: {source}")]
    Spawn {
        bin: String,
        #[source]
        source: std::io::Error,
    },
    #[error("gws timed out after {0:?}")]
    Timeout(Duration),
    #[error("Google API error {code} ({reason}): {message}")]
    Api {
        code: u16,
        reason: String,
        message: String,
    },
    #[error("gws exited with {code:?}: {stderr}")]
    Exit { code: Option<i32>, stderr: String },
    #[error("unexpected gws output: {0}")]
    Parse(String),
}

impl BackendError {
    /// A one-line fix suggestion for the status bar, when there is one.
    pub fn user_hint(&self) -> Option<&'static str> {
        match self {
            Self::Api { code: 403, .. } => Some("run: gws auth login --services tasks"),
            Self::Api { code: 401, .. } => Some("run: gws auth login"),
            Self::Spawn { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                Some("gws not found in PATH (see --gws-bin)")
            }
            _ => None,
        }
    }

    /// Status-bar text: the fix hint first so it survives truncation.
    pub fn user_message(&self) -> String {
        match self.user_hint() {
            Some(hint) => format!("{hint} ({self})"),
            None => self.to_string(),
        }
    }
}

pub type BackendResult<T> = Result<T, BackendError>;

pub trait TasksBackend: Send + Sync + 'static {
    fn list_tasklists(&self) -> impl Future<Output = BackendResult<Vec<TaskList>>> + Send;
    fn list_tasks(&self, tasklist: &str) -> impl Future<Output = BackendResult<Vec<Task>>> + Send;
    fn patch_task(
        &self,
        tasklist: &str,
        task: &str,
        patch: &TaskPatch,
    ) -> impl Future<Output = BackendResult<Task>> + Send;
    fn insert_task(
        &self,
        tasklist: &str,
        parent: Option<&str>,
        title: &str,
    ) -> impl Future<Output = BackendResult<Task>> + Send;
    fn delete_task(
        &self,
        tasklist: &str,
        task: &str,
    ) -> impl Future<Output = BackendResult<()>> + Send;
}
