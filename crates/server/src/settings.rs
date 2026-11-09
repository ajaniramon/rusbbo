//! The `settings` table, cached in memory (Java: `GameConfiguration`).

/// Java: `normalise.input.strings`, read by `StringUtil.filterInput` on every call; kept here
/// for the filters that have no settings at hand (set whenever the settings are loaded).
pub static NORMALISE_INPUT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether `StringUtil.filterInput` normalises (NFD) at the moment.
pub fn normalise_input() -> bool {
    NORMALISE_INPUT.load(std::sync::atomic::Ordering::Relaxed)
}

use std::collections::BTreeMap;

pub struct Settings {
    values: BTreeMap<String, String>,
}

impl Settings {
    pub fn new(values: BTreeMap<String, String>) -> Self {
        Self { values }
    }

    /// Every setting.
    pub fn all(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    pub fn string(&self, key: &str) -> &str {
        self.values.get(key).map(String::as_str).unwrap_or("")
    }

    /// Java: `GameConfiguration.getConfig().containsKey`.
    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// Java `getBoolean`: true only for the text `true` (any case).
    pub fn bool(&self, key: &str) -> bool {
        self.string(key).eq_ignore_ascii_case("true")
    }

    /// Java: `GameConfiguration.updateSetting`'s in-memory part.
    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }

    /// Java `getInteger`: unparsable or missing values count as 0.
    pub fn int(&self, key: &str) -> i64 {
        self.string(key).trim().parse().unwrap_or(0)
    }
}
