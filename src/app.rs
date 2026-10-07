//! Application state and the single-threaded message handler. All mutation
//! happens here; the backend is reached only through the `Dispatcher`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::widgets::ListState;
use ratatui_textarea::TextArea;

use crate::backend::BackendError;
use crate::browser;
use crate::details::{DetailsAction, DetailsState};
use crate::model::{Task, TaskList, TaskPatch, TaskStatus};
use crate::tree::{self, Row};
use crate::worker::{Cmd, Dispatcher, Msg, OpOutcome};

const QUIT_GRACE: Duration = Duration::from_secs(2);
const INFO_TTL: Duration = Duration::from_secs(5);
const ERROR_TTL: Duration = Duration::from_secs(30);

pub struct Column {
    pub list: TaskList,
    /// Server state plus the optimistic overlay.
    pub tasks: Vec<Task>,
    /// Derived from `tasks`; rebuilt by `rebuild_rows`.
    pub rows: Vec<Row>,
    /// Selection is tracked by task id so it survives refreshes and reorders.
    pub selected: Option<String>,
    pub last_index: usize,
    pub list_state: ListState,
    /// Done-grace: a just-completed task that stays visible until the cursor moves.
    pub pinned: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
}

impl Column {
    fn new(list: TaskList) -> Self {
        Self {
            list,
            tasks: Vec::new(),
            rows: Vec::new(),
            selected: None,
            last_index: 0,
            list_state: ListState::default(),
            pinned: None,
            loading: true,
            error: None,
        }
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected.as_deref()?;
        self.rows.iter().position(|r| r.task_id == id)
    }

    pub fn selected_task(&self) -> Option<&Task> {
        self.task(self.selected.as_deref()?)
    }

    /// Completed tasks currently filtered out of `rows` (a pinned task is not hidden).
    pub fn hidden_done_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.is_completed() && !t.deleted)
            .filter(|t| !self.rows.iter().any(|r| r.task_id == t.id))
            .count()
    }

    pub fn rebuild_rows(&mut self, show_completed: bool) {
        if let Some(p) = &self.pinned
            && !self.tasks.iter().any(|t| &t.id == p)
        {
            self.pinned = None;
        }
        self.rows = tree::flatten(&self.tasks, show_completed, self.pinned.as_deref());
        match self.selected_index() {
            Some(i) => self.last_index = i,
            None if self.rows.is_empty() => {
                self.selected = None;
                self.last_index = 0;
            }
            None => {
                let i = self.last_index.min(self.rows.len() - 1);
                self.selected = Some(self.rows[i].task_id.clone());
                self.last_index = i;
            }
        }
        self.list_state.select(self.selected_index());
    }

    fn select_row(&mut self, index: usize, show_completed: bool) {
        let Some(target) = self.rows.get(index).map(|r| r.task_id.clone()) else {
            return;
        };
        if self.pinned.as_deref() != Some(target.as_str()) {
            self.pinned = None;
        }
        self.selected = Some(target);
        self.last_index = index;
        self.rebuild_rows(show_completed);
    }

    fn move_by(&mut self, delta: isize, show_completed: bool) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.selected_index().unwrap_or(0) as isize;
        let max = self.rows.len() as isize - 1;
        let target = (current + delta).clamp(0, max) as usize;
        self.select_row(target, show_completed);
    }

    fn leave(&mut self, show_completed: bool) {
        if self.pinned.take().is_some() {
            self.rebuild_rows(show_completed);
        }
    }
}

pub struct NewTaskState {
    pub tasklist_id: String,
    pub parent: Option<String>,
    pub parent_title: Option<String>,
    pub input: TextArea<'static>,
}

