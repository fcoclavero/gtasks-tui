use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph, Wrap};

use super::{centered, fit};
use crate::app::{MoveMenuState, MoveStep, NewTaskState};

const MOVE_MENU_WIDTH: u16 = 60;

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

pub fn draw_move_menu(frame: &mut Frame, area: Rect, state: &mut MoveMenuState) {
    let rows = state.items.len().max(1) as u16;
    // Borders, the entries, a spacer, and the key hint.
    let popup = centered(area, MOVE_MENU_WIDTH, rows + 4);
    frame.render_widget(Clear, popup);
    let name = fit(&state.title, 30);
    let name = name.trim_end();
    let (title, hint) = match state.step {
        MoveStep::Action => (format!(" Move '{name}' "), "Enter: select    Esc: cancel"),
        MoveStep::List => (
            format!(" Move '{name}' to list "),
            "Enter: move    Esc: back",
        ),
        MoveStep::Parent => (
            format!(" Make '{name}' a subtask of "),
            "Enter: move    Esc: back",
        ),
    };
    let block = Block::bordered()
        .title(title)
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [list_area, _, hint_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(Line::from(hint).dim()).alignment(Alignment::Center),
        hint_area,
    );

    if state.items.is_empty() {
        frame.render_widget(Paragraph::new(" (nothing to choose)").dim(), list_area);
        return;
    }
    let width = list_area.width as usize;
    let items: Vec<ListItem> = state
        .items
        .iter()
        .map(|item| {
            let line = match item.disabled {
                None => Line::from(fit(&format!(" {}", item.label), width)),
                // Truncate the label, never the reason (reasons are short ASCII).
                Some(reason) => {
                    let reason = format!(" ({reason})");
                    let label = fit(
                        &format!(" {}", item.label),
                        width.saturating_sub(reason.len()),
                    );
                    Line::from(fit(&format!("{}{reason}", label.trim_end()), width)).dim()
                }
            };
            ListItem::new(line)
        })
        .collect();
    let list = List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, list_area, &mut state.list_state);
}
