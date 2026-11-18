use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Level, Mode};
use crate::tree::SortMode;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let refreshed = match app.last_refresh {
        Some(at) if app.refresh_in_flight => format!("refreshing (last {})", at.format("%H:%M:%S")),
        Some(at) => format!("refreshed {}", at.format("%H:%M:%S")),
        None if app.refresh_in_flight => "refreshing...".to_string(),
        None => "not refreshed yet".to_string(),
    };
    let done = if app.show_completed {
        "done: shown"
    } else {
        "done: hidden"
    };
    let sort = match app.sort_mode {
        SortMode::Position => "sort: position",
        SortMode::DueDate => "sort: due date",
    };
    let mut spans = vec![Span::styled(
        format!(" {} ", app.account_hint),
        Style::new().fg(Color::Black).bg(Color::Cyan),
    )];
    if !app.filter.trim().is_empty() {
        let escape = if matches!(app.mode, Mode::Filter(_)) {
            "cancel"
        } else {
            "clear"
        };
        let matches = app
            .columns
            .iter()
            .flat_map(|c| &c.rows)
            .filter(|r| !r.ghost)
            .count();
        spans.push(Span::styled(
            format!(
                " filter: {} ({matches}) | Esc: {escape} │",
                super::fit(app.filter.trim(), 24).trim_end()
            ),
            Style::new().fg(Color::Cyan),
        ));
    }
    spans.push(Span::raw(format!(" {refreshed} │ {done} │ {sort}")));
    let pending = app.pending_count();
    if pending > 0 {
        spans.push(Span::styled(
            format!(" │ {pending} pending"),
            Style::new().fg(Color::Yellow),
        ));
    }
    if let Some(msg) = app.visible_status() {
        let style = match msg.level {
            Level::Error => Style::new().fg(Color::Red),
            Level::Info => Style::new().fg(Color::Yellow),
        };
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(msg.text.clone(), style));
    } else {
        spans.push(Span::raw(" │ f: filter  s: sort  ?: help  q: quit").dim());
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
