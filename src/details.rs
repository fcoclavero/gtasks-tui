//! State and key handling for the task details editor (rendering lives in `ui::details`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui_textarea::{CursorMove, TextArea};

use crate::dates;
use crate::model::{Task, TaskPatch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Due,
    Notes,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Self::Title => Self::Due,
            Self::Due => Self::Notes,
            Self::Notes => Self::Title,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Title => Self::Notes,
            Self::Due => Self::Title,
            Self::Notes => Self::Due,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DetailsAction {
    Continue,
    Save(TaskPatch),
    Cancel,
}

pub struct DetailsState {
    pub tasklist_id: String,
    pub task_id: String,
    pub original: Task,
    pub title: TextArea<'static>,
    pub due: TextArea<'static>,
    pub notes: TextArea<'static>,
    pub focus: Field,
    pub error: Option<(Field, String)>,
    pub confirm_discard: bool,
}

fn single_line(text: &str) -> TextArea<'static> {
    let mut area = TextArea::new(vec![text.to_string()]);
    area.set_cursor_line_style(Style::default());
    area.move_cursor(CursorMove::End);
    area
}

impl DetailsState {
    pub fn new(tasklist_id: String, task: &Task) -> Self {
        let due_text = dates::due_date(task.due.as_deref())
            .map(|d| d.to_string())
            .unwrap_or_default();
        let mut due = single_line(&due_text);
        due.set_placeholder_text("YYYY-MM-DD (empty = no date)");
        let notes_lines: Vec<String> = task
            .notes
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();
        let mut notes = TextArea::new(notes_lines);
        notes.set_cursor_line_style(Style::default());
        notes.set_placeholder_text("Notes");
        notes.move_cursor(CursorMove::Bottom);
        notes.move_cursor(CursorMove::End);
        Self {
            tasklist_id,
            task_id: task.id.clone(),
            original: task.clone(),
            title: single_line(&task.title),
            due,
            notes,
            focus: Field::Title,
            error: None,
            confirm_discard: false,
        }
    }

    pub fn title_text(&self) -> String {
        self.title.lines().join("").trim().to_string()
    }

    pub fn due_text(&self) -> String {
        self.due.lines().join("").trim().to_string()
    }

    pub fn notes_text(&self) -> String {
        self.notes.lines().join("\n")
    }

    pub fn is_dirty(&self) -> bool {
        let orig_due = dates::due_date(self.original.due.as_deref())
            .map(|d| d.to_string())
            .unwrap_or_default();
        self.title_text() != self.original.title
            || self.due_text() != orig_due
            || self.notes_text() != self.original.notes.clone().unwrap_or_default()
    }