pub enum Mode {
    Normal,
    Details(Box<DetailsState>),
    NewTask(Box<NewTaskState>),
    ConfirmDelete {
        tasklist_id: String,
        task_id: String,
        title: String,
        subtask_count: usize,
    },
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

pub struct StatusMsg {
    pub level: Level,
    pub text: String,
    pub at: Instant,
}

/// What to undo if a mutation fails.
enum Op {
    Toggle {
        prev: Task,
    },
    Edit {
        prev: Task,
    },
    Create {
        temp_id: String,
    },
    Delete {
        prev: Task,
        prev_children: Vec<Task>,
    },
}

impl Op {
    fn task_id(&self) -> &str {
        match self {
            Op::Toggle { prev } | Op::Edit { prev } | Op::Delete { prev, .. } => &prev.id,
            Op::Create { temp_id } => temp_id,
        }
    }
}

struct PendingOp {
    tasklist_id: String,
    op: Op,
}

pub struct App {
    pub columns: Vec<Column>,
    pub focused: usize,
    pub col_offset: usize,
    pub show_completed: bool,
    pub mode: Mode,
    pub last_refresh: Option<DateTime<Local>>,
    pub account_hint: String,
    pub status: Option<StatusMsg>,
    pub should_quit: bool,
    pub refresh_in_flight: bool,
    pending: HashMap<u64, PendingOp>,
    next_op_id: u64,
    temp_counter: u64,
    generation: u64,
    lists_in_flight: usize,
    dirty_lists: HashSet<String>,
    quit_deadline: Option<Instant>,
    dispatch: Dispatcher,
}

impl App {
    pub fn new(dispatch: Dispatcher) -> Self {
        Self {
            columns: Vec::new(),
            focused: 0,
            col_offset: 0,
            show_completed: false,
            mode: Mode::Normal,
            last_refresh: None,
            account_hint: account_hint(),
            status: None,
            should_quit: false,
            refresh_in_flight: false,
            pending: HashMap::new(),
            next_op_id: 0,
            temp_counter: 0,
            generation: 0,
            lists_in_flight: 0,
            dirty_lists: HashSet::new(),
            quit_deadline: None,
            dispatch,
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn is_quitting(&self) -> bool {
        self.quit_deadline.is_some()
    }

    pub fn focused_column(&self) -> Option<&Column> {
        self.columns.get(self.focused)
    }

    fn focused_column_mut(&mut self) -> Option<&mut Column> {
        self.columns.get_mut(self.focused)
    }

    /// Status message still worth showing (info messages expire).
    pub fn visible_status(&self) -> Option<&StatusMsg> {
        let msg = self.status.as_ref()?;
        let ttl = match msg.level {
            Level::Info => INFO_TTL,
            Level::Error => ERROR_TTL,
        };
        (msg.at.elapsed() <= ttl).then_some(msg)
    }

    fn set_status(&mut self, level: Level, text: impl Into<String>) {
        self.status = Some(StatusMsg {
            level,
            text: text.into(),
            at: Instant::now(),
        });
    }

    fn info(&mut self, text: impl Into<String>) {
        self.set_status(Level::Info, text);
    }

    fn error(&mut self, err: &BackendError) {
        self.set_status(Level::Error, err.user_message());
    }

    /// Keep the focused column inside the window of `n_fit` visible columns.
    pub fn adjust_col_offset(&mut self, n_fit: usize) {
        let n_fit = n_fit.max(1);
        if self.focused < self.col_offset {
            self.col_offset = self.focused;
        } else if self.focused >= self.col_offset + n_fit {
            self.col_offset = self.focused + 1 - n_fit;
        }
        let max_offset = self.columns.len().saturating_sub(n_fit);
        self.col_offset = self.col_offset.min(max_offset);
    }

    // ----- message handling -------------------------------------------------

    pub fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Input(Event::Key(key)) if key.kind != KeyEventKind::Release => self.on_key(key),
            Msg::Input(Event::Mouse(mouse)) => self.on_mouse(mouse),
            Msg::Input(_) => {}
            Msg::Tick => {
                if !self.refresh_in_flight {
                    self.start_refresh();
                }
            }
            Msg::TasklistsLoaded { generation, result } => self.on_tasklists(generation, result),
            Msg::TasksLoaded {
                generation,
                tasklist_id,
                result,
            } => self.on_tasks(generation, tasklist_id, result),
            Msg::OpDone { op_id, result } => self.on_op_done(op_id, result),
        }
    }

    /// Called by the main loop every iteration while quitting.
    pub fn tick_quit(&mut self) {
        if let Some(deadline) = self.quit_deadline
            && (self.pending.is_empty() || Instant::now() >= deadline)
        {
            self.should_quit = true;
        }
    }

    fn request_quit(&mut self) {
        if self.pending.is_empty() {
            self.should_quit = true;
        } else if self.quit_deadline.is_none() {
            self.quit_deadline = Some(Instant::now() + QUIT_GRACE);
            self.info(format!(
                "finishing {} pending change(s)...",
                self.pending.len()
            ));
        }
    }

    // ----- refresh ----------------------------------------------------------

