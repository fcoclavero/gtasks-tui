use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::{centered, fit};
use crate::app::NewTaskState;

pub fn draw_new_task(frame: &mut Frame, area: Rect, state: &mut NewTaskState) {
    let popup = centered(area, 70, 3);
    frame.render_widget(Clear, popup);
    let title = match &state.parent {
        Some(parent) => format!(
            " New subtask of: {} ",
            fit(state.parent_title.as_deref().unwrap_or(parent), 40).trim_end()
        ),
        None => " New task ".to_string(),
    };
    let hint = if state.parent.is_some() {
        " Enter to create subtask, Esc to cancel "
    } else {
        " Enter to create, Esc to cancel | Tab to create subtask "
    };
    state.input.set_block(
        Block::bordered()
            .title(title)
            .title_bottom(Line::from(hint).dim())
            .border_style(Style::new().fg(Color::Cyan)),
    );
    state.area = popup;
    frame.render_widget(&state.input, popup);
}

pub fn draw_confirm_delete(frame: &mut Frame, area: Rect, title: &str, subtask_count: usize) {
    let popup = centered(area, 60, 5);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" Delete task? ")
        .border_style(Style::new().fg(Color::Red));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let what = if subtask_count > 0 {
        format!("'{title}' and {subtask_count} subtask(s)")
    } else {
        format!("'{title}'")
    };
    let text = vec![
        Line::from(format!("Delete {what}?")),
        Line::from("y: delete    n/Esc: cancel").dim(),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        inner,
    );
}