    /// Diff against the original task; `Ok(empty patch)` when nothing changed.
    pub fn build_patch(&self) -> Result<TaskPatch, (Field, String)> {
        let title = self.title_text();
        if title.is_empty() {
            return Err((Field::Title, "title cannot be empty".into()));
        }
        let due = dates::parse_due_input(&self.due_text()).map_err(|e| (Field::Due, e))?;
        let notes = self.notes_text();

        let mut patch = TaskPatch::default();
        if title != self.original.title {
            patch.title = Some(title);
        }
        if notes != self.original.notes.clone().unwrap_or_default() {
            patch.notes = Some(notes);
        }
        if due != dates::due_date(self.original.due.as_deref()) {
            patch.due = Some(due);
        }
        Ok(patch)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DetailsAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                if !self.is_dirty() || self.confirm_discard {
                    return DetailsAction::Cancel;
                }
                self.confirm_discard = true;
                DetailsAction::Continue
            }
            KeyCode::Char('s') if ctrl => match self.build_patch() {
                Ok(patch) => DetailsAction::Save(patch),
                Err(err) => {
                    self.focus = err.0;
                    self.error = Some(err);
                    DetailsAction::Continue
                }
            },
            KeyCode::Tab => {
                self.focus = self.focus.next();
                self.confirm_discard = false;
                DetailsAction::Continue
            }
            KeyCode::BackTab => {
                self.focus = self.focus.prev();
                self.confirm_discard = false;
                DetailsAction::Continue
            }
            KeyCode::Enter if self.focus != Field::Notes => {
                self.focus = self.focus.next();
                DetailsAction::Continue
            }
            KeyCode::Char('u') if ctrl && self.focus == Field::Due => {
                self.due = single_line("");
                self.due
                    .set_placeholder_text("YYYY-MM-DD (empty = no date)");
                self.error = None;
                DetailsAction::Continue
            }
            _ => {
                self.confirm_discard = false;
                if self.error.as_ref().is_some_and(|(f, _)| *f == self.focus) {
                    self.error = None;
                }
                let area = match self.focus {
                    Field::Title => &mut self.title,
                    Field::Due => &mut self.due,
                    Field::Notes => &mut self.notes,
                };
                area.input(key);
                DetailsAction::Continue
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn task() -> Task {
        let mut t = Task::new_local("t1".into(), "Original".into(), None);
        t.due = Some("2026-10-07T00:00:00.000Z".into());
        t.notes = Some("line one\nline two".into());
        t
    }

    #[test]
    fn unchanged_save_is_empty_patch() {
        let mut d = DetailsState::new("l".into(), &task());
        assert!(!d.is_dirty());
        assert_eq!(
            d.handle_key(ctrl('s')),
            DetailsAction::Save(TaskPatch::default())
        );
    }

    #[test]
    fn edits_produce_a_diff_patch() {
        let mut d = DetailsState::new("l".into(), &task());
        d.handle_key(key(KeyCode::Char('!')));
        d.handle_key(key(KeyCode::Tab));
        d.handle_key(ctrl('u'));
        for c in "2026-12-24".chars() {
            d.handle_key(key(KeyCode::Char(c)));
        }
        assert!(d.is_dirty());
        match d.handle_key(ctrl('s')) {
            DetailsAction::Save(p) => {
                assert_eq!(p.title.as_deref(), Some("Original!"));
                assert_eq!(
                    p.due,
                    Some(Some(NaiveDate::from_ymd_opt(2026, 12, 24).unwrap()))
                );
                assert_eq!(p.notes, None);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn invalid_due_blocks_save_and_focuses_due() {
        let mut d = DetailsState::new("l".into(), &task());
        d.handle_key(key(KeyCode::Tab));
        d.handle_key(key(KeyCode::Char('x')));
        assert_eq!(d.handle_key(ctrl('s')), DetailsAction::Continue);
        assert_eq!(d.focus, Field::Due);
        assert!(d.error.is_some());
    }

    #[test]
    fn empty_title_is_rejected() {
        let mut d = DetailsState::new("l".into(), &task());
        d.title = single_line("   ");
        assert_eq!(d.handle_key(ctrl('s')), DetailsAction::Continue);
        assert_eq!(d.error.as_ref().map(|e| e.0), Some(Field::Title));
    }

    #[test]
    fn esc_needs_confirmation_when_dirty() {
        let mut d = DetailsState::new("l".into(), &task());
        assert_eq!(d.handle_key(key(KeyCode::Esc)), DetailsAction::Cancel);
        d.handle_key(key(KeyCode::Char('x')));
        assert_eq!(d.handle_key(key(KeyCode::Esc)), DetailsAction::Continue);
        assert_eq!(d.handle_key(key(KeyCode::Esc)), DetailsAction::Cancel);
    }

    #[test]
    fn enter_moves_focus_except_in_notes() {
        let mut d = DetailsState::new("l".into(), &task());
        d.handle_key(key(KeyCode::Enter));
        assert_eq!(d.focus, Field::Due);
        d.handle_key(key(KeyCode::Enter));
        assert_eq!(d.focus, Field::Notes);
        d.handle_key(key(KeyCode::Enter));
        assert_eq!(d.focus, Field::Notes);
        assert_eq!(d.notes.lines().len(), 3);
    }
}
