//! Game logic (Java: `org.alexdev.havana.game`).

pub mod achievements;
pub mod badges;
pub mod bot;
pub mod catalogue;
pub mod club;
pub mod commands;
pub mod currency;
pub mod effects;
pub mod fuserights;
pub mod inventory;
pub mod item;
pub mod java_set;
pub mod messenger;
pub mod moderation;
pub mod navigator;
pub mod password;
pub mod pathfinder;
pub mod pet;
pub mod player;
pub mod room;
pub mod scheduler;
pub mod song;
pub mod statistics;
pub mod wordfilter;

/// Runs `f`, turning a panic into a logged error (Java catches the exception of a message
/// handler or a scheduled task, logs it and carries on). Without this a panic in a handler kills
/// the connection task before `player::dispose`, leaving a ghost player online, and a panic in a
/// tick ends that room's or the hotel's tick for good. The locks are `parking_lot`'s, which do not
/// poison.
pub fn contain_panic<R>(what: std::fmt::Arguments<'_>, f: impl FnOnce() -> R) -> Option<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Some(r),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            tracing::error!("panic in {what}: {reason}");
            None
        }
    }
}

/// Java: `String.valueOf(StringUtil.format(value))`: rounded to two decimals (`Math.round`, half
/// up), then printed like `Double.toString` (`0.0`, `1.5`, `1.0E7`).
pub fn format_decimal(value: f64) -> String {
    let rounded = (value * 100.0 + 0.5).floor() / 100.0;
    java_double(rounded)
}

/// Java's `Double.toString` (shortest round-trip digits, as since JDK 19).
pub fn java_double(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let abs = v.abs();
    if v == 0.0 || (1e-3..1e7).contains(&abs) {
        let s = format!("{v}");
        if s.contains('.') { s } else { format!("{s}.0") }
    } else {
        // `{:e}` gives e.g. "1.5e7"; Java writes "1.5E7" and always has a fraction digit.
        let s = format!("{v:e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exp}")
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_print_like_java() {
        assert_eq!(format_decimal(0.0), "0.0");
        assert_eq!(format_decimal(1.0), "1.0");
        assert_eq!(format_decimal(1.5), "1.5");
        assert_eq!(format_decimal(2.345), "2.35");
        assert_eq!(format_decimal(0.001), "0.0");
        assert_eq!(java_double(1.0e7), "1.0E7");
        assert_eq!(java_double(-0.0), "-0.0");
        assert_eq!(java_double(0.0001), "1.0E-4");
    }
}
