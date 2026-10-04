use eframe::egui;
use egui::Id;
use egui::text::CCursor;

use crate::encoding::TextEncoding;

/// Decodes a chunk of freshly read bytes for a "View Log (tail -f)" tab (see
/// `editor_view::poll_log_tail`), carrying over any trailing partial multi-byte character
/// left in `pending` from the previous call (and updating it with whatever's left over
/// after this call, if any) so a read landing mid-character never corrupts the decode -
/// it just shows up a poll later once the rest of its bytes have arrived.
pub fn decode_log_increment(
    encoding: TextEncoding,
    pending: &mut Vec<u8>,
    new_bytes: &[u8],
) -> String {
    let mut combined = std::mem::take(pending);
    combined.extend_from_slice(new_bytes);

    match encoding {
        TextEncoding::Utf8 | TextEncoding::Utf8Bom => match std::str::from_utf8(&combined) {
            Ok(s) => s.to_string(),
            Err(e) if e.error_len().is_none() => {
                // Trailing bytes are an incomplete char (read landed mid-character) -
                // hold them back and decode the rest, like `tail -f` catching up.
                let valid_up_to = e.valid_up_to();
                let held_back = combined.split_off(valid_up_to);
                let decoded = std::str::from_utf8(&combined)
                    .unwrap_or_default()
                    .to_string();
                *pending = held_back;
                decoded
            }
            Err(_) => String::from_utf8_lossy(&combined).into_owned(),
        },
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            if !combined.len().is_multiple_of(2) {
                *pending = vec![*combined.last().unwrap()];
                combined.truncate(combined.len() - 1);
            }
            encoding.decode_with(&combined)
        }
        TextEncoding::Windows1252 => encoding.decode_with(&combined),
    }
}

/// Converts a 1-based (line, column) pair into a 0-based char index into `text`.
pub fn char_index_for_line_col(text: &str, line: usize, column: usize) -> usize {
    let target_line = line.saturating_sub(1);
    let target_col = column.saturating_sub(1);

    let mut char_index = 0usize;
    for (idx, line_text) in text.split('\n').enumerate() {
        if idx == target_line {
            let line_chars = line_text.chars().count();
            return char_index + target_col.min(line_chars);
        }
        char_index += line_text.chars().count() + 1;
    }

    text.chars().count()
}

/// Converts a 0-based char index into a 1-based (line, column) pair.
pub fn line_col_for_char_index(text: &str, char_index: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;

    for (i, ch) in text.chars().enumerate() {
        if i >= char_index {
            break;
        }

        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}

pub fn byte_index_from_char_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

/// Is this character part of a "word" for double-click "select word" purposes - matches
/// egui's own `is_word_char` so this selects the same word boundaries egui's built-in
/// double-click handling would.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn all_word_chars(s: &str) -> bool {
    s.chars().all(is_word_char)
}

/// Unicode-aware forward word-boundary search within `text`, mirroring egui's own
/// `next_word_boundary_char_index` (used for its Ctrl+Right / double-click word
/// selection): segments `text` using the standard Unicode word-boundary algorithm (via
/// `unicode-segmentation`'s `split_word_bound_indices`, correctly handling non-ASCII
/// scripts, combining marks, contractions, etc. - not just plain alphanumeric/underscore
/// runs), with the same `.` -always-a-boundary quirk egui's version has (so e.g.
/// `www.example.com` splits at the dots, matching how macOS does it).
fn next_word_boundary_char_index(text: &str, cursor_ci: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation as _;
    for (word_byte_index, word) in text.split_word_bound_indices() {
        let word_ci = text[..word_byte_index].chars().count();
        for (dot_ci_offset, chr) in word.chars().enumerate() {
            let dot_ci = word_ci + dot_ci_offset;
            if chr == '.' && cursor_ci < dot_ci {
                return dot_ci;
            }
        }
        if cursor_ci < word_ci && !all_word_chars(word) {
            return word_ci;
        }
    }
    text.chars().count()
}