    pub fn start_refresh(&mut self) {
        if self.refresh_in_flight {
            self.info("refresh already in progress");
            return;
        }
        self.generation += 1;
        self.refresh_in_flight = true;
        (self.dispatch)(Cmd::ListTasklists {
            generation: self.generation,
        });
    }

    fn on_tasklists(&mut self, generation: u64, result: Result<Vec<TaskList>, BackendError>) {
        if generation != self.generation {
            return;
        }
        let lists = match result {
            Ok(lists) => lists,
            Err(err) => {
                self.refresh_in_flight = false;
                self.error(&err);
                return;
            }
        };
        self.reconcile_columns(lists);
        let ids: Vec<String> = self.columns.iter().map(|c| c.list.id.clone()).collect();
        self.lists_in_flight = ids.len();
        if ids.is_empty() {
            self.finish_refresh();
            return;
        }
        for tasklist_id in ids {
            (self.dispatch)(Cmd::ListTasks {
                generation,
                tasklist_id,
            });
        }
    }

    fn reconcile_columns(&mut self, lists: Vec<TaskList>) {
        let focused_id = self.focused_column().map(|c| c.list.id.clone());
        let mut old = std::mem::take(&mut self.columns);
        for list in lists {
            match old.iter().position(|c| c.list.id == list.id) {
                Some(i) => {
                    let mut column = old.remove(i);
                    column.list = list;
                    self.columns.push(column);
                }
                None => self.columns.push(Column::new(list)),
            }
        }
        self.focused = focused_id
            .and_then(|id| self.columns.iter().position(|c| c.list.id == id))
            .unwrap_or_else(|| self.focused.min(self.columns.len().saturating_sub(1)));
    }

    fn on_tasks(
        &mut self,
        generation: u64,
        tasklist_id: String,
        result: Result<Vec<Task>, BackendError>,
    ) {
        if generation != self.generation {
            return;
        }
        self.lists_in_flight = self.lists_in_flight.saturating_sub(1);
        if self.has_pending_for_list(&tasklist_id) {
            // Server state would clobber an optimistic update; retry once ops settle.
            self.dirty_lists.insert(tasklist_id);
        } else {
            let show_completed = self.show_completed;
            let mut failure = None;
            if let Some(column) = self.columns.iter_mut().find(|c| c.list.id == tasklist_id) {
                column.loading = false;
                match result {
                    Ok(tasks) => {
                        column.tasks = tasks;
                        column.error = None;
                        column.rebuild_rows(show_completed);
                    }
                    Err(err) => {
                        column.error = Some(err.user_message());
                        failure = Some(err);
                    }
                }
            }
            if let Some(err) = failure {
                self.error(&err);
            }
        }
        if self.lists_in_flight == 0 {
            self.finish_refresh();
        }
    }

    fn finish_refresh(&mut self) {
        self.refresh_in_flight = false;
        self.last_refresh = Some(Local::now());
        self.flush_dirty();
    }

    /// Re-fetch lists touched by completed mutations, once nothing is pending on them.
    fn flush_dirty(&mut self) {
        if self.refresh_in_flight {
            return;
        }
        let ready: Vec<String> = self
            .dirty_lists
            .iter()
            .filter(|id| !self.has_pending_for_list(id))
            .cloned()
            .collect();
        if ready.is_empty() {
            return;
        }
        self.refresh_in_flight = true;
        self.lists_in_flight = ready.len();
        for tasklist_id in ready {
            self.dirty_lists.remove(&tasklist_id);
            (self.dispatch)(Cmd::ListTasks {
                generation: self.generation,
                tasklist_id,
            });
        }
    }

    fn has_pending_for_list(&self, tasklist_id: &str) -> bool {
        self.pending.values().any(|p| p.tasklist_id == tasklist_id)
    }

    fn has_pending_for_task(&self, tasklist_id: &str, task_id: &str) -> bool {
        self.pending
            .values()
            .any(|p| p.tasklist_id == tasklist_id && p.op.task_id() == task_id)
    }

    // ----- key handling -----------------------------------------------------

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match &mut self.mode {
            Mode::Normal => self.on_key_normal(key),
            Mode::Help => self.mode = Mode::Normal,
            Mode::Details(_) => self.on_key_details(key),
            Mode::NewTask(_) => self.on_key_new_task(key),
            Mode::ConfirmDelete { .. } => self.on_key_confirm_delete(key),
        }
    }

