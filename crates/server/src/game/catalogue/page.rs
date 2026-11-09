//! Java: `game/catalogue/CataloguePage.java`.

use crate::settings::Settings;

/// Java: `PlayerRank.ADMINISTRATOR`, the rank given to pages whose `min_role` is not a rank.
const ADMINISTRATOR: i32 = 8;
/// Java: `PlayerRank.getRankForId`: ranks are 0 to 8.
const MAX_RANK: i32 = 8;

#[derive(Clone, Debug)]
pub struct CataloguePage {
    pub id: i32,
    pub parent_id: i32,
    /// Rank id of `min_role` (Java: `getMinRole().getRankId()`).
    pub min_role: i32,
    pub is_navigatable: bool,
    pub is_club_only: bool,
    pub name: String,
    pub icon: i32,
    pub colour: i32,
    pub layout: String,
    images: Vec<String>,
    texts: Vec<String>,
    pub seasonal_start: Option<String>,
    pub seasonal_length: i32,
}

impl CataloguePage {
    pub fn new(r: rusbbo_storage::CataloguePage) -> Self {
        Self {
            id: r.id as i32,
            parent_id: r.parent_id,
            min_role: if (0..=MAX_RANK).contains(&r.min_role) {
                r.min_role
            } else {
                ADMINISTRATOR
            },
            is_navigatable: r.is_navigatable,
            is_club_only: r.is_club_only,
            name: r.name,
            icon: r.icon,
            colour: r.colour,
            layout: r.layout,
            images: r.images,
            texts: r.texts,
            seasonal_start: r.seasonal_start,
            seasonal_length: r.seasonal_length,
        }
    }

    /// Java: `getImages`. The front page layout fills in its `{inputN}` placeholders from the
    /// `catalogue.frontpage.input.N` settings; input 1 is an image name without extension.
    pub fn images(&self, settings: &Settings) -> Vec<String> {
        if !self.layout.eq_ignore_ascii_case("frontpage3") {
            return self.images.clone();
        }
        self.images
            .iter()
            .map(|image| {
                let mut text = image.clone();
                for i in 1..5 {
                    let value = settings.string(&format!("catalogue.frontpage.input.{i}"));
                    let value = if i == 1 {
                        value.rfind('.').map_or(value, |dot| &value[..dot])
                    } else {
                        value
                    };
                    text = text.replace(&format!("{{input{i}}}"), value);
                }
                text
            })
            .collect()
    }

    /// Java: `getTexts`.
    pub fn texts(&self, settings: &Settings) -> Vec<String> {
        if !self.layout.eq_ignore_ascii_case("frontpage3") {
            return self.texts.clone();
        }
        self.texts
            .iter()
            .map(|text| {
                let mut text = text.clone();
                for i in 1..5 {
                    let value = settings.string(&format!("catalogue.frontpage.input.{i}"));
                    text = text.replace(&format!("{{input{i}}}"), value);
                }
                text
            })
            .collect()
    }

    /// Java: `isValidSeasonal`: with `seasonal.items` on, a page with a `seasonal_start` (`MM-dd`
    /// in the current year, local time, or `EASTER`) shows for `seasonal_length` seconds from that
    /// day's midnight; `EASTER` pages only on Easter Monday.
    pub fn is_valid_seasonal(&self, settings: &Settings) -> bool {
        use chrono::{Datelike, Local};
        if !settings.bool("seasonal.items") {
            return true;
        }
        let Some(start) = self.seasonal_start.as_deref() else {
            return true;
        };
        let today = Local::now().date_naive();
        if start == "EASTER" {
            return today == easter_sunday(today.year()) + chrono::Days::new(1);
        }
        let now = crate::game::now();
        let start_time = seasonal_start_time(today.year(), start);
        let end_time = start_time + i64::from(self.seasonal_length);
        now > start_time && end_time > now
    }
}

/// Java: `DateUtil.getFromFormat("yyyy-MM-dd", year + "-" + start)`: a lenient
/// `SimpleDateFormat` in local time (a month or day out of range rolls over), 0 when it does not
/// parse.
fn seasonal_start_time(year: i32, start: &str) -> i64 {
    use chrono::{Months, NaiveDate, TimeZone};
    fn number(s: &str) -> Option<u32> {
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    }
    let mut parts = start.splitn(2, '-');
    let (Some(month), Some(day)) = (parts.next().and_then(number), parts.next().and_then(number))
    else {
        return 0;
    };
    let Some(date) = NaiveDate::from_ymd_opt(year, 1, 1)
        .and_then(|d| d.checked_add_months(Months::new(month.saturating_sub(1))))
        .and_then(|d| d.checked_add_days(chrono::Days::new(u64::from(day.saturating_sub(1)))))
    else {
        return 0;
    };
    // Lenient parsing takes month 0 and day 0 as the one before the first.
    let date = if month == 0 {
        date.checked_sub_months(Months::new(1)).unwrap_or(date)
    } else {
        date
    };
    let date = if day == 0 {
        date.pred_opt().unwrap_or(date)
    } else {
        date
    };
    date.and_hms_opt(0, 0, 0)
        .and_then(|d| chrono::Local.from_local_datetime(&d).earliest())
        .map_or(0, |d| d.timestamp())
}

/// Java: `EasterUtil.sundayFor` (the anonymous Gregorian algorithm).
fn easter_sunday(y: i32) -> chrono::NaiveDate {
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let m = (32 + 2 * e + 2 * i - h - k) % 7;
    let n = (a + 11 * h + 22 * m) / 451;
    let o = h + m - 7 * n + 114;
    chrono::NaiveDate::from_ymd_opt(y, (o / 31) as u32, (o % 31 + 1) as u32)
        .expect("a valid Easter date")
}

#[cfg(test)]
mod seasonal_tests {
    use super::*;

    #[test]
    fn easter_like_java() {
        assert_eq!(
            easter_sunday(2026),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 5).unwrap()
        );
        assert_eq!(
            easter_sunday(2025),
            chrono::NaiveDate::from_ymd_opt(2025, 4, 20).unwrap()
        );
        assert_eq!(
            easter_sunday(2027),
            chrono::NaiveDate::from_ymd_opt(2027, 3, 28).unwrap()
        );
    }

    #[test]
    fn start_dates_roll_over_like_simple_date_format() {
        let jan1 = seasonal_start_time(2026, "01-01");
        assert_eq!(seasonal_start_time(2026, "02-01") - jan1, 31 * 86400);
        assert_eq!(
            seasonal_start_time(2026, "01-32"),
            seasonal_start_time(2026, "02-01")
        );
        assert_eq!(
            seasonal_start_time(2026, "13-01"),
            seasonal_start_time(2027, "01-01")
        );
        assert_eq!(seasonal_start_time(2026, "x"), 0);
        assert_eq!(seasonal_start_time(2026, ""), 0);
    }
}
