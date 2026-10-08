use chrono::NaiveDate;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui_textarea::{CursorMove, TextArea};

use crate::dates::{self, DatePart};

const EMPTY_DATE: &str = "____-__-__";
const DIGITS: [usize; 8] = [0, 1, 2, 3, 5, 6, 8, 9];

pub struct DueInput {
    pub area: TextArea<'static>,
    today: NaiveDate,
}

impl DueInput {
    pub fn new(date: Option<NaiveDate>, today: NaiveDate) -> Self {
        let mut input = Self {
            area: TextArea::default(),
            today,
        };
        match date {
            Some(date) => input.set_text(&date.to_string(), 9),
            None => input.clear(),
        }
        input
    }

    fn set_text(&mut self, text: &str, cursor: usize) {
        self.area = TextArea::new(vec![text.to_string()]);
        self.area.set_cursor_line_style(Style::default());
        self.area.move_cursor(CursorMove::Jump(0, cursor as u16));
    }

    pub fn clear(&mut self) {
        self.set_text(EMPTY_DATE, 0);
    }

    fn is_masked(&self) -> bool {
        let text = &self.area.lines()[0];
        text.len() == 10
            && text.bytes().enumerate().all(|(i, c)| match i {
                4 | 7 => c == b'-',
                _ => c.is_ascii_digit() || c == b'_',
            })
    }

    pub fn text(&self) -> String {
        let text = self.area.lines()[0].trim();
        if text == EMPTY_DATE {
            String::new()
        } else {
            text.to_string()
        }
    }

    pub fn resolve(&self) -> Result<Option<NaiveDate>, String> {
        dates::parse_due_input(&self.text(), self.today)
    }