    fn on_key_normal(&mut self, key: KeyEvent) {
        let show_completed = self.show_completed;
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Left | KeyCode::Char('h') => self.focus_by(-1),
            KeyCode::Right | KeyCode::Char('l') => self.focus_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home | KeyCode::Char('g') => self.move_by(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_by(isize::MAX / 2),
            KeyCode::Char(' ') => self.toggle_done(),
            KeyCode::Enter => self.open_details(),
            KeyCode::Char('c') => {
                self.show_completed = !show_completed;
                let show = self.show_completed;
                for column in &mut self.columns {
                    column.pinned = None;
                    column.rebuild_rows(show);
                }
            }
            KeyCode::Char('n') => self.open_new_task(false),
            KeyCode::Char('N') => self.open_new_task(true),
            KeyCode::Char('D') => self.open_confirm_delete(),
            KeyCode::Char('r') => self.start_refresh(),
            _ => {}
        }
    }

    fn on_key_details(&mut self, key: KeyEvent) {
        let Mode::Details(details) = &mut self.mode else {
            return;
        };
        let action = details.handle_key(key);
        self.on_details_action(action);
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        if let Mode::Details(details) = &self.mode {
            let action = details.handle_mouse(mouse);
            self.on_details_action(action);
        }
    }

    fn on_details_action(&mut self, action: DetailsAction) {
        match action {
            DetailsAction::Continue => {}
            DetailsAction::Cancel => self.mode = Mode::Normal,
            DetailsAction::OpenLink(url) => match browser::open_link(&url) {
                Ok(()) => self.info("opened task in browser"),
                Err(err) => self.set_status(Level::Error, err),
            },
            DetailsAction::Save(patch) => {
                let Mode::Details(details) = &self.mode else {
                    return;
                };
                let tasklist_id = details.tasklist_id.clone();
                let task_id = details.task_id.clone();
                self.mode = Mode::Normal;
                if patch.is_empty() {
                    return;
                }
                self.apply_edit(tasklist_id, task_id, patch);
            }
        }
    }

    fn on_key_new_task(&mut self, key: KeyEvent) {
        let Mode::NewTask(state) = &mut self.mode else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let title = state.input.lines().join(" ").trim().to_string();
                let tasklist_id = state.tasklist_id.clone();
                let parent = state.parent.clone();
                self.mode = Mode::Normal;
                if !title.is_empty() {
                    self.create_task(tasklist_id, parent, title);
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {}
            _ => {
                state.input.input(key);
            }
        }
    }

    fn on_key_confirm_delete(&mut self, key: KeyEvent) {
        let Mode::ConfirmDelete {
            tasklist_id,
            task_id,
            ..
        } = &self.mode
        else {
            return;
        };
        let (tasklist_id, task_id) = (tasklist_id.clone(), task_id.clone());
        self.mode = Mode::Normal;
        if matches!(
            key.code,
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
        ) {
            self.delete_task(tasklist_id, task_id);
        }
    }

    // ----- navigation -------------------------------------------------------

    fn focus_by(&mut self, delta: isize) {
        if self.columns.is_empty() {
            return;
        }
        let max = self.columns.len() as isize - 1;
        let target = (self.focused as isize + delta).clamp(0, max) as usize;
        if target == self.focused {
            return;
        }
        let show_completed = self.show_completed;
        if let Some(column) = self.focused_column_mut() {
            column.leave(show_completed);
        }
        self.focused = target;
    }

    fn move_by(&mut self, delta: isize) {
        let show_completed = self.show_completed;
        if let Some(column) = self.focused_column_mut() {
            column.move_by(delta, show_completed);
        }
    }

    /// The selected task if it can be mutated right now.
    fn actionable_selection(&mut self) -> Option<(String, Task)> {
        let column = self.focused_column()?;
        let task = column.selected_task()?.clone();
        let tasklist_id = column.list.id.clone();
        if task.is_temp() {
            self.info("task is still being created, try again in a moment");
            return None;
        }
        Some((tasklist_id, task))
    }

    // ----- mutations --------------------------------------------------------

    fn submit(&mut self, tasklist_id: String, op: Op, make_cmd: impl FnOnce(u64) -> Cmd) {
        self.next_op_id += 1;
        let op_id = self.next_op_id;
        self.pending.insert(op_id, PendingOp { tasklist_id, op });
        (self.dispatch)(make_cmd(op_id));
    }

