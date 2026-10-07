//! Builds the parent/subtask tree from the flat API list and flattens it into
//! the rows a column renders, applying the completed filter and the done-grace
//! ("pinned") rule.

use std::collections::{HashMap, HashSet};

use crate::model::Task;

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
pub fn flatten(tasks: &[Task], show_completed: bool, pinned: Option<&str>) -> Vec<Row> {
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
    roots.sort_by_key(|t| sort_key(t));
    for siblings in children.values_mut() {
        siblings.sort_by_key(|t| sort_key(t));
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
    fn orders_by_position_and_nests_children() {
        let tasks = vec![
            task("b", "B", "2", None, false),
            task("a", "A", "1", None, false),
            task("a2", "A2", "2", Some("a"), false),
            task("a1", "A1", "1", Some("a"), false),
        ];
        let rows = flatten(&tasks, false, None);
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
        assert_eq!(ids(&flatten(&tasks, false, None)), vec![("b", 0, false)]);
        assert_eq!(
            ids(&flatten(&tasks, true, None)),
            vec![("a", 0, false), ("b", 0, false)]
        );
        assert_eq!(
            ids(&flatten(&tasks, false, Some("a"))),
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
            ids(&flatten(&tasks, false, None)),
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
            ids(&flatten(&all, false, None)),
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
        let rows = flatten(&tasks, false, None);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.depth <= 1));
    }
}
