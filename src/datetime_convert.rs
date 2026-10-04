//! Date/time conversions for the "Convert" Utilities tool: Unix timestamp <-> human-
//! readable date/time, and reformatting a date/time string from one layout to another.
//! Format strings use `chrono`'s strftime-style syntax (e.g. `%Y-%m-%d %H:%M:%S`).

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

/// The unit a Unix-epoch integer is expressed in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimestampUnit {
    Seconds,
    Millis,
    Micros,
    Nanos,
}

impl TimestampUnit {
    pub const ALL: [TimestampUnit; 4] = [
        TimestampUnit::Seconds,
        TimestampUnit::Millis,
        TimestampUnit::Micros,
        TimestampUnit::Nanos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TimestampUnit::Seconds => "Seconds",
            TimestampUnit::Millis => "Milliseconds",
            TimestampUnit::Micros => "Microseconds",
            TimestampUnit::Nanos => "Nanoseconds",
        }
    }
}

/// Which timezone a date/time value is interpreted in/converted to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TimeZoneChoice {
    #[default]
    Utc,
    Local,
}

impl TimeZoneChoice {
    pub const ALL: [TimeZoneChoice; 2] = [TimeZoneChoice::Utc, TimeZoneChoice::Local];

    pub fn label(self) -> &'static str {
        match self {
            TimeZoneChoice::Utc => "UTC",
            TimeZoneChoice::Local => "Local",
        }
    }
}

/// Returns the current moment as a Unix-epoch integer in `unit` - used for a "Now" button.
pub fn now_timestamp(unit: TimestampUnit) -> i64 {
    let now = Utc::now();
    match unit {
        TimestampUnit::Seconds => now.timestamp(),
        TimestampUnit::Millis => now.timestamp_millis(),
        TimestampUnit::Micros => now.timestamp_micros(),
        TimestampUnit::Nanos => now.timestamp_nanos_opt().unwrap_or(0),
    }
}

fn timestamp_to_utc(value: i64, unit: TimestampUnit) -> Result<DateTime<Utc>, String> {
    match unit {
        TimestampUnit::Seconds => DateTime::from_timestamp(value, 0),
        TimestampUnit::Millis => DateTime::from_timestamp_millis(value),
        TimestampUnit::Micros => DateTime::from_timestamp_micros(value),
        TimestampUnit::Nanos => Some(DateTime::from_timestamp_nanos(value)),
    }
    .ok_or_else(|| "Value is out of range for a valid date/time".to_string())
}

/// Converts a Unix-epoch integer (in `unit`) into a formatted date/time string in the
/// chosen timezone. `format` uses `chrono`'s strftime-style syntax.
pub fn timestamp_to_string(
    value: i64,
    unit: TimestampUnit,
    tz: TimeZoneChoice,
    format: &str,
) -> Result<String, String> {
    let utc = timestamp_to_utc(value, unit)?;
    Ok(match tz {
        TimeZoneChoice::Utc => utc.format(format).to_string(),
        TimeZoneChoice::Local => utc.with_timezone(&Local).format(format).to_string(),
    })
}

fn parse_naive(input: &str, format: &str) -> Result<NaiveDateTime, String> {
    NaiveDateTime::parse_from_str(input.trim(), format)
        .map_err(|e| format!("Doesn't match the format \"{format}\": {e}"))
}

/// Parses a date/time string (using `format`, interpreted in timezone `tz`) and returns
/// its Unix-epoch value in `unit`.
pub fn string_to_timestamp(
    input: &str,
    format: &str,
    tz: TimeZoneChoice,
    unit: TimestampUnit,
) -> Result<i64, String> {
    let naive = parse_naive(input, format)?;
    let utc = match tz {
        TimeZoneChoice::Utc => naive.and_utc(),
        TimeZoneChoice::Local => Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| {
                "Ambiguous or invalid local time (e.g. during a DST change)".to_string()
            })?
            .with_timezone(&Utc),
    };
    match unit {
        TimestampUnit::Seconds => Ok(utc.timestamp()),
        TimestampUnit::Millis => Ok(utc.timestamp_millis()),
        TimestampUnit::Micros => Ok(utc.timestamp_micros()),
        TimestampUnit::Nanos => utc
            .timestamp_nanos_opt()
            .ok_or_else(|| "Out of range for nanosecond precision".to_string()),
    }
}

/// Reformats a date/time string from `input_format` into `output_format` - e.g. turning
/// `2024-01-02 03:04:05` into `02/01/2024 03:04 AM`. Does not change the timezone it's
/// interpreted in, just how it's displayed.
pub fn reformat_datetime(
    input: &str,
    input_format: &str,
    output_format: &str,
) -> Result<String, String> {
    let naive = parse_naive(input, input_format)?;
    Ok(naive.format(output_format).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_to_string_formats_known_instant_in_utc() {
        // 2021-01-01T00:00:00Z
        let result = timestamp_to_string(
            1609459200,
            TimestampUnit::Seconds,
            TimeZoneChoice::Utc,
            "%Y-%m-%d %H:%M:%S",
        );
        assert_eq!(result, Ok("2021-01-01 00:00:00".to_string()));
    }

    #[test]
    fn timestamp_to_string_handles_millis_and_micros() {
        let millis = timestamp_to_string(
            1609459200000,
            TimestampUnit::Millis,
            TimeZoneChoice::Utc,
            "%Y-%m-%d",
        );
        assert_eq!(millis, Ok("2021-01-01".to_string()));

        let micros = timestamp_to_string(
            1609459200000000,
            TimestampUnit::Micros,
            TimeZoneChoice::Utc,
            "%Y-%m-%d",
        );
        assert_eq!(micros, Ok("2021-01-01".to_string()));
    }

    #[test]
    fn string_to_timestamp_round_trips_through_utc() {
        let ts = string_to_timestamp(
            "2021-01-01 00:00:00",
            "%Y-%m-%d %H:%M:%S",
            TimeZoneChoice::Utc,
            TimestampUnit::Seconds,
        );
        assert_eq!(ts, Ok(1609459200));
    }

    #[test]
    fn string_to_timestamp_reports_a_clear_error_on_mismatch() {
        let err = string_to_timestamp(
            "not a date",
            "%Y-%m-%d %H:%M:%S",
            TimeZoneChoice::Utc,
            TimestampUnit::Seconds,
        );
        assert!(err.is_err());
    }

    #[test]
    fn reformat_datetime_changes_the_layout() {
        let result = reformat_datetime("2024-01-02 03:04:05", "%Y-%m-%d %H:%M:%S", "%d/%m/%Y");
        assert_eq!(result, Ok("02/01/2024".to_string()));
    }

    #[test]
    fn timestamp_and_string_round_trip_each_other() {
        let now_secs = now_timestamp(TimestampUnit::Seconds);
        let as_string = timestamp_to_string(
            now_secs,
            TimestampUnit::Seconds,
            TimeZoneChoice::Utc,
            "%Y-%m-%d %H:%M:%S",
        )
        .unwrap();
        let back = string_to_timestamp(
            &as_string,
            "%Y-%m-%d %H:%M:%S",
            TimeZoneChoice::Utc,
            TimestampUnit::Seconds,
        );
        assert_eq!(back, Ok(now_secs));
    }
}
