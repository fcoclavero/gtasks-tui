use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
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
    let [fields_area, bottom_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas(inner);
    let [footer_area, task_link_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(bottom_area);
    let [title_area, due_area, notes_area, links_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(4),
        Constraint::Min(4),
        Constraint::Length(links_rows),
    ])
    .areas(fields_area);

    let focus = details.focus;
    let error_field = details.error.as_ref().map(|e| e.0);
    style_field(
        &mut details.title,
        " Title ",
        focus == Field::Title,
        error_field == Some(Field::Title),
    );
    style_field(
        &mut details.due.area,
        " Due ",
        focus == Field::Due,
        error_field == Some(Field::Due),
    );
    style_field(&mut details.notes, " Notes ", focus == Field::Notes, false);
    frame.render_widget(&details.title, title_area);
    let [due_input_area, preview_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).areas(due_area);
    frame.render_widget(&details.due.area, due_input_area);
    let preview = match details.due.resolve() {
        Ok(Some(date)) => {
            Line::from(format!(" → {}", date.format("%a, %b %-d, %Y (%Y-%m-%d)"))).cyan()
        }
        Ok(None) => Line::from(" No due date · ↑/↓ starts at today").dim(),
        Err(_) => Line::from(format!(
            " Invalid date: {} · try today, fri, +3d",
            details.due_text()
        ))
        .yellow(),
    };
    frame.render_widget(Paragraph::new(preview), preview_area);
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
    } else if focus == Field::Due {
        Line::from("←/→ segment · ↑/↓ adjust · Shift ↑/↓ week (day) · Ctrl-U clear · Ctrl-S save")
            .dim()
    } else {
        Line::from(
            "Tab/Shift-Tab fields · Enter next · Ctrl-S save · Ctrl-U clear due · Esc cancel",
        )
        .dim()
    };
    frame.render_widget(Paragraph::new(footer), footer_area);

    details.task_link_area = Rect::default();
    let task_link = if details.task_url().is_some() {
        let label = "Open task in browser";
        details.task_link_area = Rect::new(
            task_link_area.x,
            task_link_area.y,
            task_link_area.width.min(label.len() as u16),
            task_link_area.height,
        );
        Line::from(vec![
            Span::raw(label).cyan().underlined(),
            Span::raw(" (click or Ctrl-O)").dim(),
        ])
    } else {
        Line::from("Task link unavailable").dim()
    };
    frame.render_widget(Paragraph::new(task_link), task_link_area);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::details::DetailsAction;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_textarea::CursorMove;

    use crate::model::Task;

    #[test]
    fn due_preview_updates_and_cursor_never_highlights_a_dash() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let task = Task::new_local("task".into(), "Planning".into(), None);
        let mut details = DetailsState::new("list".into(), &task);
        details.due = crate::due_input::DueInput::new(None, today);
        details.focus = Field::Due;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw(frame, frame.area(), &mut details))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("____-__-__"));
        assert!(text.contains("No due date"));
        assert!(!details.is_dirty());

        for c in "tomorrow".chars() {
            details.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        for width in [80, 44, 80] {
            terminal.backend_mut().resize(width, 24);
            terminal
                .draw(|frame| draw(frame, frame.area(), &mut details))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
            assert!(text.contains("tomorrow"));
            assert!(text.contains("Thu, Oct 8, 2026 (2026-10-08)"));
        }
        assert_eq!(
            details.build_patch().unwrap().due,
            Some(Some(today.succ_opt().unwrap()))
        );

        details.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        details.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        for _ in 0..10 {
            terminal
                .draw(|frame| draw(frame, frame.area(), &mut details))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let cursor_cells: Vec<_> = buffer
                .content
                .iter()
                .filter(|c| c.modifier.contains(Modifier::REVERSED))
                .collect();
            assert_eq!(cursor_cells.len(), 1);
            assert!(cursor_cells[0].symbol().bytes().all(|b| b.is_ascii_digit()));
            assert!(details.due.area.cursor().1 != 4 && details.due.area.cursor().1 != 7);
            details.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        }
        details.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        terminal
            .draw(|frame| draw(frame, frame.area(), &mut details))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Invalid date: x"));
    }

    #[test]
    fn task_link_is_clickable_on_the_bottom_row_and_updates_on_resize() {
        let mut task = Task::new_local("task".into(), "Planning".into(), None);
        task.web_view_link = Some("https://tasks.google.com/task/task".into());
        let mut details = DetailsState::new("list".into(), &task);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        for (width, height) in [(80, 24), (44, 18), (12, 8), (80, 24)] {
            terminal.backend_mut().resize(width, height);
            terminal
                .draw(|frame| draw(frame, frame.area(), &mut details))
                .unwrap();
            let area = details.task_link_area;
            assert!(area.width > 0 && area.height == 1);
            let buffer = terminal.backend().buffer();
            let label: String = (area.x..area.right())
                .map(|x| buffer[(x, area.y)].symbol())
                .collect();
            assert!("Open task in browser".starts_with(&label));
            assert_eq!(buffer[(area.x, area.y)].fg, Color::Cyan);
            assert!(
                buffer[(area.x, area.y)]
                    .modifier
                    .contains(Modifier::UNDERLINED)
            );
            assert_eq!(buffer[(area.x, area.bottom())].symbol(), "─");
        }

        details.original.web_view_link = None;
        terminal
            .draw(|frame| draw(frame, frame.area(), &mut details))
            .unwrap();
        assert_eq!(details.task_link_area, Rect::default());
        let buffer = terminal.backend().buffer();
        assert!(
            buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("Task link unavailable")
        );
    }

    fn render_notes(
        terminal: &mut Terminal<TestBackend>,
        details: &mut DetailsState,
    ) -> (Rect, Vec<String>) {
        let mut notes_inner = Rect::default();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let popup = centered(
                    area,
                    (area.width * 9 / 10).clamp(40, 100),
                    (area.height * 9 / 10).clamp(14, 36),
                );
                let inner = Block::bordered().inner(popup);
                let links_rows = if details.original.links.is_empty() {
                    0
                } else {
                    details.original.links.len().min(4) as u16 + 2
                };
                let [fields_area, _] =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas(inner);
                let [_, _, notes_area, _] = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Length(4),
                    Constraint::Min(4),
                    Constraint::Length(links_rows),
                ])
                .areas(fields_area);
                notes_inner = Block::bordered().inner(notes_area);
                draw(frame, area, details);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines = (notes_inner.y..notes_inner.bottom())
            .map(|y| {
                (notes_inner.x..notes_inner.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        for y in notes_inner.y..notes_inner.bottom() {
            assert_eq!(buffer[(notes_inner.x - 1, y)].symbol(), "│");
            assert_eq!(buffer[(notes_inner.right(), y)].symbol(), "│");
        }
        (notes_inner, lines)
    }

    #[test]
    fn long_notes_wrap_inside_details_without_changing_saved_text() {
        let notes = concat!(
            "Review the quarterly planning document and send feedback before Friday, ",
            "including the budget assumptions, staffing requirements, project milestones, ",
            "delivery risks, and follow-up actions for every team involved. ",
            "Document the dependencies between teams, confirm the rollout schedule, ",
            "record unresolved questions, and agree on owners for every action item. ",
            "Include the proposed review dates and communication plan for stakeholders.",
            "\n\nFollow-up: share the final summary with the team."
        );
        let mut task = Task::new_local("task".into(), "Planning".into(), None);
        task.notes = Some(notes.into());
        let mut details = DetailsState::new("list".into(), &task);
        details.notes.move_cursor(CursorMove::Jump(0, 0));
        let mut terminal = Terminal::new(TestBackend::new(44, 40)).unwrap();

        for focus in [Field::Title, Field::Notes] {
            details.focus = focus;
            let (_, lines) = render_notes(&mut terminal, &mut details);
            assert_eq!(lines[0], "Review the quarterly planning");
            assert!(
                lines.iter().filter(|line| !line.is_empty()).count() >= 10,
                "expected notes to occupy at least ten wrapped lines: {lines:?}"
            );
            assert_eq!(
                lines.join(" ").split_whitespace().collect::<Vec<_>>(),
                notes.split_whitespace().collect::<Vec<_>>()
            );
            assert_eq!(details.notes_text(), notes);
            assert!(!details.is_dirty());
            assert!(details.build_patch().unwrap().is_empty());
        }
    }

    #[test]
    fn wrapped_notes_scroll_vertically_and_keep_cursor_visible() {
        let notes = format!(
            "Start of notes. {}END OF NOTES.",
            "Review the planning document and record every follow-up action. ".repeat(30)
        );
        let mut task = Task::new_local("task".into(), "Planning".into(), None);
        task.notes = Some(notes.clone());
        let mut details = DetailsState::new("list".into(), &task);
        details.focus = Field::Notes;
        let mut terminal = Terminal::new(TestBackend::new(44, 18)).unwrap();

        let (_, lines) = render_notes(&mut terminal, &mut details);
        assert!(lines.join(" ").contains("END OF NOTES."));
        assert!(!lines.join(" ").contains("Start of notes."));

        details.notes.move_cursor(CursorMove::Jump(0, 0));
        let (_, lines) = render_notes(&mut terminal, &mut details);
        assert!(lines[0].starts_with("Start of notes."));
        assert!(!lines.join(" ").contains("END OF NOTES."));

        let mut reached_end = false;
        for _ in 0..100 {
            let previous = details.notes.cursor();
            details.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            assert_eq!(details.notes.cursor().0, 0);
            assert!(details.notes.cursor().1 > previous.1);
            let (inner, lines) = render_notes(&mut terminal, &mut details);
            let buffer = terminal.backend().buffer();
            let cursor_cells = (inner.y..inner.bottom())
                .flat_map(|y| (inner.x..inner.right()).map(move |x| (x, y)))
                .filter(|&position| buffer[position].modifier.contains(Modifier::REVERSED))
                .count();
            assert_eq!(cursor_cells, 1);
            if lines.join(" ").contains("END OF NOTES.") {
                reached_end = true;
                break;
            }
        }
        assert!(reached_end, "could not scroll to the end of wrapped notes");
        let previous = details.notes.cursor();
        details.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(details.notes.cursor().1 < previous.1);
        assert_eq!(details.notes_text(), notes);
        assert!(!details.is_dirty());
    }

    #[test]
    fn editing_wrapped_notes_preserves_logical_lines_and_undo() {
        let notes = concat!(
            "Review the planning document and record every follow-up action ",
            "before sharing the summary with all stakeholders."
        );
        let mut task = Task::new_local("task".into(), "Planning".into(), None);
        task.notes = Some(notes.into());
        let mut details = DetailsState::new("list".into(), &task);
        details.focus = Field::Notes;
        details.notes.move_cursor(CursorMove::Jump(0, 0));
        let mut terminal = Terminal::new(TestBackend::new(44, 24)).unwrap();
        render_notes(&mut terminal, &mut details);

        details.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let cursor = details.notes.cursor();
        let col = cursor.1;
        assert_eq!(cursor.0, 0);
        assert!(col > 0, "Down should move onto a wrapped visual line");
        details.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
        let expected = format!("{}!{}", &notes[..col], &notes[col..]);
        render_notes(&mut terminal, &mut details);
        assert_eq!(details.notes.lines().len(), 1);
        assert_eq!(details.notes_text(), expected);
        match details.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)) {
            DetailsAction::Save(patch) => assert_eq!(patch.notes, Some(expected)),
            other => panic!("unexpected action: {other:?}"),
        }
        assert!(details.notes.undo());
        assert_eq!(details.notes_text(), notes);
        assert!(!details.is_dirty());

        details.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        render_notes(&mut terminal, &mut details);
        assert_eq!(details.notes.lines().len(), 2);
        assert_eq!(
            details.notes_text(),
            format!("{}\n{}", &notes[..col], &notes[col..])
        );
        assert!(details.notes.undo());
        assert_eq!(details.notes_text(), notes);
    }

    #[test]
    fn wrapped_notes_reflow_on_resize_without_losing_unicode_or_long_words() {
        for notes in [
            "abcdefghijklmnopqrstuvwxyz0123456789".repeat(4),
            "日本語の長いメモをすべて表示する".repeat(4),
            "e\u{301}".repeat(100),
        ] {
            let mut task = Task::new_local("task".into(), "Planning".into(), None);
            task.notes = Some(notes.clone());
            let mut details = DetailsState::new("list".into(), &task);
            details.focus = Field::Notes;
            let mut terminal = Terminal::new(TestBackend::new(44, 40)).unwrap();
            let mut narrow_height = 0;
            for width in [44, 80, 44] {
                terminal.backend_mut().resize(width, 40);
                let (_, lines) = render_notes(&mut terminal, &mut details);
                assert_eq!(
                    lines
                        .join("")
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>(),
                    notes
                );
                let height = lines.iter().filter(|line| !line.is_empty()).count();
                if width == 44 {
                    assert!(height >= 3);
                    if narrow_height > 0 {
                        assert_eq!(height, narrow_height);
                    }
                    narrow_height = height;
                } else {
                    assert!(height < narrow_height);
                }
                assert_eq!(details.notes_text(), notes);
                assert!(!details.is_dirty());
            }
        }
    }
}
