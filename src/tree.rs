//! Builds the parent/subtask tree from the flat API list and flattens it into
//! the rows a column renders, applying the completed filter and the done-grace
//! ("pinned") rule.

use std::collections::{HashMap, HashSet};

use crate::dates;
use crate::model::Task;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMode {
    #[default]
    Position,
    DueDate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub task_id: String,
    /// 0 for top-level tasks, 1 for subtasks.
    pub depth: u8,
    /// A filtered-out parent that is still rendered because a child is visible.
    pub ghost: bool,
}

/// `pinned` is a task that stays visible even when completed and filtered,
/// so a just-completed task can be undone before the cursor moves away.
pub fn flatten(
    tasks: &[Task],
    show_completed: bool,
    pinned: Option<&str>,
    sort_mode: SortMode,
) -> Vec<Row> {
    let live: Vec<&Task> = tasks.iter().filter(|t| !t.deleted).collect();
    let by_id: HashMap<&str, &Task> = live.iter().map(|t| (t.id.as_str(), *t)).collect();

    let mut roots: Vec<&Task> = Vec::new();
    let mut children: HashMap<&str, Vec<&Task>> = HashMap::new();
    for task in &live {
        match root_ancestor(task, &by_id) {
            Some(root) => children.entry(root).or_default().push(task),
            None => roots.push(task),
        }
    }
    sort_siblings(&mut roots, sort_mode);
    for siblings in children.values_mut() {
        sort_siblings(siblings, sort_mode);
    }

    let visible = |t: &Task| show_completed || !t.is_completed() || pinned == Some(t.id.as_str());

    let mut rows = Vec::new();
    for root in roots {
        let kids: Vec<&Task> = children
            .get(root.id.as_str())
            .map(|v| v.iter().copied().filter(|t| visible(t)).collect())
            .unwrap_or_default();
        let root_visible = visible(root);
        if !root_visible && kids.is_empty() {
            continue;
        }
        rows.push(Row {
            task_id: root.id.clone(),
            depth: 0,
            ghost: !root_visible,
        });
        rows.extend(kids.into_iter().map(|t| Row {
            task_id: t.id.clone(),
            depth: 1,
            ghost: false,
        }));
    }
    rows
}

/// The top-level task this one hangs under, or `None` when it is itself a root
/// (no parent, or an orphan whose parent is not in the result set).
fn root_ancestor<'a>(task: &'a Task, by_id: &HashMap<&str, &'a Task>) -> Option<&'a str> {
    let mut current: &Task = task;
    let mut seen: HashSet<&str> = HashSet::new();
    let mut root: Option<&'a str> = None;
    while let Some(parent_id) = current.parent.as_deref() {
        if !seen.insert(parent_id) {
            break; // cycle guard
        }
        match by_id.get(parent_id) {
            Some(parent) => {
                root = Some(parent.id.as_str());
                current = parent;
            }
            None => break,
        }
    }
    root
}

fn sort_key(task: &Task) -> (String, String, String) {
    (task.position.clone(), task.title.clone(), task.id.clone())
}

