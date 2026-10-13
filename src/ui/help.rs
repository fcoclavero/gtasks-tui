use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::centered;

const KEYS: &[(&str, &str)] = &[
    ("← → / h l", "switch task list"),
    ("1–9", "jump to list by number (left to right)"),
    ("↑ ↓ / j k", "move between tasks and subtasks"),
    ("g / G", "first / last task"),
    ("Space", "toggle done (stays visible until you move away)"),
    ("Enter", "edit title, due date, notes"),
    ("c", "show / hide completed tasks"),
    ("s", "sort by position / due date"),
    ("n / N", "new task / new subtask under selection"),
    ("Tab (new)", "create as subtask under selection"),
    ("D", "delete selected task (asks for confirmation)"),
    ("r", "refresh now"),
    ("?", "this help"),
    ("q", "quit"),
];

pub fn draw(frame: &mut Frame, area: Rect) {
    let height = KEYS.len() as u16 + 4;
    let popup = centered(area, 64, height);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" Keys ")
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines: Vec<Line> = KEYS
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("  {key:<12}"), Style::new().fg(Color::Yellow)),
                Span::raw(*what),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from("  green: done   red: overdue   press any key to close").dim());
    frame.render_widget(Paragraph::new(lines), inner);
}
