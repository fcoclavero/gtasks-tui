//! Glue between the single-threaded `App` and the async world: input thread,
//! refresh ticker, and the backend calls. Reads run concurrently; mutations
//! run one at a time per task list, in submission order. Everything reports
//! back through one `mpsc` channel of `Msg`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::Event;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::time::MissedTickBehavior;

use crate::backend::{BackendError, TasksBackend};
use crate::model::{Task, TaskList, TaskPatch};

#[derive(Debug)]
pub enum Msg {
    Input(Event),
    Tick,
    TasklistsLoaded {
        generation: u64,
        result: Result<Vec<TaskList>, BackendError>,
    },
    TasksLoaded {
        generation: u64,
        tasklist_id: String,
        result: Result<Vec<Task>, BackendError>,
    },
    OpDone {
        op_id: u64,
        result: Result<OpOutcome, BackendError>,
    },
}

#[derive(Debug)]
pub enum OpOutcome {
    Task(Box<Task>),
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    ListTasklists {
        generation: u64,
    },
    ListTasks {
        generation: u64,
        tasklist_id: String,
    },
    Patch {
        op_id: u64,
        tasklist: String,
        task: String,
        patch: TaskPatch,
    },
    Insert {
        op_id: u64,
        tasklist: String,
        parent: Option<String>,
        title: String,
    },
    Delete {
        op_id: u64,
        tasklist: String,
        task: String,
    },
}

/// How `App` sends work out without knowing the backend type.
pub type Dispatcher = Box<dyn Fn(Cmd)>;

impl Cmd {
    /// The list a mutation targets; `None` for reads.
    fn mutated_list(&self) -> Option<&str> {
        match self {
            Cmd::ListTasklists { .. } | Cmd::ListTasks { .. } => None,
            Cmd::Patch { tasklist, .. }
            | Cmd::Insert { tasklist, .. }
            | Cmd::Delete { tasklist, .. } => Some(tasklist),
        }
    }
}

/// Mutations go through a FIFO queue per task list: each `gws` call is its own
/// process and request, so concurrent writes (e.g. complete then un-complete)
/// could reach the server out of order and leave the older state behind.
pub fn dispatcher<B: TasksBackend>(backend: Arc<B>, tx: UnboundedSender<Msg>) -> Dispatcher {
    let queues: RefCell<HashMap<String, UnboundedSender<Cmd>>> = RefCell::default();
    Box::new(move |cmd| {
        let Some(tasklist) = cmd.mutated_list() else {
            let (backend, tx) = (backend.clone(), tx.clone());
            tokio::spawn(async move {
                let _ = tx.send(run_cmd(backend.as_ref(), cmd).await);
            });
            return;
        };
        let mut queues = queues.borrow_mut();
        let queue = queues
            .entry(tasklist.to_string())
            .or_insert_with(|| spawn_queue(backend.clone(), tx.clone()));
        let _ = queue.send(cmd);
    })
}

fn spawn_queue<B: TasksBackend>(backend: Arc<B>, tx: UnboundedSender<Msg>) -> UnboundedSender<Cmd> {
    let (queue, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            if tx.send(run_cmd(backend.as_ref(), cmd).await).is_err() {
                break;
            }
        }
    });
    queue
}

async fn run_cmd<B: TasksBackend>(backend: &B, cmd: Cmd) -> Msg {
    match cmd {
        Cmd::ListTasklists { generation } => Msg::TasklistsLoaded {
            generation,
            result: backend.list_tasklists().await,
        },
        Cmd::ListTasks {
            generation,
            tasklist_id,
        } => {
            let result = backend.list_tasks(&tasklist_id).await;
            Msg::TasksLoaded {
                generation,
                tasklist_id,
                result,
            }
        }
        Cmd::Patch {
            op_id,
            tasklist,
            task,
            patch,
        } => Msg::OpDone {
            op_id,
            result: backend
                .patch_task(&tasklist, &task, &patch)
                .await
                .map(|t| OpOutcome::Task(Box::new(t))),
        },
        Cmd::Insert {
            op_id,
            tasklist,
            parent,
            title,
        } => Msg::OpDone {
            op_id,
            result: backend
                .insert_task(&tasklist, parent.as_deref(), &title)
                .await
                .map(|t| OpOutcome::Task(Box::new(t))),
        },
        Cmd::Delete {
            op_id,
            tasklist,
            task,
        } => Msg::OpDone {
            op_id,
            result: backend
                .delete_task(&tasklist, &task)
                .await
                .map(|_| OpOutcome::Deleted),
        },
    }
}

/// Blocking crossterm reads on a plain thread; exits once the receiver is gone.
pub fn spawn_input_thread(tx: UnboundedSender<Msg>) {
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if tx.send(Msg::Input(event)).is_err() {
                break;
            }
        }
    });
}

pub fn spawn_ticker(tx: UnboundedSender<Msg>, secs: u64) {
    if secs == 0 {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(secs));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await; // the first tick fires immediately; skip it
        loop {
            interval.tick().await;
            if tx.send(Msg::Tick).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::backend::BackendResult;
    use crate::backend::mock::MockBackend;
    use crate::model::TaskStatus;

    /// Each successive write is faster than the previous one, so concurrent
    /// writes would land at the store in reverse order.
    struct SlowFirstWrite {
        inner: MockBackend,
        writes: AtomicU64,
    }

    impl TasksBackend for SlowFirstWrite {
        async fn list_tasklists(&self) -> BackendResult<Vec<TaskList>> {
            self.inner.list_tasklists().await
        }
        async fn list_tasks(&self, tasklist: &str) -> BackendResult<Vec<Task>> {
            self.inner.list_tasks(tasklist).await
        }
        async fn patch_task(
            &self,
            tasklist: &str,
            task: &str,
            patch: &TaskPatch,
        ) -> BackendResult<Task> {
            let n = self.writes.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(100u64.saturating_sub(n * 50))).await;
            self.inner.patch_task(tasklist, task, patch).await
        }
        async fn insert_task(
            &self,
            tasklist: &str,
            parent: Option<&str>,
            title: &str,
        ) -> BackendResult<Task> {
            self.inner.insert_task(tasklist, parent, title).await
        }
        async fn delete_task(&self, tasklist: &str, task: &str) -> BackendResult<()> {
            self.inner.delete_task(tasklist, task).await
        }
    }

    #[tokio::test]
    async fn mutations_on_a_list_reach_the_backend_in_order() {
        let backend = Arc::new(SlowFirstWrite {
            inner: MockBackend::seeded(Duration::ZERO, None),
            writes: AtomicU64::new(0),
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let dispatch = dispatcher(backend.clone(), tx);
        for (op_id, status) in [(1, TaskStatus::Completed), (2, TaskStatus::NeedsAction)] {
            dispatch(Cmd::Patch {
                op_id,
                tasklist: "list-inbox".into(),
                task: "inbox-1".into(),
                patch: TaskPatch::status(status),
            });
        }
        let mut done = Vec::new();
        while done.len() < 2 {
            let Some(Msg::OpDone { op_id, .. }) = rx.recv().await else {
                panic!("expected OpDone");
            };
            done.push(op_id);
        }
        assert_eq!(done, vec![1, 2]);
        let tasks = backend.list_tasks("list-inbox").await.unwrap();
        let task = tasks.iter().find(|t| t.id == "inbox-1").unwrap();
        assert!(!task.is_completed());
    }
}