    fn adjust(&mut self, increase: bool, shift: bool) {
        let masked = self.is_masked();
        let cursor = if masked { self.area.cursor().1 } else { 9 };
        let date = match self.resolve() {
            Ok(None) => {
                self.set_text(&self.today.to_string(), 9);
                return;
            }
            Ok(Some(date)) => date,
            Err(_) => return,
        };
        let part = match cursor {
            0..=3 => DatePart::Year,
            4..=6 => DatePart::Month,
            _ => DatePart::Day,
        };
        let step = if shift && matches!(part, DatePart::Day) {
            7
        } else {
            1
        };
        if let Some(date) = dates::adjust_date(date, part, if increase { step } else { -step }) {
            self.set_text(&date.to_string(), cursor);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Up | KeyCode::Down) {
            self.adjust(
                key.code == KeyCode::Up,
                key.modifiers.contains(KeyModifiers::SHIFT),
            );
            return;
        }
        if !self.is_masked() {
            match key.code {
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Backspace
                | KeyCode::Delete => {
                    self.area.input(key);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && (c.is_ascii_alphanumeric() || matches!(c, ' ' | '+' | '-')) =>
                {
                    self.area.input(key);
                }
                _ => {}
            }
            if self.text().is_empty() {
                self.clear();
            }
            return;
        }

        let cursor = self.area.cursor().1.min(9);
        let next = || DIGITS.iter().copied().find(|&i| i > cursor).unwrap_or(9);
        let previous = || {
            DIGITS
                .iter()
                .copied()
                .rev()
                .find(|&i| i < cursor)
                .unwrap_or(0)
        };
        let mut text = self.area.lines()[0].clone();
        let cursor = match key.code {
            KeyCode::Left => previous(),
            KeyCode::Right => next(),
            KeyCode::Home => 0,
            KeyCode::End => 9,
            KeyCode::Backspace => {
                let target = previous();
                text.replace_range(target..target + 1, "_");
                target
            }
            KeyCode::Delete => {
                text.replace_range(cursor..cursor + 1, "_");
                cursor
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if c.is_ascii_digit() {
                    text.replace_range(cursor..cursor + 1, &c.to_string());
                    next()
                } else if c.is_ascii_alphabetic()
                    || c == '+'
                    || (c == '-' && self.text().is_empty())
                {
                    self.set_text(&c.to_string(), 1);
                    return;
                } else {
                    return;
                }
            }
            _ => return,
        };
        self.set_text(&text, cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn press(input: &mut DueInput, code: KeyCode) {
        input.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn type_text(input: &mut DueInput, text: &str) {
        for c in text.chars() {
            press(input, KeyCode::Char(c));
        }
    }

    #[test]
    fn empty_mask_is_not_a_date_and_first_adjustment_sets_today() {
        let today = date(2026, 10, 7);
        for code in [KeyCode::Up, KeyCode::Down] {
            let mut input = DueInput::new(None, today);
            assert_eq!(input.area.lines()[0], EMPTY_DATE);
            assert_eq!(input.resolve(), Ok(None));
            press(&mut input, code);
            assert_eq!(input.resolve(), Ok(Some(today)));
            press(&mut input, code);
            assert_eq!(
                input.resolve(),
                Ok(Some(date(
                    2026,
                    10,
                    if code == KeyCode::Up { 8 } else { 6 }
                )))
            );
            input.clear();
            assert_eq!(input.area.lines()[0], EMPTY_DATE);
            assert_eq!(input.resolve(), Ok(None));
        }
    }

    #[test]
    fn cursor_skips_both_separators_in_both_directions() {
        let mut input = DueInput::new(Some(date(2026, 12, 24)), date(2026, 10, 7));
        press(&mut input, KeyCode::Home);
        for cursor in [1, 2, 3, 5, 6, 8, 9, 9] {
            press(&mut input, KeyCode::Right);
            assert_eq!(input.area.cursor().1, cursor);
        }
        for cursor in [8, 6, 5, 3, 2, 1, 0, 0] {
            press(&mut input, KeyCode::Left);
            assert_eq!(input.area.cursor().1, cursor);
        }
    }

    #[test]
    fn digits_overwrite_and_deletion_preserves_the_mask() {
        let mut input = DueInput::new(None, date(2026, 10, 7));
        type_text(&mut input, "2026-12-24");
        assert_eq!(input.text(), "2026-12-24");
        press(&mut input, KeyCode::Home);
        type_text(&mut input, "2027");
        assert_eq!(input.text(), "2027-12-24");
        assert_eq!(input.area.cursor().1, 5);
        press(&mut input, KeyCode::Backspace);
        assert_eq!(input.text(), "202_-12-24");
        assert_eq!(input.area.cursor().1, 3);
        press(&mut input, KeyCode::Right);
        press(&mut input, KeyCode::Delete);
        assert_eq!(input.text(), "202_-_2-24");
        assert!(input.resolve().is_err());
        press(&mut input, KeyCode::Char('-'));
        assert_eq!(input.text(), "202_-_2-24");
    }

    #[test]
    fn arrows_adjust_the_cursor_segment_and_keep_the_cursor() {
        let original = date(2026, 12, 24);
        for (cursor, up, down) in [
            (3, date(2027, 12, 24), date(2025, 12, 24)),
            (5, date(2027, 1, 24), date(2026, 11, 24)),
            (8, date(2026, 12, 25), date(2026, 12, 23)),
        ] {
            for (code, expected) in [(KeyCode::Up, up), (KeyCode::Down, down)] {
                let mut input = DueInput::new(Some(original), date(2026, 10, 7));
                input.area.move_cursor(CursorMove::Jump(0, cursor));
                press(&mut input, code);
                assert_eq!(input.resolve(), Ok(Some(expected)));
                assert_eq!(input.area.cursor().1, cursor as usize);
            }
        }
    }

    #[test]
    fn shorthand_replaces_a_numeric_date_and_can_be_adjusted() {
        let mut input = DueInput::new(Some(date(2026, 12, 24)), date(2026, 10, 7));
        type_text(&mut input, "next mon");
        assert_eq!(input.resolve(), Ok(Some(date(2026, 10, 12))));
        press(&mut input, KeyCode::Up);
        assert_eq!(input.text(), "2026-10-13");
        assert_eq!(input.area.cursor().1, 9);
        input.clear();
        type_text(&mut input, "+2w");
        assert_eq!(input.resolve(), Ok(Some(date(2026, 10, 21))));
        input.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
        assert_eq!(input.text(), "2026-10-14");
    }

    #[test]
    fn invalid_shorthand_is_not_silently_replaced_by_arrows() {
        let mut input = DueInput::new(None, date(2026, 10, 7));
        type_text(&mut input, "tomorr");
        press(&mut input, KeyCode::Up);
        assert_eq!(input.text(), "tomorr");
        assert!(input.resolve().is_err());
        type_text(&mut input, "ow");
        assert_eq!(input.resolve(), Ok(Some(date(2026, 10, 8))));
    }
}