/// Backward counterpart of `next_word_boundary_char_index`, implemented (like egui's own
/// `ccursor_previous_word`) by reversing the text's grapheme clusters and searching
/// forward in the reversed copy. Safe to do here because `text` is only ever a single
/// *line* (see `word_range_at_char_index` below), unlike egui's own version, which
/// reverses the *entire document* on every call - an O(document length) cost paid on
/// every double-click no matter where it lands, which is what made double-clicking to
/// select a word freeze the UI on large files.
fn previous_word_boundary_char_index(text: &str, cursor_ci: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation as _;
    let num_chars = text.chars().count();
    let reversed: String = text.graphemes(true).rev().collect();
    num_chars - next_word_boundary_char_index(&reversed, num_chars - cursor_ci).min(num_chars)
}

/// Finds the word touching char index `at` in `text` - i.e. what a double-click "select
/// word" should select - using the same Unicode-aware word segmentation egui's own
/// double-click handling uses (so special characters, accented letters, CJK text, etc.
/// are handled just as intelligently - not just plain alphanumeric/underscore runs), but
/// bounded to the current *line* instead of egui's own implementation, which scans the
/// *entire document* on every double-click. A line break is already a hard word boundary,
/// so restricting the search to the clicked line gives identical results to scanning the
/// whole document while keeping the cost proportional to that one line's length, not the
/// document's. Returns `None` if there's nothing to select (e.g. double-clicking
/// whitespace with no adjacent word), matching egui's own behavior.
pub fn word_range_at_char_index(text: &str, at: usize) -> Option<(usize, usize)> {
    let byte_at = byte_index_from_char_index(text, at);
    let (before_text, after_text) = text.split_at(byte_at);

    let line_start_byte = before_text.rfind('\n').map_or(0, |i| i + 1);
    let line_end_byte = byte_at + after_text.find('\n').unwrap_or(after_text.len());
    let line = &text[line_start_byte..line_end_byte];
    let col = text[line_start_byte..byte_at].chars().count();
    let line_start_char = at - col;

    let (min, max) = if col == 0 {
        (0, next_word_boundary_char_index(line, 0))
    } else {
        let mut it = line.chars().skip(col - 1);
        match (it.next(), it.next()) {
            (Some(before), Some(after)) if is_word_char(before) && is_word_char(after) => {
                let min = previous_word_boundary_char_index(line, col + 1);
                let max = next_word_boundary_char_index(line, min);
                (min, max)
            }
            (Some(before), Some(_)) if is_word_char(before) => {
                let min = previous_word_boundary_char_index(line, col);
                let max = next_word_boundary_char_index(line, min);
                (min, max)
            }
            (Some(_), Some(after)) if is_word_char(after) => {
                let max = next_word_boundary_char_index(line, col);
                (col, max)
            }
            (Some(_), Some(_)) => {
                let min = previous_word_boundary_char_index(line, col);
                let max = next_word_boundary_char_index(line, col);
                (min, max)
            }
            (Some(_), None) => {
                let min = previous_word_boundary_char_index(line, col);
                (min, col)
            }
            (None, _) => {
                let max = next_word_boundary_char_index(line, col);
                (col, max)
            }
        }
    };

    if min == max {
        None
    } else {
        Some((line_start_char + min, line_start_char + max))
    }
}

pub fn slice_char_range(text: &str, char_range: std::ops::Range<usize>) -> String {
    let start = char_range.start.min(char_range.end);
    let end = char_range.end.max(char_range.start);
    let byte_start = byte_index_from_char_index(text, start);
    let byte_end = byte_index_from_char_index(text, end);
    text[byte_start..byte_end].to_string()
}

pub fn replace_char_range(
    text: &mut String,
    char_range: std::ops::Range<usize>,
    replacement: &str,
) -> usize {
    let start = char_range.start.min(char_range.end);
    let end = char_range.end.max(char_range.start);
    let byte_start = byte_index_from_char_index(text, start);
    let byte_end = byte_index_from_char_index(text, end);
    text.replace_range(byte_start..byte_end, replacement);
    start + replacement.chars().count()
}

