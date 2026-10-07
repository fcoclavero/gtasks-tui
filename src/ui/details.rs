use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui_textarea::TextArea;

use super::{centered, fit};
use crate::details::{DetailsState, Field};

pub fn draw(frame: &mut Frame, area: Rect, details: &mut DetailsState) {
    let popup = centered(
        area,
        (area.width * 9 / 10).clamp(40, 100),
        (area.height * 9 / 10).clamp(14, 36),
    );
    frame.render_widget(Clear, popup);
    let heading = format!(" Task: {} ", fit(&details.original.title, 40).trim_end());
    let block = Block::bordered()
        .title(heading)
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let links = &details.original.links;
    let links_rows: u16 = if links.is_empty() {
        0
    } else {
        links.len().min(4) as u16 + 2
    };
    let [title_area, due_area, notes_area, links_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(links_rows),
        Constraint::Length(1),
    ])
    .areas(inner);

    let focus = details.focus;
    let error_field = details.error.as_ref().map(|e| e.0);
    style_field(
        &mut details.title,
        " Title ",
        focus == Field::Title,
        error_field == Some(Field::Title),
    );
    style_field(
        &mut details.due,
        " Due (YYYY-MM-DD) ",
        focus == Field::Due,
        error_field == Some(Field::Due),
    );
    style_field(&mut details.notes, " Notes ", focus == Field::Notes, false);
    frame.render_widget(&details.title, title_area);
    frame.render_widget(&details.due, due_area);
    frame.render_widget(&details.notes, notes_area);

    if links_rows > 0 {
        let lines: Vec<Line> = links
            .iter()
            .map(|l| {
                let label = if l.description.is_empty() {
                    l.kind.as_str()
                } else {
                    l.description.as_str()
                };
                Line::from(format!("{label}: {}", l.link))
            })
            .collect();
        let paragraph = Paragraph::new(lines)
            .block(Block::bordered().title(" Links (read-only) ").dim())
            .wrap(Wrap { trim: true })
            .dim();
        frame.render_widget(paragraph, links_area);
    }

    let footer = if let Some((_, err)) = &details.error {
        Line::from(err.as_str()).red()
    } else if details.confirm_discard {
        Line::from("Unsaved changes: Esc again to discard, Ctrl-S to save").yellow()
    } else {
        Line::from(
            "Tab/Shift-Tab fields · Enter next · Ctrl-S save · Ctrl-U clear due · Esc cancel",
        )
        .dim()
    };
    frame.render_widget(Paragraph::new(footer), footer_area);
}

fn style_field(area: &mut TextArea<'static>, title: &'static str, focused: bool, error: bool) {
    let border = if error {
        Style::new().fg(Color::Red)
    } else if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    area.set_block(Block::bordered().title(title).border_style(border));
    area.set_cursor_style(if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    });
}
