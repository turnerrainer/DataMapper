//! Minimal UTC RFC 3339 timestamp formatting, std only.
//!
//! Replaces the `chrono` dependency (RUSTSEC-2026-0543, unmaintained,
//! no patched version). Output is byte-compatible with
//! `chrono::Utc::now().to_rfc3339()`: `+00:00` suffix and the
//! fractional part omitted when zero, otherwise 3, 6 or 9 digits
//! (the shortest of those that is lossless).

use std::time::{SystemTime, UNIX_EPOCH};

/// Current UTC time as an RFC 3339 string.
pub fn now_rfc3339() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_unix(d.as_secs(), d.subsec_nanos())
}

/// Format seconds since the Unix epoch (+ nanoseconds) as RFC 3339 UTC.
pub fn format_unix(secs: u64, nanos: u32) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // `is_multiple_of` is avoided: it needs Rust 1.91, MSRV is 1.88.
    let frac = match (nanos, nanos % 1_000_000, nanos % 1_000) {
        (0, _, _) => String::new(),
        (_, 0, _) => format!(".{:03}", nanos / 1_000_000),
        (_, _, 0) => format!(".{:06}", nanos / 1_000),
        _ => format!(".{nanos:09}"),
    };
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}{frac}+00:00")
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's
/// `civil_from_days` algorithm, valid for the whole proleptic
/// Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch() {
        assert_eq!(format_unix(0, 0), "1970-01-01T00:00:00+00:00");
    }

    #[test]
    fn known_instants() {
        // `date -u -d @N` reference values.
        assert_eq!(format_unix(951_782_400, 0), "2000-02-29T00:00:00+00:00");
        assert_eq!(format_unix(1_000_000_000, 0), "2001-09-09T01:46:40+00:00");
        assert_eq!(format_unix(1_709_164_799, 0), "2024-02-28T23:59:59+00:00");
        assert_eq!(format_unix(1_709_251_199, 0), "2024-02-29T23:59:59+00:00");
        assert_eq!(format_unix(1_709_251_200, 0), "2024-03-01T00:00:00+00:00");
        assert_eq!(format_unix(1_767_225_599, 0), "2025-12-31T23:59:59+00:00");
        assert_eq!(format_unix(1_767_225_600, 0), "2026-01-01T00:00:00+00:00");
        assert_eq!(format_unix(4_102_444_800, 0), "2100-01-01T00:00:00+00:00");
        assert_eq!(format_unix(4_107_542_400, 0), "2100-03-01T00:00:00+00:00");
    }

    #[test]
    fn fraction_precision_matches_chrono_autosi() {
        assert_eq!(format_unix(0, 123_000_000), "1970-01-01T00:00:00.123+00:00");
        assert_eq!(
            format_unix(0, 123_456_000),
            "1970-01-01T00:00:00.123456+00:00"
        );
        assert_eq!(
            format_unix(0, 123_456_789),
            "1970-01-01T00:00:00.123456789+00:00"
        );
        assert_eq!(format_unix(0, 1_000_000), "1970-01-01T00:00:00.001+00:00");
        assert_eq!(format_unix(0, 1), "1970-01-01T00:00:00.000000001+00:00");
    }

    #[test]
    fn every_day_roundtrips_against_naive_calendar() {
        // Walk every day 1970..2200 with an independent day counter.
        let leap = |y: u32| matches!((y % 4, y % 100, y % 400), (0, 1..=99, _) | (_, _, 0));
        let dim = |y: u32, m: u32| match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ if leap(y) => 29,
            _ => 28,
        };
        let (mut y, mut m, mut d) = (1970u32, 1u32, 1u32);
        let mut day = 0u64;
        while y < 2200 {
            let want = format!("{y:04}-{m:02}-{d:02}T00:00:00+00:00");
            assert_eq!(format_unix(day * 86_400, 0), want);
            d += 1;
            if d > dim(y, m) {
                d = 1;
                m += 1;
                if m > 12 {
                    m = 1;
                    y += 1;
                }
            }
            day += 1;
        }
    }

    #[test]
    fn now_is_sane() {
        let s = now_rfc3339();
        assert!(s.ends_with("+00:00"));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
        assert!(s[..4].parse::<u32>().unwrap() >= 2026);
    }
}