pub fn set_editor_selection(ctx: &egui::Context, editor_id: Id, start: usize, end: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) {
        let range = egui::text::CCursorRange {
            primary: CCursor::new(start),
            secondary: CCursor::new(end),
            h_pos: None,
        };
        state.cursor.set_char_range(Some(range));
        state.store(ctx, editor_id);
    }
}

pub fn read_clipboard_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard.get_text().ok()
}

/// Like the now-removed `overlay_selection_background` but only tints the background,
/// keeping each
/// section's original text color (used for diff/compare line highlighting, where the
/// syntax-highlighted text color should stay readable).
pub fn overlay_background_only(
    job: &mut egui::text::LayoutJob,
    byte_range: std::ops::Range<usize>,
    background: egui::Color32,
) {
    if byte_range.is_empty() {
        return;
    }

    let mut new_sections = Vec::with_capacity(job.sections.len() + 2);
    for section in job.sections.drain(..) {
        let sec_start = section.byte_range.start;
        let sec_end = section.byte_range.end;
        let overlap_start = sec_start.max(byte_range.start);
        let overlap_end = sec_end.min(byte_range.end);

        if overlap_start >= overlap_end {
            new_sections.push(section);
            continue;
        }

        if sec_start < overlap_start {
            new_sections.push(egui::text::LayoutSection {
                leading_space: section.leading_space,
                byte_range: sec_start..overlap_start,
                format: section.format.clone(),
            });
        }

        let mut tinted_format = section.format.clone();
        tinted_format.background = background;
        new_sections.push(egui::text::LayoutSection {
            leading_space: if sec_start < overlap_start {
                0.0
            } else {
                section.leading_space
            },
            byte_range: overlap_start..overlap_end,
            format: tinted_format,
        });

        if overlap_end < sec_end {
            new_sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: overlap_end..sec_end,
                format: section.format,
            });
        }
    }
    job.sections = new_sections;
}

#[cfg(test)]
mod tests {
    use super::{decode_log_increment, word_range_at_char_index};
    use crate::encoding::TextEncoding;

    #[test]
    fn decode_log_increment_decodes_plain_utf8_in_one_go() {
        let mut pending = Vec::new();
        let decoded =
            decode_log_increment(TextEncoding::Utf8, &mut pending, "hello log\n".as_bytes());
        assert_eq!(decoded, "hello log\n");
        assert!(pending.is_empty());
    }

    #[test]
    fn decode_log_increment_holds_back_a_split_multibyte_char() {
        // "é" is 0xC3 0xA9 in UTF-8 - split the two bytes across two "reads".
        let bytes: Vec<u8> = vec![b'c', b'a', b'f', 0xC3, 0xA9];
        let (first_chunk, second_chunk) = bytes.split_at(4);

        let mut pending = Vec::new();
        let first = decode_log_increment(TextEncoding::Utf8, &mut pending, first_chunk);
        assert_eq!(first, "caf");
        assert_eq!(pending, vec![0xC3]);

        let second = decode_log_increment(TextEncoding::Utf8, &mut pending, second_chunk);
        assert_eq!(second, "\u{e9}");
        assert!(pending.is_empty());
    }

    #[test]
    fn decode_log_increment_holds_back_an_odd_utf16_byte() {
        let text = "hi";
        let bytes = TextEncoding::Utf16Le.encode(text);
        // Strip the BOM `encode` adds so this looks like a mid-stream read, then split
        // mid-code-unit (an odd number of bytes).
        let body = &bytes[2..];
        let (first_chunk, second_chunk) = body.split_at(1);

        let mut pending = Vec::new();
        let first = decode_log_increment(TextEncoding::Utf16Le, &mut pending, first_chunk);
        assert_eq!(first, "");
        assert_eq!(pending, first_chunk.to_vec());

        let second = decode_log_increment(TextEncoding::Utf16Le, &mut pending, second_chunk);
        assert_eq!(second, "hi");
        assert!(pending.is_empty());
    }

