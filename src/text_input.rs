//! Editing behaviour shared by the free-text `TextArea` fields: macOS-style
//! word movement, mouse cursor placement and drag selection, and copy/cut of
//! the selection.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Margin, Rect};
use ratatui_textarea::{CursorMove, TextArea};

/// Feed a key to the field, treating Alt+←/→ (Option on macOS) as word
/// movement like Ctrl+←/→. Shift still extends the selection.
pub fn input(area: &mut TextArea<'static>, mut key: KeyEvent) -> bool {
    if matches!(key.code, KeyCode::Left | KeyCode::Right)
        && key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::CONTROL)
    {
        key.modifiers = (key.modifiers - KeyModifiers::ALT) | KeyModifiers::CONTROL;
    }
    area.input(key)
}

/// Handle a left press, drag, or release on a bordered field drawn at `rect`.
/// The caller decides which field a gesture belongs to: a drag keeps going to
/// the field it started in even when the pointer leaves it.
pub fn mouse(area: &mut TextArea<'static>, rect: Rect, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            area.cancel_selection();
            jump(area, rect, mouse);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if !area.is_selecting() {
                area.start_selection();
            }
            jump(area, rect, mouse);
        }
        MouseEventKind::Up(MouseButton::Left)
            if area
                .selection_range()
                .is_some_and(|(start, end)| start == end) =>
        {
            area.cancel_selection();
        }
        _ => {}
    }
}

/// Column within the text of a click at `column`, for single-line fields
/// that place the cursor themselves.
pub fn text_column(area: &TextArea<'static>, rect: Rect, column: u16) -> usize {
    let inner = rect.inner(Margin::new(1, 1));
    (column.saturating_sub(inner.x).min(inner.width) + area.scroll_offset().1) as usize
}

fn jump(area: &mut TextArea<'static>, rect: Rect, mouse: MouseEvent) {
    let inner = rect.inner(Margin::new(1, 1));
    // Rows past the bottom clamp to the last line inside `screen_to_data`.
    let row = mouse.row.saturating_sub(inner.y) + area.scroll_offset().0;
    let col = text_column(area, rect, mouse.column);
    let target = area.screen_to_data(row as usize, col);
    area.move_cursor(CursorMove::Jump(target.0 as u16, target.1 as u16));
}

/// The selected text, if any, leaving the selection in place.
pub fn copy_selection(area: &mut TextArea<'static>) -> Option<String> {
    let (start, end) = area.selection_range().filter(|(start, end)| start != end)?;
    let cursor = area.cursor();
    let anchor = if (cursor.0, cursor.1) == start {
        end
    } else {
        start
    };
    area.copy();
    // `copy` ends the selection; restore it so the highlight stays visible.
    area.move_cursor(CursorMove::Jump(anchor.0 as u16, anchor.1 as u16));
    area.start_selection();
    area.move_cursor(CursorMove::Jump(cursor.0 as u16, cursor.1 as u16));
    Some(area.yank_text())
}

/// Remove the selected text and return it, if any.
pub fn cut_selection(area: &mut TextArea<'static>) -> Option<String> {
    area.selection_range().filter(|(start, end)| start != end)?;
    area.cut();
    Some(area.yank_text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::widgets::{Block, Widget};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn left(kind: fn(MouseButton) -> MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: kind(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// A bordered field drawn at (10, 5), so text starts at (11, 6).
    fn drawn(lines: &[&str]) -> (TextArea<'static>, Rect) {
        let mut area = TextArea::new(lines.iter().map(|l| l.to_string()).collect());
        area.set_block(Block::bordered());
        let rect = Rect::new(10, 5, 30, 6);
        (&area).render(rect, &mut Buffer::empty(Rect::new(0, 0, 50, 20)));
        (area, rect)
    }

    #[test]
    fn alt_arrows_move_by_word_and_shift_extends_selection() {
        let mut area = TextArea::new(vec!["Pay electricity bill".into()]);
        area.move_cursor(CursorMove::End);
        input(&mut area, key(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(area.cursor(), (0, 16));
        input(&mut area, key(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(area.cursor(), (0, 4));
        input(&mut area, key(KeyCode::Right, KeyModifiers::ALT));
        assert_eq!(area.cursor(), (0, 16));
        input(
            &mut area,
            key(KeyCode::Left, KeyModifiers::ALT | KeyModifiers::SHIFT),
        );
        assert_eq!(area.selection_range(), Some(((0, 4), (0, 16))));
        assert_eq!(copy_selection(&mut area).as_deref(), Some("electricity "));
    }

    #[test]
    fn click_places_cursor_and_drag_selects_across_lines() {
        let (mut area, rect) = drawn(&["hello world", "second line"]);
        mouse(&mut area, rect, left(MouseEventKind::Down, 17, 6));
        assert_eq!(area.cursor(), (0, 6));
        assert!(!area.is_selecting());
        mouse(&mut area, rect, left(MouseEventKind::Drag, 17, 7));
        mouse(&mut area, rect, left(MouseEventKind::Up, 17, 7));
        assert_eq!(area.selection_range(), Some(((0, 6), (1, 6))));
        assert_eq!(copy_selection(&mut area).as_deref(), Some("world\nsecond"));
        // Copying keeps the selection, with the cursor where the drag ended.
        assert_eq!(area.selection_range(), Some(((0, 6), (1, 6))));
        assert_eq!(area.cursor(), (1, 6));
    }

    #[test]
    fn clicks_outside_the_text_clamp_and_plain_clicks_leave_no_selection() {
        let (mut area, rect) = drawn(&["hello", "world"]);
        for (column, row, expected) in [(39, 6, (0, 5)), (0, 7, (1, 0)), (12, 19, (1, 1))] {
            mouse(&mut area, rect, left(MouseEventKind::Down, column, row));
            mouse(&mut area, rect, left(MouseEventKind::Up, column, row));
            assert_eq!(area.cursor(), expected, "click at ({column}, {row})");
            assert_eq!(area.selection_range(), None);
            assert_eq!(copy_selection(&mut area), None);
            assert_eq!(cut_selection(&mut area), None);
        }
    }

    #[test]
    fn dragging_backwards_copies_and_cuts_the_same_text() {
        let (mut area, rect) = drawn(&["hello world"]);
        mouse(&mut area, rect, left(MouseEventKind::Down, 22, 6));
        mouse(&mut area, rect, left(MouseEventKind::Drag, 17, 6));
        assert_eq!(copy_selection(&mut area).as_deref(), Some("world"));
        assert_eq!(area.cursor(), (0, 6));
        assert_eq!(cut_selection(&mut area).as_deref(), Some("world"));
        assert_eq!(area.lines(), ["hello "]);
        assert_eq!(area.selection_range(), None);
    }
}
