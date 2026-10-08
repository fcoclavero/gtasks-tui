//! Date helpers. Google stores `due` as a date-only RFC 3339 timestamp at
//! midnight UTC, so it must never be converted through a timezone.

use chrono::{Datelike, Days, Local, Months, NaiveDate, SecondsFormat, Utc};

use crate::model::Task;

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Resolve editor input against a fixed local date. Empty input clears the date.
pub fn parse_due_input(input: &str, today: NaiveDate) -> Result<Option<NaiveDate>, String> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Ok(None);
    }
    let date = match s.as_str() {
        "today" => Some(today),
        "tomorrow" => today.checked_add_days(Days::new(1)),
        _ if s.starts_with('+') || s.starts_with('-') => {
            let relative = &s[1..];
            let days = if let Some(amount) = relative.strip_suffix('d') {
                amount.parse::<u64>().ok()
            } else if let Some(amount) = relative.strip_suffix('w') {
                amount.parse::<u64>().ok().and_then(|n| n.checked_mul(7))
            } else {
                None
            };
            days.and_then(|n| {
                if s.starts_with('+') {
                    today.checked_add_days(Days::new(n))
                } else {
                    today.checked_sub_days(Days::new(n))
                }
            })
        }
        _ => {
            let (weekday, strictly_future) = s
                .strip_prefix("next ")
                .map_or((s.as_str(), false), |day| (day, true));
            let weekday = match weekday {
                "mon" | "monday" => Some(0),
                "tue" | "tuesday" => Some(1),
                "wed" | "wednesday" => Some(2),
                "thu" | "thursday" => Some(3),
                "fri" | "friday" => Some(4),
                "sat" | "saturday" => Some(5),
                "sun" | "sunday" => Some(6),
                _ => None,
            };
            if let Some(weekday) = weekday {
                let mut days = (weekday + 7 - today.weekday().num_days_from_monday()) % 7;
                if strictly_future && days == 0 {
                    days = 7;
                }
                today.checked_add_days(Days::new(u64::from(days)))
            } else if s.len() == 10
                && s.bytes().enumerate().all(|(i, c)| {
                    if i == 4 || i == 7 {
                        c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                })
            {
                NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()
            } else {
                None
            }
        }
    };
    date.filter(|date| (1..=9999).contains(&date.year()))
        .map(Some)
        .ok_or_else(|| "Use YYYY-MM-DD, today, tomorrow, fri, next mon, +3d, or +2w".to_string())
}

#[derive(Clone, Copy)]
pub enum DatePart {
    Year,
    Month,
    Day,
}

pub fn adjust_date(date: NaiveDate, part: DatePart, amount: i32) -> Option<NaiveDate> {
    let magnitude = amount.unsigned_abs();
    let adjusted = match part {
        DatePart::Year | DatePart::Month => {
            let months = match part {
                DatePart::Year => magnitude.checked_mul(12)?,
                _ => magnitude,
            };
            if amount >= 0 {
                date.checked_add_months(Months::new(months))
            } else {
                date.checked_sub_months(Months::new(months))
            }
        }
        DatePart::Day => {
            if amount >= 0 {
                date.checked_add_days(Days::new(u64::from(magnitude)))
            } else {
                date.checked_sub_days(Days::new(u64::from(magnitude)))
            }
        }
    };
    adjusted.filter(|date| (1..=9999).contains(&date.year()))
}

