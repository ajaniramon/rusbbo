//! Game logic (Java: `org.alexdev.havana.game`).

pub mod fuserights;
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