fn sort_siblings(tasks: &mut [&Task], sort_mode: SortMode) {
    match sort_mode {
        SortMode::Position => tasks.sort_by_key(|t| sort_key(t)),
        SortMode::DueDate => tasks.sort_by_cached_key(|t| {
            let due = dates::due_date(t.due.as_deref());
            (due.is_none(), due, sort_key(t))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskStatus;

    fn task(id: &str, title: &str, position: &str, parent: Option<&str>, done: bool) -> Task {
        let mut t = Task::new_local(id.into(), title.into(), parent.map(String::from));
        t.position = position.into();
        if done {
            t.status = TaskStatus::Completed;
        }
        t
    }

    fn ids(rows: &[Row]) -> Vec<(&str, u8, bool)> {
        rows.iter()
            .map(|r| (r.task_id.as_str(), r.depth, r.ghost))
            .collect()
    }

    #[test]
    fn due_sort_orders_dates_across_years_and_keeps_position_ties() {
        let tasks: Vec<_> = [
            ("undated", "0", None),
            ("future", "1", Some("2027-01-01T00:00:00.000Z")),
            ("same-later", "4", Some("2026-12-31T00:00:00.000Z")),
            ("same-first", "3", Some("2026-12-31T23:00:00-12:00")),
            ("overdue", "5", Some("2025-12-31T00:00:00.000Z")),
            ("invalid", "2", Some("invalid")),
        ]
        .into_iter()
        .map(|(id, position, due)| {
            let mut t = task(id, id, position, None, false);
            t.due = due.map(String::from);
            t
        })
        .collect();
        assert_eq!(
            ids(&flatten(&tasks, false, None, SortMode::DueDate)),
            vec![
                ("overdue", 0, false),
                ("same-first", 0, false),
                ("same-later", 0, false),
                ("future", 0, false),
                ("undated", 0, false),
                ("invalid", 0, false),
            ]
        );
    }

    #[test]
    fn due_sort_keeps_children_nested_and_sorts_their_own_dates() {
        let mut parent = task("parent", "Parent", "1", None, false);
        parent.due = Some("2026-10-10T00:00:00.000Z".into());
        let undated_child = task("undated-child", "Undated", "0", Some("parent"), false);
        let mut later_child = task("later-child", "Later", "1", Some("parent"), false);
        later_child.due = Some("2026-10-09T00:00:00.000Z".into());
        let mut earlier_child = task("earlier-child", "Earlier", "2", Some("parent"), false);
        earlier_child.due = Some("2026-10-01T00:00:00.000Z".into());
        let mut other = task("other", "Other", "2", None, false);
        other.due = Some("2026-10-05T00:00:00.000Z".into());
        let tasks = vec![parent, undated_child, later_child, earlier_child, other];
        assert_eq!(
            ids(&flatten(&tasks, false, None, SortMode::DueDate)),
            vec![
                ("other", 0, false),
                ("parent", 0, false),
                ("earlier-child", 1, false),
                ("later-child", 1, false),
                ("undated-child", 1, false),
            ]
        );
    }

    #[test]
    fn orders_by_position_and_nests_children() {
        let tasks = vec![
            task("b", "B", "2", None, false),
            task("a", "A", "1", None, false),
            task("a2", "A2", "2", Some("a"), false),
            task("a1", "A1", "1", Some("a"), false),
        ];
        let rows = flatten(&tasks, false, None, SortMode::Position);
        assert_eq!(
            ids(&rows),
            vec![
                ("a", 0, false),
                ("a1", 1, false),
                ("a2", 1, false),
                ("b", 0, false)
            ]
        );
    }

    #[test]
    fn hides_completed_unless_shown_or_pinned() {
        let tasks = vec![
            task("a", "A", "1", None, true),
            task("b", "B", "2", None, false),
        ];
        assert_eq!(
            ids(&flatten(&tasks, false, None, SortMode::Position)),
            vec![("b", 0, false)]
        );
        assert_eq!(
            ids(&flatten(&tasks, true, None, SortMode::Position)),
            vec![("a", 0, false), ("b", 0, false)]
        );
        assert_eq!(
            ids(&flatten(&tasks, false, Some("a"), SortMode::Position)),
            vec![("a", 0, false), ("b", 0, false)]
        );
    }

    #[test]
    fn completed_parent_is_ghost_when_child_visible() {
        let tasks = vec![
            task("p", "P", "1", None, true),
            task("c", "C", "1", Some("p"), false),
            task("q", "Q", "2", None, true),
            task("qc", "QC", "1", Some("q"), true),
        ];
        assert_eq!(
            ids(&flatten(&tasks, false, None, SortMode::Position)),
            vec![("p", 0, true), ("c", 1, false)]
        );
    }

    #[test]
    fn orphans_are_promoted_and_deleted_dropped() {
        let tasks = vec![
            task("c", "C", "1", Some("missing"), false),
            task("d", "D", "0", None, false),
        ];
        let mut deleted = task("x", "X", "0", None, false);
        deleted.deleted = true;
        let mut all = tasks;
        all.push(deleted);
        assert_eq!(
            ids(&flatten(&all, false, None, SortMode::Position)),
            vec![("d", 0, false), ("c", 0, false)]
        );
    }

    #[test]
    fn deeper_nesting_collapses_under_root() {
        let tasks = vec![
            task("r", "R", "1", None, false),
            task("c", "C", "1", Some("r"), false),
            task("g", "G", "1", Some("c"), false),
        ];
        let rows = flatten(&tasks, false, None, SortMode::Position);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.depth <= 1));
    }
}
