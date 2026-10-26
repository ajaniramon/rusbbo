//! Game logic (Java: `org.alexdev.havana.game`).

pub mod fuserights;
pub mod messenger;
pub mod player;
pub mod scheduler;

/// Current Unix time in seconds (Java: `DateUtil.getCurrentTimeSeconds`).
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Today as `dd-MM-yyyy` in local time (Java: `DateUtil.getCurrentDate(SHORT_DATE)`).
pub fn today_short() -> String {
    chrono::Local::now().format("%d-%m-%Y").to_string()
}

/// Formats Unix seconds in local time (Java: `DateUtil.getDate`, a `SimpleDateFormat` pattern is
/// given here as a chrono format string).
pub fn format_local(time: i64, chrono_format: &str) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(time, 0).single() {
        Some(t) => t.format(chrono_format).to_string(),
        None => chrono_format.to_string(),
    }
}

/// Java: `DateUtil.LONG_DATE` (`dd-MM-yyyy HH:mm:ss`).
pub fn long_date(time: i64) -> String {
    format_local(time, "%d-%m-%Y %H:%M:%S")
}
