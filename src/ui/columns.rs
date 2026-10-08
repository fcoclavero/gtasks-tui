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
            if !focused && !task.is_some_and(|task| dates::is_overdue(task, today)) {
                let style = if column.selected.as_deref() == Some(row.task_id.as_str()) {
                    Style::new().fg(Color::White)
                } else {
                    border_style.fg(Color::Reset)
                };
                for span in lines.iter_mut().flat_map(|line| line.spans.iter_mut()) {
                    span.style = span.style.patch(style);
                }
            }
            // Ratatui skips items taller than the viewport entirely.
            if lines.len() > inner.height as usize {
                lines.truncate(inner.height as usize);
                if inner.height > 1 {
                    let last = lines.last_mut().unwrap();
                    let indent = " ".repeat(row_marker(row).width());
                    let metadata_cells: usize =
                        last.spans.iter().skip(1).map(|span| span.width()).sum();
                    last.spans[0] = Span::styled(
                        format!(
                            "{indent}{}",
                            fit(
                                "… (more in editor)",
                                (inner.width as usize)
                                    .saturating_sub(indent.width() + metadata_cells)
                            )
                        ),
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
        Style::new().add_modifier(Modifier::BOLD)
    };
    let list = List::new(items).highlight_style(highlight);
    column.list_state.select(column.selected_index());
    frame.render_stateful_widget(list, inner, &mut column.list_state);
}

fn row_marker(row: &Row) -> &'static str {
    if row.depth > 0 { "  └ " } else { "• " }
}

