//! Dates and times. Every stored date-time is a `jiff::civil::DateTime` that means **UTC**; the API
//! sends and accepts them as RFC 3339 with an offset (`2026-10-16T08:30:00Z`), so no client can read
//! one as local time. Task dates (`start_date`, `due_date`) are a calendar day stored as that day at
//! `00:00` UTC: only the date part counts.

use std::ops::Deref;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

/// A UTC date-time: a `jiff::civil::DateTime` that sqlx can store (as `YYYY-MM-DDTHH:MM:SS` text, which
/// sorts like the time it names) and that derefs to the jiff type, so its methods are all there.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, sea_orm::DeriveValueType,
)]
#[sea_orm(
    value_type = "String",
    from_str = "DateTime::from_text",
    to_str = "DateTime::as_text"
)]
pub struct DateTime(pub jiff::civil::DateTime);

impl DateTime {
    /// The stored form, `YYYY-MM-DDTHH:MM:SS`.
    fn as_text(&self) -> String {
        self.0.to_string()
    }

    fn from_text(text: &str) -> Result<Self, sea_orm::sea_query::ValueTypeErr> {
        text.parse()
            .map(Self)
            .map_err(|_| sea_orm::sea_query::ValueTypeErr)
    }
}

impl Deref for DateTime {
    type Target = jiff::civil::DateTime;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<jiff::civil::DateTime> for DateTime {
    fn from(dt: jiff::civil::DateTime) -> Self {
        Self(dt)
    }
}

impl From<DateTime> for jiff::civil::DateTime {
    fn from(dt: DateTime) -> Self {
        dt.0
    }
}

impl std::fmt::Display for DateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The current moment, as a UTC date-time in whole seconds (every stored time has the same shape, so
/// they sort and compare as text too).
pub fn now() -> DateTime {
    from_timestamp(
        Timestamp::from_second(Timestamp::now().as_second()).unwrap_or(Timestamp::UNIX_EPOCH),
    )
}

/// The moment as a timestamp (for formatting with a `Z`).
pub fn timestamp(dt: DateTime) -> Timestamp {
    dt.to_zoned(TimeZone::UTC)
        .map(|z| z.timestamp())
        .unwrap_or(Timestamp::UNIX_EPOCH)
}

/// A UTC date-time from a timestamp.
pub fn from_timestamp(ts: Timestamp) -> DateTime {
    DateTime(ts.to_zoned(TimeZone::UTC).datetime())
}

/// Whole seconds from `from` to `to` (negative if `to` is earlier).
pub fn seconds_between(from: DateTime, to: DateTime) -> i64 {
    from.duration_until(*to).as_secs()
}

/// `dt` moved by `seconds`.
pub fn add_seconds(dt: DateTime, seconds: i64) -> DateTime {
    DateTime(
        dt.checked_add(SignedDuration::from_secs(seconds))
            .unwrap_or(*dt),
    )
}

/// A calendar day as stored: that day at midnight UTC.
pub fn day(date: Date) -> DateTime {
    DateTime(date.to_datetime(jiff::civil::Time::midnight()))
}

/// Normalize a task date to its day (midnight UTC), dropping any time of day.
pub fn to_day(dt: DateTime) -> DateTime {
    day(dt.date())
}

/// Serde for a required UTC date-time: RFC 3339 with `Z` out, any RFC 3339 offset in.
pub mod utc {
    use super::DateTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(dt: &DateTime, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&super::timestamp(*dt))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime, D::Error> {
        let text = String::deserialize(d)?;
        text.parse::<jiff::Timestamp>()
            .map(super::from_timestamp)
            .map_err(serde::de::Error::custom)
    }
}

/// Serde for an optional UTC date-time (`null` for none).
pub mod utc_option {
    use super::DateTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(dt: &Option<DateTime>, s: S) -> Result<S::Ok, S::Error> {
        match dt {
            Some(dt) => s.collect_str(&super::timestamp(*dt)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime>, D::Error> {
        let text = Option::<String>::deserialize(d)?;
        text.map(|t| {
            t.parse::<jiff::Timestamp>()
                .map(super::from_timestamp)
                .map_err(serde::de::Error::custom)
        })
        .transpose()
    }
}

/// Serde for a PATCH field that can be absent (leave), `null` (clear) or a date-time (set).
pub mod utc_patch {
    use super::DateTime;
    use serde::Deserializer;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<DateTime>>, D::Error> {
        super::utc_option::deserialize(d).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_rfc3339() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct T {
            #[serde(with = "utc")]
            at: DateTime,
            #[serde(with = "utc_option")]
            maybe: Option<DateTime>,
        }
        let t = T {
            at: jiff::civil::date(2026, 10, 16).at(8, 30, 0, 0).into(),
            maybe: None,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"at":"2026-10-16T08:30:00Z","maybe":null}"#);
        assert_eq!(serde_json::from_str::<T>(&json).unwrap(), t);
        // Another offset is converted to UTC.
        let t2: T =
            serde_json::from_str(r#"{"at":"2026-10-16T10:30:00+02:00","maybe":null}"#).unwrap();
        assert_eq!(t2.at, t.at);
        // A time without an offset is refused.
        assert!(serde_json::from_str::<T>(r#"{"at":"2026-10-16T08:30:00","maybe":null}"#).is_err());
    }

    #[test]
    fn days_and_differences() {
        let a: DateTime = jiff::civil::date(2026, 10, 16).at(23, 59, 0, 0).into();
        assert_eq!(
            to_day(a),
            jiff::civil::date(2026, 10, 16).at(0, 0, 0, 0).into()
        );
        assert_eq!(seconds_between(to_day(a), a), 23 * 3600 + 59 * 60);
        assert_eq!(
            add_seconds(a, 60),
            jiff::civil::date(2026, 10, 17).at(0, 0, 0, 0).into()
        );
    }
}
