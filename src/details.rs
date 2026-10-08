//! State and key handling for the task details editor (rendering lives in `ui::details`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui_textarea::{CursorMove, TextArea, WrapMode};

use crate::dates;
use crate::due_input::DueInput;
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
    OpenLink(String),
    Cancel,
}

pub struct DetailsState {
    pub tasklist_id: String,
    pub task_id: String,
    pub original: Task,
    pub title: TextArea<'static>,
    pub due: DueInput,
    pub notes: TextArea<'static>,
    pub focus: Field,
    pub error: Option<(Field, String)>,
    pub confirm_discard: bool,
    pub task_link_area: Rect,
}

fn single_line(text: &str) -> TextArea<'static> {
    let mut area = TextArea::new(vec![text.to_string()]);
    area.set_cursor_line_style(Style::default());
    area.move_cursor(CursorMove::End);
    area
}

impl DetailsState {
    pub fn new(tasklist_id: String, task: &Task) -> Self {
        let due = DueInput::new(dates::due_date(task.due.as_deref()), dates::today());
        let notes_lines: Vec<String> = task
            .notes
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();
        let mut notes = TextArea::new(notes_lines);
        notes.set_wrap_mode(WrapMode::WordOrGlyph);
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
            task_link_area: Rect::default(),
        }
    }

    pub fn title_text(&self) -> String {
        self.title.lines().join("").trim().to_string()
    }

    pub fn due_text(&self) -> String {
        self.due.text()
    }

    pub fn notes_text(&self) -> String {
        self.notes.lines().join("\n")
    }

    pub fn task_url(&self) -> Option<&str> {
        self.original
            .web_view_link
            .as_deref()
            .filter(|url| !url.trim().is_empty())
    }

    fn open_task_link(&self) -> DetailsAction {
        self.task_url()
            .map(|url| DetailsAction::OpenLink(url.to_string()))
            .unwrap_or(DetailsAction::Continue)
    }

    pub fn handle_mouse(&self, mouse: MouseEvent) -> DetailsAction {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self
                .task_link_area
                .contains(Position::new(mouse.column, mouse.row))
        {
            self.open_task_link()
        } else {
            DetailsAction::Continue
        }
    }

    pub fn is_dirty(&self) -> bool {
        let due_changed = self.due.resolve().map_or(true, |due| {
            due != dates::due_date(self.original.due.as_deref())
        });
        self.title_text() != self.original.title
            || due_changed
            || self.notes_text() != self.original.notes.clone().unwrap_or_default()
    }

    /// Diff against the original task; `Ok(empty patch)` when nothing changed.
    pub fn build_patch(&self) -> Result<TaskPatch, (Field, String)> {
        let title = self.title_text();
        if title.is_empty() {
            return Err((Field::Title, "title cannot be empty".into()));
        }
        let due = self.due.resolve().map_err(|e| (Field::Due, e))?;
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
            KeyCode::Char('o') if ctrl => self.open_task_link(),
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
                self.due.clear();
                self.confirm_discard = false;
                self.error = None;
                DetailsAction::Continue
            }
            _ => {
                self.confirm_discard = false;
                if self.error.as_ref().is_some_and(|(f, _)| *f == self.focus) {
                    self.error = None;
                }
                match self.focus {
                    Field::Title => {
                        self.title.input(key);
                    }
                    Field::Due => self.due.handle_key(key),
                    Field::Notes => {
                        self.notes.input(key);
                    }
                }
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
    fn ctrl_o_opens_task_link_without_changing_edits_or_focus() {
        let mut task = task();
        let url = "https://tasks.google.com/task/t1";
        task.web_view_link = Some(url.into());
        let mut details = DetailsState::new("l".into(), &task);
        details.handle_key(key(KeyCode::Char('!')));
        let patch = details.build_patch().unwrap();
        for focus in [Field::Title, Field::Due, Field::Notes] {
            details.focus = focus;
            assert_eq!(
                details.handle_key(ctrl('o')),
                DetailsAction::OpenLink(url.into())
            );
            assert_eq!(details.focus, focus);
            assert_eq!(details.build_patch().unwrap(), patch);
        }
    }

    #[test]
    fn only_left_clicks_inside_task_link_open_it() {
        let mut task = task();
        let url = "https://tasks.google.com/task/t1";
        task.web_view_link = Some(url.into());
        let mut details = DetailsState::new("l".into(), &task);
        details.task_link_area = Rect::new(10, 20, 19, 1);
        for (kind, column, row, opens) in [
            (MouseEventKind::Down(MouseButton::Left), 10, 20, true),
            (MouseEventKind::Down(MouseButton::Left), 28, 20, true),
            (MouseEventKind::Down(MouseButton::Left), 29, 20, false),
            (MouseEventKind::Down(MouseButton::Left), 10, 19, false),
            (MouseEventKind::Down(MouseButton::Right), 10, 20, false),
            (MouseEventKind::Up(MouseButton::Left), 10, 20, false),
            (MouseEventKind::Moved, 10, 20, false),
        ] {
            assert_eq!(
                details.handle_mouse(MouseEvent {
                    kind,
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                }),
                if opens {
                    DetailsAction::OpenLink(url.into())
                } else {
                    DetailsAction::Continue
                }
            );
        }
        assert!(!details.is_dirty());
    }

    #[test]
    fn missing_task_link_does_not_open_or_edit() {
        let mut details = DetailsState::new("l".into(), &task());
        for url in [None, Some(""), Some("  ")] {
            details.original.web_view_link = url.map(String::from);
            assert!(details.task_url().is_none());
            assert_eq!(details.handle_key(ctrl('o')), DetailsAction::Continue);
            assert!(!details.is_dirty());
        }
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
    fn shorthand_save_matches_preview_and_same_date_is_not_dirty() {
        let mut details = DetailsState::new("l".into(), &task());
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        details.due = DueInput::new(dates::due_date(details.original.due.as_deref()), today);
        details.focus = Field::Due;
        for c in "today".chars() {
            details.handle_key(key(KeyCode::Char(c)));
        }
        assert!(!details.is_dirty());
        assert_eq!(details.build_patch().unwrap(), TaskPatch::default());
        details.handle_key(ctrl('u'));
        for c in "tomorrow".chars() {
            details.handle_key(key(KeyCode::Char(c)));
        }
        assert!(details.is_dirty());
        assert_eq!(
            details.build_patch().unwrap().due,
            Some(details.due.resolve().unwrap())
        );
    }

    #[test]
    fn clearing_date_saves_none_and_partial_mask_blocks_save() {
        let mut details = DetailsState::new("l".into(), &task());
        details.focus = Field::Due;
        details.handle_key(ctrl('u'));
        assert_eq!(details.build_patch().unwrap().due, Some(None));
        details.handle_key(key(KeyCode::Char('2')));
        assert_eq!(details.handle_key(ctrl('s')), DetailsAction::Continue);
        assert_eq!(details.error.as_ref().map(|e| e.0), Some(Field::Due));
        assert_eq!(details.due.area.lines()[0], "2___-__-__");
        details.handle_key(ctrl('u'));
        assert!(details.error.is_none());
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
