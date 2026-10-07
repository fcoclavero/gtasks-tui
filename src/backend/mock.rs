//! In-memory backend for `--mock`: lets the UI be exercised without `gws` or
//! network access, with optional latency and simulated mutation failures.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use chrono::Duration as ChronoDuration;

use super::{BackendError, BackendResult, TasksBackend};
use crate::dates;
use crate::model::{Link, Task, TaskList, TaskPatch, TaskStatus};

pub struct MockBackend {
    state: Mutex<State>,
    latency: Duration,
    fail_every: Option<u32>,
    mutations: AtomicU32,
}

struct State {
    lists: Vec<TaskList>,
    tasks: HashMap<String, Vec<Task>>,
    next_id: u32,
    next_position: u64,
}

impl MockBackend {
    pub fn seeded(latency: Duration, fail_every: Option<u32>) -> Self {
        let today = dates::today();
        let due = |days: i64| Some(dates::to_api_due(today + ChronoDuration::days(days)));
        let mut pos = 0u64;
        let mut t = |id: &str, title: &str, parent: Option<&str>| {
            let mut task = Task::new_local(id.into(), title.into(), parent.map(String::from));
            task.position = format!("{pos:020}");
            pos += 1;
            task
        };

        let mut overdue = t("inbox-1", "Pay electricity bill", None);
        overdue.due = due(-3);
        let mut today_task = t("inbox-2", "Call the dentist", None);
        today_task.due = due(0);
        let mut later = t("inbox-3", "Read the ratatui book", None);
        later.due = due(5);
        later.notes = Some("Chapters 1-3.\nTake notes on layout.".into());
        let mut with_links = t("inbox-4", "Reply to fixture thread", None);
        with_links.links = vec![Link {
            kind: "email".into(),
            description: "Fixture mail".into(),
            link: "https://mail.example.test/thread/42".into(),
        }];
        let mut done = t("inbox-5", "Buy groceries", None);
        done.status = TaskStatus::Completed;
        done.completed = Some(dates::now_rfc3339());
        let mut done_hidden = t("inbox-6", "Old chore", None);
        done_hidden.status = TaskStatus::Completed;
        done_hidden.hidden = true;
        done_hidden.completed = Some(dates::now_rfc3339());
        let no_date = t("inbox-7", "Think about the weekend", None);

        let parent = t("work-1", "Ship gtasks-tui", None);
        let mut child_a = t("work-1a", "Write the plan", Some("work-1"));
        child_a.status = TaskStatus::Completed;
        child_a.completed = Some(dates::now_rfc3339());
        let mut child_b = t("work-1b", "Implement the TUI", Some("work-1"));
        child_b.due = due(2);
        let child_c = t("work-1c", "Write the README", Some("work-1"));
        let mut other = t("work-2", "Quarterly review prep", None);
        other.due = due(-1);
        let mut other_child = t("work-2a", "Collect metrics", Some("work-2"));
        other_child.due = due(-1);

        let mut tasks = HashMap::new();
        tasks.insert(
            "list-inbox".to_string(),
            vec![
                overdue,
                today_task,
                later,
                with_links,
                done,
                done_hidden,
                no_date,
            ],
        );
        tasks.insert(
            "list-work".to_string(),
            vec![parent, child_a, child_b, child_c, other, other_child],
        );
        tasks.insert("list-empty".to_string(), Vec::new());

        let lists = vec![
            TaskList {
                id: "list-inbox".into(),
                title: "Inbox".into(),
            },
            TaskList {
                id: "list-work".into(),
                title: "Work".into(),
            },
            TaskList {
                id: "list-empty".into(),
                title: "Someday".into(),
            },
        ];

        Self {
            state: Mutex::new(State {
                lists,
                tasks,
                next_id: 1,
                next_position: 1_000_000,
            }),
            latency,
            fail_every,
            mutations: AtomicU32::new(0),
        }
    }

    async fn delay(&self) {
        if !self.latency.is_zero() {
            tokio::time::sleep(self.latency).await;
        }
    }

    fn maybe_fail(&self) -> BackendResult<()> {
        let n = self.mutations.fetch_add(1, Ordering::SeqCst) + 1;
        match self.fail_every {
            Some(every) if every > 0 && n.is_multiple_of(every) => Err(BackendError::Api {
                code: 500,
                reason: "mockFailure".into(),
                message: format!("simulated failure #{n}"),
            }),
            _ => Ok(()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn not_found(what: &str) -> BackendError {
    BackendError::Api {
        code: 404,
        reason: "notFound".into(),
        message: format!("{what} not found"),
    }
}

impl TasksBackend for MockBackend {
    async fn list_tasklists(&self) -> BackendResult<Vec<TaskList>> {
        self.delay().await;
        Ok(self.lock().lists.clone())
    }

    async fn list_tasks(&self, tasklist: &str) -> BackendResult<Vec<Task>> {
        self.delay().await;
        self.lock()
            .tasks
            .get(tasklist)
            .cloned()
            .ok_or_else(|| not_found("task list"))
    }

    async fn patch_task(
        &self,
        tasklist: &str,
        task: &str,
        patch: &TaskPatch,
    ) -> BackendResult<Task> {
        self.delay().await;
        self.maybe_fail()?;
        let mut state = self.lock();
        let tasks = state
            .tasks
            .get_mut(tasklist)
            .ok_or_else(|| not_found("task list"))?;
        let found = tasks
            .iter_mut()
            .find(|t| t.id == task)
            .ok_or_else(|| not_found("task"))?;
        patch.apply(found);
        found.updated = Some(dates::now_rfc3339());
        Ok(found.clone())
    }

    async fn insert_task(
        &self,
        tasklist: &str,
        parent: Option<&str>,
        title: &str,
    ) -> BackendResult<Task> {
        self.delay().await;
        self.maybe_fail()?;
        let mut state = self.lock();
        let id = format!("mock-{}", state.next_id);
        state.next_id += 1;
        // New tasks go to the top, like Google does.
        state.next_position -= 1;
        let mut task = Task::new_local(id, title.to_string(), parent.map(String::from));
        task.position = format!("{:020}", state.next_position);
        task.updated = Some(dates::now_rfc3339());
        let tasks = state
            .tasks
            .get_mut(tasklist)
            .ok_or_else(|| not_found("task list"))?;
        tasks.push(task.clone());
        Ok(task)
    }

    async fn delete_task(&self, tasklist: &str, task: &str) -> BackendResult<()> {
        self.delay().await;
        self.maybe_fail()?;
        let mut state = self.lock();
        let tasks = state
            .tasks
            .get_mut(tasklist)
            .ok_or_else(|| not_found("task list"))?;
        if !tasks.iter().any(|t| t.id == task) {
            return Err(not_found("task"));
        }
        tasks.retain(|t| t.id != task && t.parent.as_deref() != Some(task));
        Ok(())
    }
}
