//! Rendering. Nothing here mutates app data except ratatui `ListState`
//! offsets, the column window offset, and the screen rects recorded for
//! mouse hit-testing.

mod columns;
mod details;
mod help;
mod prompt;
mod status;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use unicode_width::UnicodeWidthChar;

use crate::app::{App, Mode};
use crate::dates;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let today = dates::today();
    let [main, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    columns::draw(frame, main, app, today);
    match &mut app.mode {
        Mode::Normal => {}
        Mode::Details(details) => details::draw(frame, main, details),
        Mode::NewTask(state) => prompt::draw_new_task(frame, main, state),
        Mode::ConfirmDelete {
            title,
            subtask_count,
            ..
        } => prompt::draw_confirm_delete(frame, main, title, *subtask_count),
        Mode::Help => help::draw(frame, main),
    }
    status::draw(frame, status_area, app);
}

/// A centered rectangle of at most `width` x `height`, clamped to `area`.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Truncate to `width` display cells (with an ellipsis) and pad to exactly `width`.
pub fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let total: usize = text.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= width {
        let mut out = text.to_string();
        out.extend(std::iter::repeat_n(' ', width - total));
        return out;
    }
    let keep = width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > keep {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    used += 1;
    out.extend(std::iter::repeat_n(' ', width.saturating_sub(used)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_pads_and_truncates() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 0), "");
        assert_eq!(fit("日本語", 4), "日… ");
    }
}
