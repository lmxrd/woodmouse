//! iCalendar (RFC 5545) text for the calendar feeds: one all-day event per task with dates.

use std::fmt::Write;

use crate::time::DateTime;
use jiff::civil::Date;

/// A task as it appears in a feed.
pub struct FeedTask {
    pub id: uuid::Uuid,
    pub title: String,
    pub description: String,
    pub project: String,
    pub priority: &'static str,
    /// The start and due dates as the app stores them (a day at 00:00 UTC): only the day counts.
    pub start: Option<DateTime>,
    pub due: Option<DateTime>,
    pub completed: bool,
    /// Where the task opens in the app.
    pub url: String,
}

fn date_value(date: Date) -> String {
    date.strftime("%Y%m%d").to_string()
}

/// Escape text values (RFC 5545 section 3.3.11).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Add one content line, folded so no line is longer than 75 octets (RFC 5545 section 3.1).
fn line(out: &mut String, text: &str) {
    let mut width = 0;
    for c in text.chars() {
        let size = c.len_utf8();
        if width + size > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += size;
    }
    out.push_str("\r\n");
}

/// The calendar. Tasks without a start or due date are left out; a completed task is marked done.
pub fn calendar(name: &str, tasks: &[FeedTask], now: DateTime) -> String {
    let mut out = String::new();
    line(&mut out, "BEGIN:VCALENDAR");
    line(&mut out, "VERSION:2.0");
    line(&mut out, "PRODID:-//Woodmouse//Tasks//EN");
    line(&mut out, "CALSCALE:GREGORIAN");
    line(&mut out, "METHOD:PUBLISH");
    line(&mut out, &format!("X-WR-CALNAME:{}", escape(name)));
    line(&mut out, "REFRESH-INTERVAL;VALUE=DURATION:PT1H");
    line(&mut out, "X-PUBLISHED-TTL:PT1H");
    for task in tasks {
        // Start..due as a range, or the one date there is; a due date before the start is just the due date.
        let (first, last) = match (task.start, task.due) {
            (Some(s), Some(d)) if d >= s => (s, d),
            (_, Some(d)) => (d, d),
            (Some(s), None) => (s, s),
            (None, None) => continue,
        };
        let (first_day, last_day) = (first.date(), last.date());
        line(&mut out, "BEGIN:VEVENT");
        line(&mut out, &format!("UID:{}@woodmouse", task.id));
        line(
            &mut out,
            &format!("DTSTAMP:{}", now.strftime("%Y%m%dT%H%M%SZ")),
        );
        line(
            &mut out,
            &format!("DTSTART;VALUE=DATE:{}", date_value(first_day)),
        );
        // The end of an all-day event is exclusive.
        line(
            &mut out,
            &format!(
                "DTEND;VALUE=DATE:{}",
                date_value(last_day.tomorrow().unwrap_or(last_day))
            ),
        );
        let title = if task.completed {
            format!("\u{2713} {}", task.title)
        } else {
            task.title.clone()
        };
        line(&mut out, &format!("SUMMARY:{}", escape(&title)));
        let mut description = String::new();
        let _ = write!(
            description,
            "{} priority \u{b7} {}",
            task.priority, task.project
        );
        if task.completed {
            description.push_str(" \u{b7} done");
        }
        description.push('\n');
        description.push_str(&task.url);
        if !task.description.trim().is_empty() {
            description.push_str("\n\n");
            description.extend(task.description.trim().chars().take(1500));
        }
        line(&mut out, &format!("DESCRIPTION:{}", escape(&description)));
        line(&mut out, &format!("URL:{}", task.url));
        line(&mut out, "TRANSP:TRANSPARENT");
        if task.completed {
            line(&mut out, "CATEGORIES:Done");
        }
        line(&mut out, "END:VEVENT");
    }
    line(&mut out, "END:VCALENDAR");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(start: Option<DateTime>, due: Option<DateTime>, completed: bool) -> FeedTask {
        FeedTask {
            id: uuid::Uuid::nil(),
            title: "Order oak, worktop; now".into(),
            description: "line one\nline two".into(),
            project: "House".into(),
            priority: "high",
            start,
            due,
            completed,
            url: "https://example.test/w/1/p/2/t/3".into(),
        }
    }

    #[test]
    fn a_range_becomes_one_event_ending_after_the_last_day() {
        let day: DateTime = jiff::civil::date(2026, 10, 16).at(0, 0, 0, 0).into();
        let text = calendar(
            "My tasks",
            &[task(
                Some(day),
                Some(day.checked_add(jiff::Span::new().days(2)).unwrap().into()),
                false,
            )],
            day,
        );
        assert!(text.contains("DTSTART;VALUE=DATE:20261016\r\n"), "{text}");
        assert!(text.contains("DTEND;VALUE=DATE:20261019\r\n"));
        assert!(text.contains("SUMMARY:Order oak\\, worktop\\; now\r\n"));
        assert!(text.contains("DESCRIPTION:high priority"));
        assert!(!text.contains("CATEGORIES:Done"));
    }

    #[test]
    fn a_single_date_is_a_one_day_event_and_done_tasks_are_marked() {
        let day: DateTime = jiff::civil::date(2026, 10, 16).at(0, 0, 0, 0).into();
        let text = calendar("x", &[task(None, Some(day), true)], day);
        assert!(text.contains("DTSTART;VALUE=DATE:20261016\r\n"));
        assert!(text.contains("DTEND;VALUE=DATE:20261017\r\n"));
        assert!(text.contains("SUMMARY:\u{2713} Order oak"));
        assert!(text.contains("CATEGORIES:Done\r\n"));
        // A due date before the start counts as just the due date.
        let text = calendar(
            "x",
            &[task(
                Some(day.checked_add(jiff::Span::new().days(5)).unwrap().into()),
                Some(day),
                false,
            )],
            day,
        );
        assert!(text.contains("DTEND;VALUE=DATE:20261017\r\n"));
    }

    #[test]
    fn undated_tasks_are_left_out_and_long_lines_fold() {
        let mut long = task(None, None, false);
        assert!(
            !calendar(
                "x",
                &[long.clone_dates()],
                DateTime(jiff::civil::DateTime::ZERO)
            )
            .contains("BEGIN:VEVENT")
        );
        long.due = Some(DateTime(jiff::civil::DateTime::ZERO));
        long.title = "x".repeat(200);
        let text = calendar("x", &[long], DateTime(jiff::civil::DateTime::ZERO));
        assert!(text.split("\r\n").all(|l| l.len() <= 75), "{text}");
        assert!(text.ends_with("END:VCALENDAR\r\n"));
    }

    impl FeedTask {
        fn clone_dates(&self) -> FeedTask {
            task(self.start, self.due, self.completed)
        }
    }
}
