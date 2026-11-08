//! Java's `SimpleDateFormat` (English, the JVM's local time zone), for Havana's `DateUtil`.

use chrono::{Datelike, Local, TimeZone, Timelike};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Java: `DateUtil.LONG_DATE`.
pub const LONG_DATE: &str = "dd-MM-yyyy HH:mm:ss";
/// Java: `DateUtil.SHORT_DATE`.
pub const SHORT_DATE: &str = "dd-MM-yyyy";
/// Java: `DateUtil.SHORT_DATE_TIME`.
pub const SHORT_DATE_TIME: &str = "dd-MM-yyyy hh:mm a";

/// Java: `DateUtil.getDate(seconds, format)`.
pub fn format(seconds: i64, pattern: &str) -> String {
    let Some(t) = Local.timestamp_opt(seconds, 0).single() else {
        return pattern.to_string();
    };
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            // Quoted text; '' is a quote.
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if chars.get(i + 1) == Some(&'\'') {
                        out.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        i += n;
        let num = |v: i64, out: &mut String| out.push_str(&format!("{v:0n$}"));
        match c {
            'y' => {
                let y = t.year() as i64;
                if n == 2 {
                    num(y % 100, &mut out)
                } else {
                    num(y, &mut out)
                }
            }
            'Y' => {
                let y = week_year(&t) as i64;
                if n == 2 {
                    num(y % 100, &mut out)
                } else {
                    num(y, &mut out)
                }
            }
            'M' | 'L' => {
                let m = t.month0() as usize;
                match n {
                    1 | 2 => num(m as i64 + 1, &mut out),
                    3 => out.push_str(&MONTHS[m][..3]),
                    _ => out.push_str(MONTHS[m]),
                }
            }
            'd' => num(t.day() as i64, &mut out),
            'D' => num(t.ordinal() as i64, &mut out),
            'E' => {
                let d = DAYS[t.weekday().num_days_from_monday() as usize];
                if n >= 4 {
                    out.push_str(d)
                } else {
                    out.push_str(&d[..3])
                }
            }
            'u' => num(t.weekday().number_from_monday() as i64, &mut out),
            'a' => out.push_str(if t.hour() < 12 { "AM" } else { "PM" }),
            'H' => num(t.hour() as i64, &mut out),
            'k' => num(if t.hour() == 0 { 24 } else { t.hour() as i64 }, &mut out),
            'K' => num((t.hour() % 12) as i64, &mut out),
            'h' => num(
                if t.hour() % 12 == 0 {
                    12
                } else {
                    (t.hour() % 12) as i64
                },
                &mut out,
            ),
            'm' => num(t.minute() as i64, &mut out),
            's' => num(t.second() as i64, &mut out),
            'S' => num(0, &mut out),
            'z' => out.push_str(&t.format("%Z").to_string()),
            'Z' => out.push_str(&t.format("%z").to_string()),
            'X' => out.push_str(&t.format("%:z").to_string()),
            'G' => out.push_str("AD"),
            'w' => num(us_week_of_year(&t).1 as i64, &mut out),
            _ => {
                for _ in 0..n {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// The US week (weeks start on Sunday; week 1 holds January 1st): (week-year, week).
fn us_week_of_year<T: Datelike>(t: &T) -> (i32, u32) {
    let year = t.year();
    let jan1 = chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let offset = jan1.weekday().num_days_from_sunday();
    let week = (t.ordinal0() + offset) / 7 + 1;
    // The last days of December belong to week 1 of the next year when that week starts in it.
    let next_jan1 = chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap();
    let next_offset = next_jan1.weekday().num_days_from_sunday();
    let days_left =
        (next_jan1 - chrono::NaiveDate::from_ymd_opt(year, t.month(), t.day()).unwrap()).num_days();
    if next_offset != 0 && days_left <= next_offset as i64 {
        return (year + 1, 1);
    }
    (year, week)
}

fn week_year<T: Datelike>(t: &T) -> i32 {
    us_week_of_year(t).0
}

/// Java: `DateUtil.getFriendlyDate`: `MMM dd, yyyy hh:mm:ss a`.
pub fn friendly(seconds: i64) -> String {
    format(seconds, "MMM dd, yyyy hh:mm:ss a")
        .replace("am", "AM")
        .replace("pm", "PM")
        .replace('.', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_simple_date_format() {
        // 2026-01-01 00:00:00 in the local zone.
        let t = Local
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(friendly(t), "Jan 01, 2026 12:00:00 AM");
        assert_eq!(format(t, "EEE dd MMM, yyyy"), "Thu 01 Jan, 2026");
        assert_eq!(format(t, "dd-MM-yyyy"), "01-01-2026");
        assert_eq!(format(t + 13 * 3600 + 5 * 60, "h:mm a"), "1:05 PM");
        assert_eq!(format(t, "MMMM d'th' ''yy"), "January 1th '26");
        // 2025-12-29 is in the week of January 1st, 2026.
        let dec = Local
            .with_ymd_and_hms(2025, 12, 29, 12, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(format(dec, "MMM dd, YYYY"), "Dec 29, 2026");
    }
}