    #[test]
    fn decode_log_increment_handles_windows_1252_high_bytes() {
        let mut pending = Vec::new();
        // 0xE9 is 'é' in Windows-1252.
        let decoded = decode_log_increment(TextEncoding::Windows1252, &mut pending, &[0xE9]);
        assert_eq!(decoded, "é");
        assert!(pending.is_empty());
    }

    #[test]
    fn word_range_selects_the_word_the_cursor_is_inside() {
        let text = "let hello_world = 1;";
        // Cursor in the middle of "hello_world" (char index 8, inside "hello_world").
        assert_eq!(word_range_at_char_index(text, 8), Some((4, 15)));
    }

    #[test]
    fn word_range_at_either_edge_of_a_word_still_selects_it() {
        let text = "foo bar";
        assert_eq!(word_range_at_char_index(text, 0), Some((0, 3))); // start of "foo"
        assert_eq!(word_range_at_char_index(text, 3), Some((0, 3))); // end of "foo"
        assert_eq!(word_range_at_char_index(text, 4), Some((4, 7))); // start of "bar"
    }

    #[test]
    fn word_range_on_whitespace_between_words_spans_both() {
        // Matches egui's own double-click/Ctrl+Right word-boundary semantics: clicking in
        // a whitespace gap extends to the surrounding words on both sides, rather than
        // selecting nothing (the previous, simpler implementation's behavior).
        let text = "foo   bar";
        assert_eq!(word_range_at_char_index(text, 4), Some((0, 9)));
    }

    #[test]
    fn word_range_on_whitespace_with_nothing_around_selects_the_run() {
        // A lone whitespace token still gets selected as "the thing under the cursor" -
        // egui's algorithm doesn't special-case "no word anywhere nearby" into `None`,
        // only an exactly empty range (nothing at all to select) becomes `None`.
        let text = "   ";
        assert_eq!(word_range_at_char_index(text, 1), Some((0, 3)));
    }

    #[test]
    fn word_range_handles_start_and_end_of_document() {
        let text = "abc";
        assert_eq!(word_range_at_char_index(text, 0), Some((0, 3)));
        assert_eq!(word_range_at_char_index(text, 3), Some((0, 3)));
        assert_eq!(word_range_at_char_index("", 0), None);
    }

    #[test]
    fn word_range_handles_unicode_letters() {
        // `char::is_alphanumeric` (what `is_word_char` is built on) is Unicode-aware, so
        // accented/non-Latin letters are treated as word characters just like ASCII ones.
        let text = "cà phê sáng";
        assert_eq!(word_range_at_char_index(text, 1), Some((0, 2))); // inside "cà"
        assert_eq!(word_range_at_char_index(text, 4), Some((3, 6))); // inside "phê"
    }

    #[test]
    fn word_range_treats_dot_as_a_boundary_like_egui_does() {
        // Mirrors egui's own quirk (matching how e.g. macOS lets you select just the
        // `example` part of a domain) instead of selecting the whole dotted run.
        let text = "www.example.com";
        assert_eq!(word_range_at_char_index(text, 6), Some((4, 11)));
    }

    #[test]
    fn word_range_stops_at_non_word_punctuation() {
        // A hyphen isn't a word char and isn't the special-cased `.`, so it's a plain
        // boundary - clicking inside "foo" doesn't pull in "bar" across the `-`.
        let text = "foo-bar";
        assert_eq!(word_range_at_char_index(text, 1), Some((0, 3)));
    }

    #[test]
    fn word_range_is_bounded_to_the_clicked_line() {
        // A `\n` is always a hard boundary - a word on one line is never pulled into a
        // selection that started on a different line.
        let text = "first\nsecond";
        assert_eq!(word_range_at_char_index(text, 1), Some((0, 5)));
        assert_eq!(word_range_at_char_index(text, 8), Some((6, 12)));
    }
}