fn row_lines(task: Option<&Task>, row: &Row, width: usize, today: NaiveDate) -> Vec<Line<'static>> {
    let Some(task) = task else {
        return vec![Line::from(fit("?", width))];
    };
    let indent = row_marker(row);
    let date = dates::due_date(task.due.as_deref())
        .map(|d| dates::format_short(d, today))
        .unwrap_or_default();
    let mut metadata = Vec::new();
    if !date.is_empty() {
        metadata.push(date.as_str());
    }
    if task
        .notes
        .as_deref()
        .is_some_and(|notes| !notes.trim().is_empty())
    {
        metadata.push("≡");
    }
    let metadata_width = metadata
        .iter()
        .map(|label| label.width())
        .max()
        .unwrap_or(0);
    let metadata_cells = if metadata_width == 0 {
        0
    } else {
        metadata_width + 1
    };
    let title_width = width.saturating_sub(indent.width() + metadata_cells);

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
    let mut title_lines = textwrap::wrap(&task.title, title_width.max(1));
    title_lines.resize(title_lines.len().max(metadata.len()), "".into());
    title_lines
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
            if let Some(label) = metadata.get(index) {
                spans.push(Span::styled(
                    format!("{}{label}", " ".repeat(metadata_width + 1 - label.width())),
                    style,
                ));
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

    use crate::model::{TaskList, TaskStatus};
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
        column.rebuild_rows(false, crate::tree::SortMode::Position);
        column
    }

    fn render(terminal: &mut Terminal<TestBackend>, column: &mut Column) {
        render_with_focus(terminal, column, true);
    }

    fn render_with_focus(terminal: &mut Terminal<TestBackend>, column: &mut Column, focused: bool) {
        terminal
            .draw(|frame| {
                draw_column(
                    frame,
                    frame.area(),
                    column,
                    focused,
                    NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
                    (false, false),
                );
            })
            .unwrap();
    }

    #[test]
    fn short_tasks_align_details_and_dates_without_overlapping_next_task() {
        for (has_date, has_details, expected) in [
            (false, false, vec!["• Call".to_string()]),
            (true, false, vec![format!("• {:<18} 10-08", "Call")]),
            (false, true, vec![format!("• {:<22} ≡", "Call")]),
            (
                true,
                true,
                vec![
                    format!("• {:<18} 10-08", "Call"),
                    format!("{}≡", " ".repeat(25)),
                ],
            ),
        ] {
            let mut task = Task::new_local("task".into(), "Call".into(), None);
            task.position = "000".into();
            if has_date {
                task.due = Some("2026-10-08T00:00:00.000Z".into());
            }
            if has_details {
                task.notes = Some("Discuss the planning document".into());
            }
            let mut next = Task::new_local("next".into(), "Next task".into(), None);
            next.position = "001".into();
            let mut column = column(vec![task, next]);
            let mut terminal = Terminal::new(TestBackend::new(28, 8)).unwrap();
            render(&mut terminal, &mut column);

            let buffer = terminal.backend().buffer();
            for (index, expected_line) in expected.iter().enumerate() {
                let y = index as u16 + 1;
                let actual: String = (1..27).map(|x| buffer[(x, y)].symbol()).collect();
                assert_eq!(
                    actual.trim_end(),
                    expected_line,
                    "date={has_date}, details={has_details}, row={index}"
                );
                assert!(buffer[(26, y)].modifier.contains(Modifier::REVERSED));
                assert_eq!(buffer[(0, y)].symbol(), "│");
                assert_eq!(buffer[(27, y)].symbol(), "│");
            }
            let next_y = expected.len() as u16 + 1;
            let actual: String = (1..27).map(|x| buffer[(x, next_y)].symbol()).collect();
            assert_eq!(actual.trim_end(), "• Next task");
            assert!(!buffer[(1, next_y)].modifier.contains(Modifier::REVERSED));
        }
    }

    #[test]
    fn empty_details_do_not_show_a_marker_or_reserve_space() {
        let row = Row {
            task_id: "task".into(),
            depth: 0,
            ghost: false,
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        for due in [None, Some("2026-10-08T00:00:00.000Z")] {
            let mut task = Task::new_local(row.task_id.clone(), "Call".into(), None);
            task.due = due.map(str::to_string);
            let expected = row_lines(Some(&task), &row, 26, today);
            for notes in ["", " \t\n", "\u{2003}"] {
                task.notes = Some(notes.into());
                let lines = row_lines(Some(&task), &row, 26, today);
                assert_eq!(lines, expected, "due={due:?}, notes={notes:?}");
            }
        }
    }

    #[test]
    fn titles_wrap_before_dates_details_or_both_without_losing_text() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        for due in [
            None,
            Some(("2026-10-08T00:00:00.000Z", "10-08")),
            Some(("2027-01-02T00:00:00.000Z", "2027-01-02")),
        ] {
            for has_details in [false, true] {
                for depth in [0, 1] {
                    let row = Row {
                        task_id: "task".into(),
                        depth,
                        ghost: false,
                    };
                    let indent_width = row_marker(&row).width();
                    let metadata_width = due
                        .map(|(_, date)| date.width())
                        .unwrap_or(usize::from(has_details));
                    let reserved = if metadata_width == 0 {
                        0
                    } else {
                        metadata_width + 1
                    };
                    for width in [20, 26, 40] {
                        for title in [
                            "Review the quarterly planning document and send feedback",
                            "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                            "日本語の長いタスクの説明をすべて表示する",
                            "",
                        ] {
                            let mut task = Task::new_local(row.task_id.clone(), title.into(), None);
                            task.due = due.map(|(value, _)| value.to_string());
                            if has_details {
                                task.notes = Some(" \nPlanning notes\n ".into());
                            }
                            let lines = row_lines(Some(&task), &row, width, today);
                            assert!(!lines.is_empty());
                            let marker_row = usize::from(due.is_some());
                            let expected_height =
                                textwrap::wrap(title, width - indent_width - reserved)
                                    .len()
                                    .max(if has_details { marker_row + 1 } else { 1 });
                            assert_eq!(lines.len(), expected_height);
                            let mut rendered_title = String::new();
                            for (index, line) in lines.iter().enumerate() {
                                assert!(line.width() <= width);
                                let title_span = &line.spans[0];
                                assert_eq!(title_span.width(), width - reserved);
                                let prefix = if index == 0 {
                                    row_marker(&row).to_string()
                                } else {
                                    " ".repeat(indent_width)
                                };
                                rendered_title.push_str(
                                    title_span.content.strip_prefix(&prefix).unwrap().trim_end(),
                                );
                                let metadata = line
                                    .spans
                                    .iter()
                                    .skip(1)
                                    .map(|span| span.content.as_ref())
                                    .collect::<String>();
                                if index == 0
                                    && let Some((_, date)) = due
                                {
                                    assert_eq!(metadata, format!(" {date}"));
                                    assert_eq!(line.width(), width);
                                } else if has_details && index == marker_row {
                                    assert_eq!(
                                        metadata,
                                        format!("{}≡", " ".repeat(metadata_width))
                                    );
                                    assert_eq!(line.width(), width);
                                } else {
                                    assert!(metadata.is_empty());
                                }
                            }
                            assert_eq!(
                                rendered_title
                                    .chars()
                                    .filter(|c| !c.is_whitespace())
                                    .collect::<String>(),
                                title
                                    .chars()
                                    .filter(|c| !c.is_whitespace())
                                    .collect::<String>()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn viewport_clipping_keeps_details_marker_below_date() {
        let mut task = Task::new_local(
            "task".into(),
            "Review the quarterly planning document and send feedback. ".repeat(20),
            None,
        );
        task.due = Some("2026-10-08T00:00:00.000Z".into());
        task.notes = Some("Planning notes".into());
        let mut column = column(vec![task]);
        let mut terminal = Terminal::new(TestBackend::new(28, 4)).unwrap();
        render(&mut terminal, &mut column);

        let buffer = terminal.backend().buffer();
        let first: String = (1..27).map(|x| buffer[(x, 1)].symbol()).collect();
        let last: String = (1..27).map(|x| buffer[(x, 2)].symbol()).collect();
        assert_eq!(first, "• Review the         10-08");
        assert_eq!(last, format!("  {:<18}     ≡", "… (more in editor)"));
        assert!(buffer[(26, 2)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buffer[(27, 2)].symbol(), "│");
    }

    #[test]
    fn selection_style_tracks_focus_without_changing_overdue_color() {
        let mut task = Task::new_local(
            "task".into(),
            "Review the quarterly planning document".into(),
            None,
        );
        task.due = Some("2026-10-06T00:00:00.000Z".into());
        task.notes = Some("Planning notes".into());
        let mut column = column(vec![task]);
        let mut terminal = Terminal::new(TestBackend::new(28, 10)).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let task_height = row_lines(column.task("task"), &column.rows[0], 26, today).len();
        assert!(task_height > 1);

        for focused in [true, false, true] {
            render_with_focus(&mut terminal, &mut column, focused);
            let expected = if focused {
                Modifier::REVERSED
            } else {
                Modifier::BOLD
            };
            let buffer = terminal.backend().buffer();
            for y in 1..=task_height as u16 {
                let cell = &buffer[(3, y)];
                assert_eq!(cell.modifier, expected, "focused={focused}, line={y}");
                assert_eq!(cell.fg, Color::Red);
            }
            assert_eq!(buffer[(1, 1)].symbol(), "•");
            assert_eq!(column.selected.as_deref(), Some("task"));
            assert_eq!(column.list_state.selected(), Some(0));
        }
    }

    #[test]
    fn unfocused_list_keeps_selection_white_and_overdue_tasks_red() {
        let mut normal = Task::new_local(
            "normal".into(),
            "Review the quarterly planning document".into(),
            None,
        );
        normal.position = "000".into();
        normal.due = Some("2026-10-08T00:00:00.000Z".into());
        normal.notes = Some("Planning notes".into());
        let mut overdue = Task::new_local(
            "overdue".into(),
            "Send the outstanding feedback to the team".into(),
            None,
        );
        overdue.position = "001".into();
        overdue.due = Some("2026-10-06T00:00:00.000Z".into());
        overdue.notes = Some("Feedback notes".into());
        let mut completed = Task::new_local("completed".into(), "Call the dentist".into(), None);
        completed.position = "002".into();
        completed.status = TaskStatus::Completed;
        completed.notes = Some("Appointment notes".into());
        let mut column = column(vec![normal, overdue, completed]);
        column.rebuild_rows(true, crate::tree::SortMode::Position);
        let mut terminal = Terminal::new(TestBackend::new(28, 24)).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();

        for focused in [false, true] {
            for selected in ["normal", "overdue", "completed"] {
                column.selected = Some(selected.into());
                render_with_focus(&mut terminal, &mut column, focused);
                let buffer = terminal.backend().buffer();
                let mut y = 1;
                for row in &column.rows {
                    let task = column.task(&row.task_id).unwrap();
                    let is_selected = task.id == selected;
                    let color = if task.id == "overdue" {
                        Color::Red
                    } else if focused && task.is_completed() {
                        Color::Green
                    } else if focused || is_selected {
                        Color::White
                    } else {
                        Color::Reset
                    };
                    let modifier = if !focused && !is_selected && task.id != "overdue" {
                        Modifier::DIM
                    } else if !is_selected {
                        Modifier::empty()
                    } else if focused {
                        Modifier::REVERSED
                    } else {
                        Modifier::BOLD
                    };
                    if task.due.is_some() {
                        assert_eq!(buffer[(26, y)].fg, color);
                        assert_eq!(buffer[(26, y)].modifier, modifier);
                    }
                    let marker_y = y + u16::from(task.due.is_some());
                    assert_eq!(buffer[(26, marker_y)].symbol(), "≡");
                    assert_eq!(buffer[(26, marker_y)].fg, color);
                    assert_eq!(buffer[(26, marker_y)].modifier, modifier);
                    let height = row_lines(Some(task), row, 26, today).len() as u16;
                    for line_y in y..y + height {
                        for x in [1, 3] {
                            let cell = &buffer[(x, line_y)];
                            assert_eq!(
                                cell.fg, color,
                                "task={}, selected={selected}, focused={focused}, line={line_y}",
                                task.id
                            );
                            assert_eq!(cell.modifier, modifier);
                            if !focused && !is_selected && task.id != "overdue" {
                                for border_or_title in [(0, line_y), (3, 0)] {
                                    let reference = &buffer[border_or_title];
                                    assert_eq!(cell.fg, reference.fg);
                                    assert_eq!(cell.modifier, reference.modifier);
                                }
                            }
                        }
                    }
                    y += height;
                }
                assert_eq!(column.selected.as_deref(), Some(selected));
            }
        }
    }

    #[test]
    fn task_start_markers_distinguish_wrapped_lines_and_subtasks() {
        let mut parent = Task::new_local(
            "parent".into(),
            "Review the quarterly planning document and send feedback".into(),
            None,
        );
        parent.position = "000".into();
        let child = Task::new_local(
            "child".into(),
            "Collect estimates".into(),
            Some(parent.id.clone()),
        );
        let mut next = Task::new_local("next".into(), "Call the dentist".into(), None);
        next.position = "001".into();
        let mut column = column(vec![parent, child, next]);
        let mut terminal = Terminal::new(TestBackend::new(28, 10)).unwrap();
        render(&mut terminal, &mut column);

        let buffer = terminal.backend().buffer();
        let contents: Vec<String> = (1..=5)
            .map(|y| {
                (1..27)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        assert_eq!(
            contents,
            [
                "• Review the quarterly",
                "  planning document and",
                "  send feedback",
                "  └ Collect estimates",
                "• Call the dentist",
            ]
        );
        for y in 1..=3 {
            assert!(buffer[(1, y)].modifier.contains(Modifier::REVERSED));
            assert_eq!(buffer[(0, y)].symbol(), "│");
            assert_eq!(buffer[(27, y)].symbol(), "│");
        }
        assert!(!buffer[(1, 4)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn task_start_marker_keeps_short_and_empty_titles_on_one_line() {
        let row = Row {
            task_id: "task".into(),
            depth: 0,
            ghost: false,
        };
        for title in ["Call", "abcdef", ""] {
            let task = Task::new_local(row.task_id.clone(), title.into(), None);
            let lines = row_lines(
                Some(&task),
                &row,
                8,
                NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
            );
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].width(), 8);
            assert_eq!(
                lines[0].to_string().trim_end(),
                format!("• {title}").trim_end()
            );
        }
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
        assert_eq!(contents[0], "• Review the         10-08");
        let next_row = contents
            .iter()
            .position(|line| line == "• Next task")
            .unwrap();
        assert!(next_row >= 10, "expected at least ten wrapped lines");
        let rendered = contents[..next_row]
            .iter()
            .map(|line| line.strip_prefix("• ").unwrap_or(line).trim_start())
            .collect::<Vec<_>>()
            .join(" ")
            .replace(" 10-08", "");
        for line in &contents[1..next_row] {
            assert!(line.starts_with("  "));
        }
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
        assert_eq!(selected.trim_end(), "• Next task");
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
        assert_eq!(first.trim_end(), "• Review the quarterly");
        assert_eq!(last.trim_end(), "  … (more in editor)");
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
                .map(|line| {
                    line.to_string()
                        .chars()
                        .skip(2)
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
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
        task.notes = Some("Planning notes".into());
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
        assert!(lines[1].to_string().ends_with('≡'));
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