    fn toggle_done(&mut self) {
        let Some((tasklist_id, prev)) = self.actionable_selection() else {
            return;
        };
        let new_status = if prev.is_completed() {
            TaskStatus::NeedsAction
        } else {
            TaskStatus::Completed
        };
        let patch = TaskPatch::status(new_status);
        let show_completed = self.show_completed;
        if let Some(column) = self.focused_column_mut() {
            if let Some(task) = column.task_mut(&prev.id) {
                patch.apply(task);
            }
            if new_status == TaskStatus::Completed && !show_completed {
                column.pinned = Some(prev.id.clone());
            }
            column.rebuild_rows(show_completed);
        }
        let task_id = prev.id.clone();
        let list_for_cmd = tasklist_id.clone();
        self.submit(tasklist_id, Op::Toggle { prev }, move |op_id| Cmd::Patch {
            op_id,
            tasklist: list_for_cmd,
            task: task_id,
            patch,
        });
    }

    fn open_details(&mut self) {
        let Some((tasklist_id, task)) = self.actionable_selection() else {
            return;
        };
        self.mode = Mode::Details(Box::new(DetailsState::new(tasklist_id, &task)));
    }

    fn apply_edit(&mut self, tasklist_id: String, task_id: String, patch: TaskPatch) {
        let show_completed = self.show_completed;
        let Some(column) = self.columns.iter_mut().find(|c| c.list.id == tasklist_id) else {
            return;
        };
        let Some(task) = column.task_mut(&task_id) else {
            self.info("task no longer exists");
            return;
        };
        let prev = task.clone();
        patch.apply(task);
        column.rebuild_rows(show_completed);
        let list_for_cmd = tasklist_id.clone();
        self.submit(tasklist_id, Op::Edit { prev }, move |op_id| Cmd::Patch {
            op_id,
            tasklist: list_for_cmd,
            task: task_id,
            patch,
        });
    }

    fn open_new_task(&mut self, as_subtask: bool) {
        let Some(column) = self.focused_column() else {
            self.info("no task list to add to");
            return;
        };
        if column.loading && column.tasks.is_empty() {
            self.info("list is still loading");
            return;
        }
        let tasklist_id = column.list.id.clone();
        let mut parent = None;
        let mut parent_title = None;
        if as_subtask {
            match column.selected_task() {
                Some(selected) if selected.is_temp() => {
                    self.info("parent is still being created, try again in a moment");
                    return;
                }
                Some(selected) => {
                    // Only one nesting level exists: a subtask's sibling goes under its parent.
                    let parent_id = selected
                        .parent
                        .clone()
                        .unwrap_or_else(|| selected.id.clone());
                    parent_title = column.task(&parent_id).map(|t| t.title.clone());
                    parent = Some(parent_id);
                }
                None => {}
            }
        }
        let mut input = TextArea::default();
        input.set_cursor_line_style(ratatui::style::Style::default());
        input.set_placeholder_text("Task title");
        self.mode = Mode::NewTask(Box::new(NewTaskState {
            tasklist_id,
            parent,
            parent_title,
            input,
        }));
    }

    fn create_task(&mut self, tasklist_id: String, parent: Option<String>, title: String) {
        self.temp_counter += 1;
        let temp_id = format!("tmp-{}", self.temp_counter);
        let show_completed = self.show_completed;
        let Some(column) = self.columns.iter_mut().find(|c| c.list.id == tasklist_id) else {
            return;
        };
        column.tasks.push(Task::new_local(
            temp_id.clone(),
            title.clone(),
            parent.clone(),
        ));
        column.selected = Some(temp_id.clone());
        column.pinned = None;
        column.rebuild_rows(show_completed);
        let list_for_cmd = tasklist_id.clone();
        self.submit(tasklist_id, Op::Create { temp_id }, move |op_id| {
            Cmd::Insert {
                op_id,
                tasklist: list_for_cmd,
                parent,
                title,
            }
        });
    }

    fn open_confirm_delete(&mut self) {
        let Some((tasklist_id, task)) = self.actionable_selection() else {
            return;
        };
        let subtask_count = self
            .focused_column()
            .map(|c| {
                c.tasks
                    .iter()
                    .filter(|t| t.parent.as_deref() == Some(task.id.as_str()) && !t.deleted)
                    .count()
            })
            .unwrap_or(0);
        self.mode = Mode::ConfirmDelete {
            tasklist_id,
            task_id: task.id,
            title: task.title,
            subtask_count,
        };
    }

