//! Date helpers. Google stores `due` as a date-only RFC 3339 timestamp at
//! midnight UTC, so it must never be converted through a timezone.

use chrono::{Datelike, Local, NaiveDate, SecondsFormat, Utc};

use crate::model::Task;

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Strict `YYYY-MM-DD` parser for the details editor. Empty input clears the date.
pub fn parse_due_input(input: &str) -> Result<Option<NaiveDate>, String> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if s.len() != 10 {
        return Err(format!("expected YYYY-MM-DD, got '{s}'"));
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| format!("invalid date '{s}', expected YYYY-MM-DD"))
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
        assert_eq!(parse_due_input("2026-10-07"), Ok(Some(d(2026, 10, 7))));
        assert_eq!(parse_due_input("  2026-10-07 "), Ok(Some(d(2026, 10, 7))));
        assert_eq!(parse_due_input(""), Ok(None));
        assert_eq!(parse_due_input("   "), Ok(None));
        assert!(parse_due_input("2026-2-3").is_err());
        assert!(parse_due_input("2026/02/03").is_err());
        assert!(parse_due_input("2026-02-30").is_err());
        assert!(parse_due_input("tomorrow").is_err());
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
