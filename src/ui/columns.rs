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
            let mut lines = row_lines(task, row, inner.width as usize, today);
            // Ratatui skips items taller than the viewport entirely.
            if lines.len() > inner.height as usize {
                lines.truncate(inner.height as usize);
                if inner.height > 1 {
                    let last = lines.last_mut().unwrap();
                    *last = Line::styled(
                        fit("… (more in editor)", inner.width as usize),
                        last.spans[0].style,
                    );
                }
            }
            ListItem::new(lines)
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

fn row_lines(task: Option<&Task>, row: &Row, width: usize, today: NaiveDate) -> Vec<Line<'static>> {
    let Some(task) = task else {
        return vec![Line::from(fit("?", width))];
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
    textwrap::wrap(&task.title, title_width.max(1))
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let prefix = if index == 0 {
                indent.to_string()
            } else {
                " ".repeat(indent.width())
            };
            let mut spans = vec![Span::styled(
                format!("{prefix}{}", fit(&text, title_width)),
                style,
            )];
            if index == 0 && !date.is_empty() {
                spans.push(Span::styled(format!(" {date}"), style));
            }
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::ListState;

    use crate::model::TaskList;
    fn column(tasks: Vec<Task>) -> Column {
        let mut column = Column {
            list: TaskList {
                id: "list".into(),
                title: "Inbox".into(),
            },
            tasks,
            rows: Vec::new(),
            selected: None,
            last_index: 0,
            list_state: ListState::default(),
            pinned: None,
            loading: false,
            error: None,
        };
        column.rebuild_rows(false);
        column
    }

    fn render(terminal: &mut Terminal<TestBackend>, column: &mut Column) {
        terminal
            .draw(|frame| {
                draw_column(
                    frame,
                    frame.area(),
                    column,
                    true,
                    NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
                    (false, false),
                );
            })
            .unwrap();
    }

    #[test]
    fn long_task_description_wraps_inside_column() {
        let description = concat!(
            "Review the quarterly planning document and send feedback before Friday, ",
            "including the budget assumptions, staffing requirements, project milestones, ",
            "delivery risks, and follow-up actions for every team involved."
        );
        let mut task = Task::new_local("long".into(), description.into(), None);
        task.due = Some("2026-10-08T00:00:00.000Z".into());
        task.position = "000".into();
        let mut next = Task::new_local("next".into(), "Next task".into(), None);
        next.position = "001".into();
        let mut column = column(vec![task, next]);
        let mut terminal = Terminal::new(TestBackend::new(28, 24)).unwrap();
        render(&mut terminal, &mut column);

        let buffer = terminal.backend().buffer();
        let contents: Vec<String> = (1..23)
            .map(|y| {
                (1..27)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        assert_eq!(contents[0], "Review the quarterly 10-08");
        let next_row = contents
            .iter()
            .position(|line| line == "Next task")
            .unwrap();
        assert!(next_row >= 10, "expected at least ten wrapped lines");
        let rendered = contents[..next_row].join(" ").replace(" 10-08", "");
        assert_eq!(
            rendered.split_whitespace().collect::<Vec<_>>(),
            description.split_whitespace().collect::<Vec<_>>()
        );
        for y in 1..23 {
            assert_eq!(buffer[(0, y)].symbol(), "│");
            assert_eq!(buffer[(27, y)].symbol(), "│");
        }
        for y in 1..=next_row as u16 {
            assert!(buffer[(1, y)].modifier.contains(Modifier::REVERSED));
        }
        assert!(
            !buffer[(1, next_row as u16 + 1)]
                .modifier
                .contains(Modifier::REVERSED)
        );

        column.selected = Some("next".into());
        terminal.backend_mut().resize(28, 6);
        render(&mut terminal, &mut column);
        assert_eq!(column.list_state.offset(), 1);
        let buffer = terminal.backend().buffer();
        let selected: String = (1..27).map(|x| buffer[(x, 1)].symbol()).collect();
        assert_eq!(selected.trim_end(), "Next task");
        assert!(buffer[(1, 1)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn description_taller_than_viewport_stays_visible() {
        let task = Task::new_local(
            "long".into(),
            "Review the quarterly planning document and send feedback before Friday. ".repeat(20),
            None,
        );
        let mut column = column(vec![task]);
        let mut terminal = Terminal::new(TestBackend::new(28, 6)).unwrap();
        render(&mut terminal, &mut column);

        let buffer = terminal.backend().buffer();
        let first: String = (1..27).map(|x| buffer[(x, 1)].symbol()).collect();
        let last: String = (1..27).map(|x| buffer[(x, 4)].symbol()).collect();
        assert_eq!(first.trim_end(), "Review the quarterly");
        assert_eq!(last.trim_end(), "… (more in editor)");
        assert!(buffer[(1, 1)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buffer[(0, 4)].symbol(), "│");
        assert_eq!(buffer[(27, 4)].symbol(), "│");
    }

    #[test]
    fn wraps_unicode_and_unbroken_words_without_losing_text() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let row = Row {
            task_id: "task".into(),
            depth: 0,
            ghost: false,
        };
        for title in [
            "abcdefghijklmnopqrstuvwxyz0123456789",
            "日本語の長いタスクの説明をすべて表示する",
            "",
        ] {
            let task = Task::new_local(row.task_id.clone(), title.into(), None);
            let lines = row_lines(Some(&task), &row, 10, today);
            assert!(!lines.is_empty());
            assert!(lines.iter().all(|line| line.width() <= 10));
            let rendered: String = lines
                .iter()
                .map(|line| line.to_string().trim_end().to_string())
                .collect();
            assert_eq!(rendered, title);
        }
    }

    #[test]
    fn wrapped_subtask_keeps_indentation_date_and_style() {
        let mut task = Task::new_local(
            "child".into(),
            "Prepare the quarterly planning document and collect feedback".into(),
            Some("parent".into()),
        );
        task.due = Some("2026-10-06T00:00:00.000Z".into());
        let row = Row {
            task_id: task.id.clone(),
            depth: 1,
            ghost: true,
        };
        let lines = row_lines(
            Some(&task),
            &row,
            26,
            NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
        );
        assert!(lines.len() > 1);
        assert!(lines[0].to_string().starts_with("  └ Prepare"));
        assert!(lines[0].to_string().ends_with(" 10-06"));
        for line in &lines[1..] {
            assert!(line.to_string().starts_with("    "));
            assert!(!line.to_string().contains('└'));
            assert!(!line.to_string().contains("10-06"));
        }
        for line in &lines {
            assert!(line.width() <= 26);
            assert!(line.spans.iter().all(|span| {
                span.style.fg == Some(Color::Red) && span.style.add_modifier.contains(Modifier::DIM)
            }));
        }
    }
}
