use chrono::NaiveDate;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::fit;
use crate::app::{App, Column, Level};
use crate::dates;
use crate::model::Task;
use crate::tree::Row;

pub const MIN_COL_WIDTH: u16 = 28;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, today: NaiveDate) {
    if area.width < 20 || area.height < 3 {
        frame.render_widget(
            Paragraph::new("terminal too small").alignment(Alignment::Center),
            area,
        );
        return;
    }
    if app.columns.is_empty() {
        let block = Block::bordered().title(" Google Tasks ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let error = app
            .visible_status()
            .filter(|s| s.level == Level::Error)
            .map(|s| s.text.clone());
        let paragraph = match error {
            Some(text) => Paragraph::new(text).red(),
            None if app.refresh_in_flight => Paragraph::new("loading task lists...").dim(),
            None => Paragraph::new("No task lists. Press r to refresh, ? for help.").dim(),
        };
        frame.render_widget(
            paragraph
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            super::centered(inner, inner.width.saturating_sub(4), 4),
        );
        return;
    }

    let n_fit = ((area.width / MIN_COL_WIDTH).max(1) as usize).min(app.columns.len());
    app.adjust_col_offset(n_fit);
    let first = app.col_offset;
    let last = first + n_fit; // exclusive
    let more_left = first > 0;
    let more_right = last < app.columns.len();
    let rects = Layout::horizontal(vec![Constraint::Fill(1); n_fit]).split(area);
    let focused = app.focused;

    for (slot, index) in (first..last).enumerate() {
        let column = &mut app.columns[index];
        let marker_left = more_left && slot == 0;
        let marker_right = more_right && slot + 1 == n_fit;
        draw_column(
            frame,
            rects[slot],
            column,
            index == focused,
            today,
            (marker_left, marker_right),
        );
    }
}

fn draw_column(
    frame: &mut Frame,
    area: Rect,
    column: &mut Column,
    focused: bool,
    today: NaiveDate,
    (more_left, more_right): (bool, bool),
) {
    let mut title = format!(" {} ", column.list.title);
    let hidden = column.hidden_done_count();
    if hidden > 0 {
        title = format!(" {} ({hidden} done hidden) ", column.list.title);
    }
    let border_style = if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().dim()
    };
    if more_left {
        title = format!(" ◀{title}");
    }
    let mut block = Block::bordered().title(title).border_style(border_style);
    if more_right {
        block = block.title_top(Line::from(" ▶ ").right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if column.rows.is_empty() {
        let text = if let Some(err) = &column.error {
            Paragraph::new(err.as_str()).red().wrap(Wrap { trim: true })
        } else if column.loading {
            Paragraph::new("loading...").dim()
        } else {
            Paragraph::new("(empty)").dim()
        };
        frame.render_widget(text, inner);
        return;
    }

    let items: Vec<ListItem> = column
        .rows
        .iter()
        .map(|row| {
            let task = column.task(&row.task_id);
            ListItem::new(row_line(task, row, inner.width as usize, today))
        })
        .collect();
    let highlight = if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    };
    let list = List::new(items).highlight_style(highlight);
    column.list_state.select(column.selected_index());
    frame.render_stateful_widget(list, inner, &mut column.list_state);
}

fn row_line(task: Option<&Task>, row: &Row, width: usize, today: NaiveDate) -> Line<'static> {
    let Some(task) = task else {
        return Line::from(fit("?", width));
    };
    let indent = if row.depth > 0 { "  └ " } else { "" };
    let date = dates::due_date(task.due.as_deref())
        .map(|d| dates::format_short(d, today))
        .unwrap_or_default();
    let date_cells = if date.is_empty() { 0 } else { date.width() + 1 };
    let title_width = width.saturating_sub(indent.width() + date_cells);

    let color = if task.is_completed() {
        Color::Green
    } else if dates::is_overdue(task, today) {
        Color::Red
    } else {
        Color::White
    };
    let mut style = Style::new().fg(color);
    if row.ghost {
        style = style.add_modifier(Modifier::DIM);
    }
    let mut spans = vec![Span::styled(
        format!("{indent}{}", fit(&task.title, title_width)),
        style,
    )];
    if !date.is_empty() {
        spans.push(Span::styled(format!(" {date}"), style));
    }
    Line::from(spans)
}