    fn delete_task(&mut self, tasklist_id: String, task_id: String) {
        let show_completed = self.show_completed;
        let Some(column) = self.columns.iter_mut().find(|c| c.list.id == tasklist_id) else {
            return;
        };
        let Some(index) = column.tasks.iter().position(|t| t.id == task_id) else {
            self.info("task no longer exists");
            return;
        };
        let prev = column.tasks.remove(index);
        let (prev_children, kept): (Vec<Task>, Vec<Task>) = column
            .tasks
            .drain(..)
            .partition(|t| t.parent.as_deref() == Some(task_id.as_str()));
        column.tasks = kept;

        // Select the next surviving row below, else the nearest above.
        let removed: HashSet<&str> = std::iter::once(prev.id.as_str())
            .chain(prev_children.iter().map(|t| t.id.as_str()))
            .collect();
        let current = column.selected_index().unwrap_or(0);
        let survivor = |r: &Row| !removed.contains(r.task_id.as_str());
        let next = column
            .rows
            .iter()
            .skip(current + 1)
            .find(|r| survivor(r))
            .or_else(|| {
                column.rows[..current.min(column.rows.len())]
                    .iter()
                    .rev()
                    .find(|r| survivor(r))
            })
            .map(|r| r.task_id.clone());
        column.selected = next;
        column.pinned = None;
        column.rebuild_rows(show_completed);

        let list_for_cmd = tasklist_id.clone();
        self.submit(
            tasklist_id,
            Op::Delete {
                prev,
                prev_children,
            },
            move |op_id| Cmd::Delete {
                op_id,
                tasklist: list_for_cmd,
                task: task_id,
            },
        );
    }

    fn on_op_done(&mut self, op_id: u64, result: Result<OpOutcome, BackendError>) {
        let Some(PendingOp { tasklist_id, op }) = self.pending.remove(&op_id) else {
            return;
        };
        let show_completed = self.show_completed;
        let busy = self.has_pending_for_task(&tasklist_id, op.task_id());
        let mut failure = None;
        if let Some(column) = self.columns.iter_mut().find(|c| c.list.id == tasklist_id) {
            match (result, op) {
                (Ok(OpOutcome::Task(server)), Op::Toggle { prev } | Op::Edit { prev }) => {
                    // A later op on the same task owns the final state; don't clobber it.
                    if !busy && let Some(task) = column.task_mut(&prev.id) {
                        *task = *server;
                    }
                }
                (Err(err), Op::Toggle { prev } | Op::Edit { prev }) => {
                    if let Some(task) = column.task_mut(&prev.id) {
                        *task = prev;
                    }
                    failure = Some(err);
                }
                (Ok(OpOutcome::Task(server)), Op::Create { temp_id }) => {
                    let real_id = server.id.clone();
                    match column.task_mut(&temp_id) {
                        Some(task) => *task = *server,
                        None => column.tasks.push(*server),
                    }
                    if column.selected.as_deref() == Some(temp_id.as_str()) {
                        column.selected = Some(real_id.clone());
                    }
                    if column.pinned.as_deref() == Some(temp_id.as_str()) {
                        column.pinned = Some(real_id);
                    }
                }
                (Err(err), Op::Create { temp_id }) => {
                    column.tasks.retain(|t| t.id != temp_id);
                    failure = Some(err);
                }
                (Ok(_), Op::Delete { .. }) => {}
                (
                    Err(err),
                    Op::Delete {
                        prev,
                        prev_children,
                    },
                ) => {
                    column.tasks.push(prev);
                    column.tasks.extend(prev_children);
                    failure = Some(err);
                }
                (
                    Ok(OpOutcome::Deleted),
                    Op::Toggle { .. } | Op::Edit { .. } | Op::Create { .. },
                ) => {}
            }
            column.rebuild_rows(show_completed);
        }
        if let Some(err) = failure {
            self.error(&err);
        }
        self.dirty_lists.insert(tasklist_id);
        self.flush_dirty();
        self.tick_quit();
    }
}

