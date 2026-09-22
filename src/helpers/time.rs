use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// RFC 3339 UTC timestamp, second precision. These sort as text, which
/// the log relies on to pick recent lines without parsing them.
pub fn iso(t: SystemTime) -> String {
    let d = OffsetDateTime::from(t);
    d.replace_nanosecond(0).unwrap_or(d).format(&Rfc3339).unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

pub fn now_iso() -> String {
    iso(SystemTime::now())
}

pub fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// `YYYY-MM-DD HH:MM` in UTC, for "last seen" style labels.
pub fn short_utc(t: SystemTime) -> String {
    let d = OffsetDateTime::from(t);
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", d.year(), u8::from(d.month()), d.day(), d.hour(), d.minute())
}

/// A point in time from `30m`, `12h`, `7d` (ago) or `2026-09-22` (UTC midnight).
pub fn parse_since(s: &str, now: SystemTime) -> Option<SystemTime> {
    let s = s.trim();
    if let Some(unit) = s.chars().last().filter(char::is_ascii_alphabetic) {
        let n: u64 = s[..s.len() - 1].parse().ok()?;
        let secs = n.checked_mul(match unit {
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            _ => return None,
        })?;
        return now.checked_sub(std::time::Duration::from_secs(secs));
    }
    let mut parts = s.split('-').map(str::parse::<i32>);
    let (y, m, d) = (parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
    let month = time::Month::try_from(u8::try_from(m).ok()?).ok()?;
    let date = time::Date::from_calendar_date(y, month, u8::try_from(d).ok()?).ok()?;
    Some(date.midnight().assume_utc().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parses_relative_and_calendar_since() {
        let now = UNIX_EPOCH + Duration::from_secs(10 * 86_400);
        assert_eq!(parse_since("2d", now), Some(UNIX_EPOCH + Duration::from_secs(8 * 86_400)));
        assert_eq!(parse_since("90m", now), Some(now - Duration::from_secs(5400)));
        assert_eq!(parse_since("1970-01-03", now), Some(UNIX_EPOCH + Duration::from_secs(2 * 86_400)));
        assert_eq!(parse_since("soon", now), None);
        assert_eq!(parse_since("5y", now), None);
        assert_eq!(parse_since("999999999999999999d", now), None);
    }

    #[test]
    fn formats_short_utc() {
        assert_eq!(short_utc(UNIX_EPOCH + Duration::from_secs(86_400 + 3_660)), "1970-01-02 01:01 UTC");
    }
}
