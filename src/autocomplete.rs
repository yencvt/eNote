use std::collections::BTreeSet;

use crate::language::Language;

/// Live popup state for the word-based autocomplete feature.
#[derive(Default)]
pub struct AutocompleteState {
    pub active: bool,
    pub prefix: String,
    pub suggestions: Vec<String>,
    pub selected: usize,
    // Char index right after the typed prefix, where an accepted suggestion is inserted.
    pub anchor_char_index: usize,
}

/// Collects unique identifier-like words (len >= 2) from `text`, for word-based
/// autocomplete suggestions.
pub fn collect_words(text: &str) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            if current.len() >= 2 {
                words.insert(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 2 {
        words.insert(current);
    }
    words
}

/// Returns up to `limit` suggestions starting with `prefix` (case-insensitive), combining
/// the language's keyword list with words already present in the document.
pub fn suggestions(
    prefix: &str,
    language: Language,
    doc_words: &BTreeSet<String>,
    limit: usize,
) -> Vec<String> {
    if prefix.len() < 2 {
        return Vec::new();
    }
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();

    for keyword in language.keywords() {
        if keyword.to_ascii_lowercase().starts_with(&prefix_lower) && *keyword != prefix {
            out.push(keyword.to_string());
        }
    }
    for word in doc_words {
        if word.to_ascii_lowercase().starts_with(&prefix_lower)
            && word != prefix
            && !out.contains(word)
        {
            out.push(word.clone());
        }
    }
    out.truncate(limit);
    out
}

/// Extracts the identifier prefix ending exactly at `char_index` in `text`, if any.
pub fn current_word_prefix(text: &str, char_index: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    if char_index == 0 || char_index > chars.len() {
        return None;
    }
    let mut start = char_index;
    while start > 0 {
        let c = chars[start - 1];
        if c.is_alphanumeric() || c == '_' {
            start -= 1;
        } else {
            break;
        }
    }
    if start == char_index {
        return None;
    }
    Some(chars[start..char_index].iter().collect())
}
