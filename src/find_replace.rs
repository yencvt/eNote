use regex::{Regex, RegexBuilder};

#[derive(Default)]
pub struct FindReplaceState {
    pub find_text: String,
    pub replace_text: String,
    pub use_regex: bool,
    pub match_case: bool,
    pub last_match: Option<(usize, usize)>,
}

impl FindReplaceState {
    fn compile_regex(&self) -> Option<Regex> {
        RegexBuilder::new(&self.find_text)
            .case_insensitive(!self.match_case)
            .build()
            .ok()
    }

    /// Finds the next match after the previous one (wrapping to the start), returning
    /// the byte range of the match within `text`.
    pub fn find_next(&mut self, text: &str) -> Result<Option<(usize, usize)>, String> {
        if self.find_text.is_empty() {
            self.last_match = None;
            return Ok(None);
        }

        let start_at = self.last_match.map(|(_, end)| end).unwrap_or(0);

        if self.use_regex {
            let re = self
                .compile_regex()
                .ok_or_else(|| "Invalid regex".to_string())?;
            if let Some(m) = re.find_at(text, start_at.min(text.len())) {
                self.last_match = Some((m.start(), m.end()));
                return Ok(self.last_match);
            }
            if let Some(m) = re.find(text) {
                self.last_match = Some((m.start(), m.end()));
                return Ok(self.last_match);
            }
            self.last_match = None;
            Ok(None)
        } else {
            let haystack = if self.match_case {
                text.to_string()
            } else {
                text.to_lowercase()
            };
            let needle = if self.match_case {
                self.find_text.clone()
            } else {
                self.find_text.to_lowercase()
            };
            let segment_start = start_at.min(haystack.len());
            if let Some(rel) = haystack[segment_start..].find(&needle) {
                let pos = segment_start + rel;
                self.last_match = Some((pos, pos + needle.len()));
                return Ok(self.last_match);
            }
            if let Some(pos) = haystack.find(&needle) {
                self.last_match = Some((pos, pos + needle.len()));
                return Ok(self.last_match);
            }
            self.last_match = None;
            Ok(None)
        }
    }

    /// Replaces the current match (if any was found via `find_next`) with `replace_text`,
    /// returning the new byte range of the inserted replacement.
    pub fn replace_current(&mut self, text: &mut String) -> Result<bool, String> {
        let Some((start, end)) = self.last_match else {
            return Ok(false);
        };
        if self.use_regex {
            let re = self
                .compile_regex()
                .ok_or_else(|| "Invalid regex".to_string())?;
            if let Some(m) = re.find_at(text, start) {
                if m.start() != start {
                    return Ok(false);
                }
                let replaced = re
                    .replace(&text[start..end], self.replace_text.as_str())
                    .into_owned();
                text.replace_range(start..end, &replaced);
                self.last_match = Some((start, start + replaced.len()));
                return Ok(true);
            }
            Ok(false)
        } else {
            text.replace_range(start..end, &self.replace_text);
            self.last_match = Some((start, start + self.replace_text.len()));
            Ok(true)
        }
    }

    /// Replaces every match in `text`, returning the number of replacements made.
    pub fn replace_all(&mut self, text: &mut String) -> Result<usize, String> {
        if self.find_text.is_empty() {
            return Ok(0);
        }
        if self.use_regex {
            let re = self
                .compile_regex()
                .ok_or_else(|| "Invalid regex".to_string())?;
            let count = re.find_iter(text).count();
            if count > 0 {
                *text = re
                    .replace_all(text, self.replace_text.as_str())
                    .into_owned();
            }
            self.last_match = None;
            Ok(count)
        } else {
            let count = if self.match_case {
                text.matches(self.find_text.as_str()).count()
            } else {
                text.to_lowercase()
                    .matches(&self.find_text.to_lowercase())
                    .count()
            };
            if count > 0 {
                if self.match_case {
                    *text = text.replace(&self.find_text, &self.replace_text);
                } else {
                    *text = replace_case_insensitive(text, &self.find_text, &self.replace_text);
                }
            }
            self.last_match = None;
            Ok(count)
        }
    }
}

fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let mut result = String::with_capacity(haystack.len());
    let mut pos = 0usize;
    while let Some(rel) = lower_haystack[pos..].find(&lower_needle) {
        let start = pos + rel;
        result.push_str(&haystack[pos..start]);
        result.push_str(replacement);
        pos = start + needle.len();
    }
    result.push_str(&haystack[pos..]);
    result
}