/// Date part of an API `due` value, ignoring any time component.
pub fn due_date(due: Option<&str>) -> Option<NaiveDate> {
    let date_part = due?.get(..10)?;
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

pub fn to_api_due(date: NaiveDate) -> String {
    format!("{date}T00:00:00.000Z")
}

pub fn is_overdue(task: &Task, today: NaiveDate) -> bool {
    !task.is_completed() && due_date(task.due.as_deref()).is_some_and(|d| d < today)
}

/// `MM-DD` within the current year, otherwise the full ISO date.
pub fn format_short(date: NaiveDate, today: NaiveDate) -> String {
    if date.year() == today.year() {
        date.format("%m-%d").to_string()
    } else {
        date.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parses_strict_dates() {
        let today = d(2026, 10, 7);
        assert_eq!(parse_due_input("2026-10-07", today), Ok(Some(today)));
        assert_eq!(parse_due_input("  2026-10-07 ", today), Ok(Some(today)));
        assert_eq!(parse_due_input("", today), Ok(None));
        assert_eq!(parse_due_input("   ", today), Ok(None));
        for invalid in ["2026-2-3", "2026/02/03", "2026-02-30", "0000-01-01"] {
            assert!(parse_due_input(invalid, today).is_err());
        }
    }

    #[test]
    fn resolves_shorthand_against_the_reference_day() {
        let today = d(2026, 10, 7);
        for (input, expected) in [
            (" Today ", today),
            ("TOMORROW", d(2026, 10, 8)),
            ("fri", d(2026, 10, 9)),
            ("Friday", d(2026, 10, 9)),
            ("next mon", d(2026, 10, 12)),
            ("wed", today),
            ("next wednesday", d(2026, 10, 14)),
            ("+3d", d(2026, 10, 10)),
            ("+2w", d(2026, 10, 21)),
            ("-1d", d(2026, 10, 6)),
        ] {
            assert_eq!(parse_due_input(input, today), Ok(Some(expected)), "{input}");
        }
        assert_eq!(
            parse_due_input("tomorrow", d(2026, 12, 31)),
            Ok(Some(d(2027, 1, 1)))
        );
        for invalid in [
            "+",
            "+d",
            "+3m",
            "+-3d",
            "next",
            "+18446744073709551615w",
            "+日",
            "-é",
        ] {
            assert!(parse_due_input(invalid, today).is_err(), "{invalid}");
        }
    }

    #[test]
    fn adjustment_clamps_months_and_years_and_rolls_days() {
        assert_eq!(
            adjust_date(d(2026, 1, 31), DatePart::Month, 1),
            Some(d(2026, 2, 28))
        );
        assert_eq!(
            adjust_date(d(2024, 2, 29), DatePart::Year, 1),
            Some(d(2025, 2, 28))
        );
        assert_eq!(
            adjust_date(d(2026, 12, 24), DatePart::Month, 1),
            Some(d(2027, 1, 24))
        );
        assert_eq!(
            adjust_date(d(2026, 1, 1), DatePart::Day, -1),
            Some(d(2025, 12, 31))
        );
        assert_eq!(adjust_date(d(9999, 12, 31), DatePart::Day, 1), None);
        assert_eq!(adjust_date(d(1, 1, 1), DatePart::Year, -1), None);
        assert!(parse_due_input("tomorrow", d(9999, 12, 31)).is_err());
    }

    #[test]
    fn due_date_ignores_time_and_zone() {
        assert_eq!(
            due_date(Some("2026-10-07T00:00:00.000Z")),
            Some(d(2026, 10, 7))
        );
        assert_eq!(due_date(Some("2026-10-07")), Some(d(2026, 10, 7)));
        assert_eq!(due_date(Some("garbage")), None);
        assert_eq!(due_date(None), None);
    }

    #[test]
    fn overdue_is_strictly_before_today() {
        let today = d(2026, 10, 6);
        let mut task = Task::new_local("a".into(), "x".into(), None);
        task.due = Some(to_api_due(d(2026, 10, 5)));
        assert!(is_overdue(&task, today));
        task.due = Some(to_api_due(today));
        assert!(!is_overdue(&task, today));
        task.due = Some(to_api_due(d(2026, 10, 5)));
        task.status = crate::model::TaskStatus::Completed;
        assert!(!is_overdue(&task, today));
    }

    #[test]
    fn short_format_depends_on_year() {
        let today = d(2026, 10, 6);
        assert_eq!(format_short(d(2026, 1, 2), today), "01-02");
        assert_eq!(format_short(d(2027, 1, 2), today), "2027-01-02");
    }
}
