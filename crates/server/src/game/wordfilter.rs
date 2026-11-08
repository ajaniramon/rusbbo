//! The word filter (Java: `game/wordfilter/WordfilterManager`). Sentences are ISO-8859-1 bytes;
//! they are compared as one char per byte, like Java's strings.

use rusbbo_storage::WordfilterWord;

use crate::settings::Settings;

pub struct WordFilter {
    /// Longest words first (Java sorts a stable list by descending length). Replaced by
    /// `:reload wordfilter`.
    words: parking_lot::RwLock<Vec<WordfilterWord>>,
}

fn to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

fn to_bytes(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

impl WordFilter {
    pub fn new(words: Vec<WordfilterWord>) -> Self {
        Self {
            words: parking_lot::RwLock::new(Self::sorted(words)),
        }
    }

    fn sorted(mut words: Vec<WordfilterWord>) -> Vec<WordfilterWord> {
        words.sort_by_key(|w| std::cmp::Reverse(w.word.chars().count()));
        words
    }

    /// Java: `WordfilterManager.reset`.
    pub fn reload(&self, words: Vec<WordfilterWord>) {
        *self.words.write() = Self::sorted(words);
    }

    /// Java: `WordfilterManager.hasBannableSentence`, the sentence part: users who joined more
    /// than 30 minutes ago are never banned (the caller checks that).
    pub fn has_bannable_sentence(&self, settings: &Settings, sentence: &[u8]) -> bool {
        if !settings.bool("wordfitler.enabled") {
            return false;
        }
        let lower = to_string(sentence).to_lowercase();
        self.words
            .read()
            .iter()
            .any(|w| w.is_bannable && lower.contains(&w.word.to_lowercase()))
    }

    fn replace(&self, settings: &Settings, sentence: &[u8], mandatory_only: bool) -> Vec<u8> {
        if !settings.bool("wordfitler.enabled") {
            return sentence.to_vec();
        }
        let replacement = settings.string("wordfilter.word.replacement");
        let mut text = to_string(sentence);
        for w in self.words.read().iter() {
            if mandatory_only && w.is_filterable {
                continue;
            }
            let word = w.word.to_lowercase();
            let lower = text.to_lowercase();
            if lower.contains(&word) {
                text = lower.replace(&word, replacement);
            }
        }
        to_bytes(&text)
    }

    /// Java: `filterMandatorySentence`: the words that are not filterable, for everyone.
    pub fn filter_mandatory(&self, settings: &Settings, sentence: &[u8]) -> Vec<u8> {
        self.replace(settings, sentence, true)
    }

    /// Java: `filterSentence`: every word.
    pub fn filter_sentence(&self, settings: &Settings, sentence: &[u8]) -> Vec<u8> {
        self.replace(settings, sentence, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn word(word: &str, bannable: bool, filterable: bool) -> WordfilterWord {
        WordfilterWord {
            id: 0,
            word: word.into(),
            is_bannable: bannable,
            is_filterable: filterable,
        }
    }

    fn settings() -> Settings {
        Settings::new(BTreeMap::from([
            ("wordfitler.enabled".to_string(), "true".to_string()),
            (
                "wordfilter.word.replacement".to_string(),
                "bobba".to_string(),
            ),
        ]))
    }

    #[test]
    fn filters_like_chat_filter_trace() {
        let f = WordFilter::new(vec![
            word("aaron", false, true),
            word("zaphotel", false, false),
            word("habbo id", true, false),
        ]);
        let s = settings();
        // traces/chat-filter.json
        assert_eq!(
            f.filter_sentence(&s, b"Ask Aaron about it"),
            b"ask bobba about it"
        );
        assert_eq!(
            f.filter_mandatory(&s, b"Ask Aaron about it"),
            b"Ask Aaron about it"
        );
        assert_eq!(
            f.filter_mandatory(&s, b"Visit ZapHotel now"),
            b"visit bobba now"
        );
        assert_eq!(f.filter_mandatory(&s, b"zaphotel"), b"bobba");
        assert_eq!(f.filter_sentence(&s, b"AARON!"), b"bobba!");
        assert!(f.has_bannable_sentence(&s, b"my Habbo ID"));
        assert!(!f.has_bannable_sentence(&s, b"hello"));
    }
}