fn account_hint() -> String {
    std::env::var_os("GOOGLE_WORKSPACE_CLI_CONFIG_DIR")
        .and_then(|dir| {
            std::path::Path::new(&dir)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc::{self, UnboundedReceiver};

    use super::*;
    use crate::backend::mock::MockBackend;
    use crate::model::is_temp_id;
    use crate::worker::dispatcher;

    fn key(code: KeyCode) -> Msg {
        Msg::Input(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    async fn settle(app: &mut App, rx: &mut UnboundedReceiver<Msg>) {
        while app.refresh_in_flight || !app.pending.is_empty() {
            let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("message within timeout")
                .expect("channel open");
            app.handle(msg);
        }
    }

    async fn loaded_app(fail_every: Option<u32>) -> (App, UnboundedReceiver<Msg>) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let backend = Arc::new(MockBackend::seeded(Duration::ZERO, fail_every));
        let mut app = App::new(dispatcher(backend, tx));
        app.start_refresh();
        settle(&mut app, &mut rx).await;
        (app, rx)
    }

    fn row_ids(app: &App) -> Vec<String> {
        app.focused_column()
            .map(|c| c.rows.iter().map(|r| r.task_id.clone()).collect())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn invalid_task_link_reports_error_and_preserves_open_editor() {
        use crossterm::event::{MouseButton, MouseEventKind};
        use ratatui::layout::Rect;

        let (mut app, _rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Enter));
        app.handle(key(KeyCode::Char('!')));
        let Mode::Details(details) = &mut app.mode else {
            panic!("expected details editor");
        };
        details.original.web_view_link = Some("javascript:alert(1)".into());
        details.task_link_area = Rect::new(10, 20, 19, 1);
        let patch = details.build_patch().unwrap();
        for event in [
            Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 20,
                modifiers: KeyModifiers::NONE,
            }),
        ] {
            app.handle(Msg::Input(event));
            let Mode::Details(details) = &app.mode else {
                panic!("opening a link must keep the editor open");
            };
            assert_eq!(details.build_patch().unwrap(), patch);
            assert_eq!(app.pending_count(), 0);
            assert_eq!(app.status.as_ref().unwrap().level, Level::Error);
            assert_eq!(
                app.status.as_ref().unwrap().text,
                "only valid HTTP(S) links can be opened"
            );
        }
    }

    #[tokio::test]
    async fn loads_columns_and_hides_completed() {
        let (app, _rx) = loaded_app(None).await;
        assert_eq!(app.columns.len(), 3);
        let inbox = &app.columns[0];
        assert_eq!(inbox.list.title, "Inbox");
        assert!(
            inbox
                .rows
                .iter()
                .all(|r| !inbox.task(&r.task_id).unwrap().is_completed())
        );
        assert_eq!(inbox.hidden_done_count(), 2);
        assert_eq!(inbox.selected.as_deref(), Some("inbox-1"));
        assert!(app.last_refresh.is_some());
    }

    #[tokio::test]
    async fn toggle_keeps_task_until_cursor_moves() {
        let (mut app, mut rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Char(' ')));
        let inbox = app.focused_column().unwrap();
        assert!(inbox.task("inbox-1").unwrap().is_completed());
        assert_eq!(inbox.pinned.as_deref(), Some("inbox-1"));
        assert!(row_ids(&app).contains(&"inbox-1".to_string()));

        app.handle(key(KeyCode::Down));
        assert!(!row_ids(&app).contains(&"inbox-1".to_string()));
        assert_eq!(
            app.focused_column().unwrap().selected.as_deref(),
            Some("inbox-2")
        );

        settle(&mut app, &mut rx).await;
        assert!(
            app.focused_column()
                .unwrap()
                .task("inbox-1")
                .unwrap()
                .is_completed()
        );
        assert!(!row_ids(&app).contains(&"inbox-1".to_string()));
    }

    #[tokio::test]
    async fn toggle_twice_undoes() {
        let (mut app, mut rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Char(' ')));
        app.handle(key(KeyCode::Char(' ')));
        settle(&mut app, &mut rx).await;
        let inbox = app.focused_column().unwrap();
        assert!(!inbox.task("inbox-1").unwrap().is_completed());
        assert_eq!(row_ids(&app)[0], "inbox-1");
    }

    #[tokio::test]
    async fn failed_patch_reverts_and_reports() {
        let (mut app, mut rx) = loaded_app(Some(1)).await;
        app.handle(key(KeyCode::Char(' ')));
        assert!(
            app.focused_column()
                .unwrap()
                .task("inbox-1")
                .unwrap()
                .is_completed()
        );
        settle(&mut app, &mut rx).await;
        assert!(
            !app.focused_column()
                .unwrap()
                .task("inbox-1")
                .unwrap()
                .is_completed()
        );
        assert_eq!(app.status.as_ref().map(|s| s.level), Some(Level::Error));
    }

    #[tokio::test]
    async fn selection_survives_refresh() {
        let (mut app, mut rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Down));
        app.handle(key(KeyCode::Down));
        let before = app.focused_column().unwrap().selected.clone();
        assert_eq!(before.as_deref(), Some("inbox-3"));
        app.handle(Msg::Tick);
        assert!(app.refresh_in_flight);
        settle(&mut app, &mut rx).await;
        assert_eq!(app.focused_column().unwrap().selected, before);
    }

    #[tokio::test]
    async fn create_swaps_temp_id_for_server_id() {
        let (mut app, mut rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Char('n')));
        assert!(matches!(app.mode, Mode::NewTask(_)));
        for c in "New one".chars() {
            app.handle(key(KeyCode::Char(c)));
        }
        app.handle(key(KeyCode::Enter));
        let selected = app.focused_column().unwrap().selected.clone().unwrap();
        assert!(is_temp_id(&selected));
        assert_eq!(row_ids(&app)[0], selected);
        settle(&mut app, &mut rx).await;
        let column = app.focused_column().unwrap();
        let selected = column.selected.clone().unwrap();
        assert!(!is_temp_id(&selected));
        assert_eq!(column.task(&selected).unwrap().title, "New one");
        assert!(!column.tasks.iter().any(|t| t.is_temp()));
    }

    #[tokio::test]
    async fn subtask_goes_under_parent_and_delete_removes_children() {
        let (mut app, mut rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Right));
        assert_eq!(app.focused_column().unwrap().list.title, "Work");
        app.handle(key(KeyCode::Down)); // work-1b (work-1a is completed and hidden)
        app.handle(key(KeyCode::Char('N')));
        match &app.mode {
            Mode::NewTask(s) => assert_eq!(s.parent.as_deref(), Some("work-1")),
            _ => panic!("expected new task prompt"),
        }
        app.handle(key(KeyCode::Char('x')));
        app.handle(key(KeyCode::Enter));
        settle(&mut app, &mut rx).await;
        let column = app.focused_column().unwrap();
        let created = column.selected_task().unwrap();
        assert_eq!(created.parent.as_deref(), Some("work-1"));

        app.handle(key(KeyCode::Home));
        assert_eq!(
            app.focused_column().unwrap().selected.as_deref(),
            Some("work-1")
        );
        app.handle(key(KeyCode::Char('D')));
        match &app.mode {
            Mode::ConfirmDelete { subtask_count, .. } => assert_eq!(*subtask_count, 4),
            _ => panic!("expected confirm prompt"),
        }
        app.handle(key(KeyCode::Char('y')));
        assert!(!row_ids(&app).iter().any(|id| id.starts_with("work-1")));
        settle(&mut app, &mut rx).await;
        assert!(!row_ids(&app).iter().any(|id| id.starts_with("work-1")));
        assert_eq!(
            app.focused_column().unwrap().selected.as_deref(),
            Some("work-2")
        );
    }

    #[tokio::test]
    async fn show_completed_toggle_reveals_done_tasks() {
        let (mut app, _rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Char('c')));
        assert!(row_ids(&app).contains(&"inbox-5".to_string()));
        app.handle(key(KeyCode::Char('c')));
        assert!(!row_ids(&app).contains(&"inbox-5".to_string()));
    }

    #[tokio::test]
    async fn edit_from_details_patches_title() {
        let (mut app, mut rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Enter));
        assert!(matches!(app.mode, Mode::Details(_)));
        app.handle(key(KeyCode::Char('!')));
        app.handle(Msg::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL,
        ))));
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(
            app.focused_column().unwrap().task("inbox-1").unwrap().title,
            "Pay electricity bill!"
        );
        settle(&mut app, &mut rx).await;
        assert_eq!(
            app.focused_column().unwrap().task("inbox-1").unwrap().title,
            "Pay electricity bill!"
        );
    }

    #[tokio::test]
    async fn quit_waits_for_pending_ops() {
        let (mut app, mut rx) = loaded_app(None).await;
        app.handle(key(KeyCode::Char(' ')));
        app.handle(key(KeyCode::Char('q')));
        assert!(app.is_quitting());
        assert!(!app.should_quit);
        settle(&mut app, &mut rx).await;
        app.tick_quit();
        assert!(app.should_quit);
    }
}
