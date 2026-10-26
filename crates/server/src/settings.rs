//! The `settings` table, cached in memory (Java: `GameConfiguration`).

use std::collections::BTreeMap;

pub struct Settings {
    values: BTreeMap<String, String>,
}

impl Settings {
    pub fn new(values: BTreeMap<String, String>) -> Self {
        Self { values }
    }

    pub fn string(&self, key: &str) -> &str {
        self.values.get(key).map(String::as_str).unwrap_or("")
    }

    /// Java `getBoolean`: true only for the text `true` (any case).
    pub fn bool(&self, key: &str) -> bool {
        self.string(key).eq_ignore_ascii_case("true")
    }

    /// Java `getInteger`: unparsable or missing values count as 0.
    pub fn int(&self, key: &str) -> i64 {
        self.string(key).trim().parse().unwrap_or(0)
    }
}
