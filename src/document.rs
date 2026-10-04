use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::encoding::TextEncoding;
use crate::language::Language;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "Unix (LF)",
            LineEnding::Crlf => "Windows (CRLF)",
        }
    }
}

#[derive(Clone)]
pub struct Document {
    // Stable identity independent of position in the tab list, so widget state
    // (cursor/scroll/undo) survives closing or reordering other tabs.
    pub id: u64,
    pub title: String,
    pub path: Option<PathBuf>,
    pub text: String,
    // Explicit "has this been edited since it was loaded/saved" flag, instead of
    // comparing `text` against a saved copy of the original content on every check (the
    // previous approach) - an O(document length) cost that was being paid every single
    // frame just to render the tab's "*" unsaved marker, which made the tab bar alone a
    // meaningful source of lag on large files. Kept in sync explicitly: cleared on
    // load/save, set whenever `text` is mutated (see `set_text` and the various direct
    // mutation sites that set it alongside their edit).
    pub dirty: bool,
    // Cached `text.chars().count()` / `text.lines().count()`, recomputed only when `text`
    // actually changes (see `recompute_size_caches`) instead of on every frame - the
    // status bar's length/line-count display used to recompute both from scratch (a full
    // scan each) every single frame regardless of whether anything changed.
    pub cached_char_count: usize,
    pub cached_line_count: usize,
    // Cached (line, column) for `cursor_char_index`, recomputed only when the cursor
    // moves or the text changes (see `recompute_cursor_cache`) instead of on every frame -
    // same reasoning as the size caches above, for the status bar's "Ln/Col" display.
    pub cached_cursor_line: usize,
    pub cached_cursor_col: usize,
    // The laid-out galley from the last frame, reused directly (see `editor_view`'s
    // `draw_editor`) whenever nothing that would change it - text, selection, word-wrap
    // mode/width, font size - changed since, instead of rebuilding the `LayoutJob` (which
    // copies the *entire* document text) and re-running egui's own layout-cache lookup
    // (which hashes that same text) on every single frame. Without this, just scrolling
    // or idling in a large file paid a full O(document length) cost on every repaint even
    // though nothing about the laid-out text had actually changed.
    pub cached_galley: Option<Arc<egui::Galley>>,
    pub cached_galley_text_len: usize,
    pub cached_galley_word_wrap: bool,
    pub cached_galley_wrap_width_bits: u32,
    pub cached_galley_font_size_bits: u32,
    pub cached_galley_pixels_per_point_bits: u32,
    // Caches the result of baking a *persisted* selection (see `bake_persisted_selection_if_needed`
    // in `editor_view.rs`) into a clone of `cached_galley` - e.g. while the right-click menu
    // is open. `Galley` derives `Clone` at the struct level, so `Arc::make_mut` (used by
    // egui's own `paint_text_selection`) clones its *entire* `Vec<PlacedRow>` - one entry
    // per line in the whole document - every time it's called on a shared Arc, regardless
    // of how small the selection itself is. Without this cache, that full-document-sized
    // clone was being redone on *every single frame* the menu stayed open (since nothing
    // else needed to change, nothing else invalidated it), which is what made opening the
    // context menu stutter/freeze on larger files.
    pub cached_baked_selection_base: Option<Arc<egui::Galley>>,
    pub cached_baked_selection_range: Option<(usize, usize)>,
    pub cached_baked_selection_result: Option<Arc<egui::Galley>>,
    pub line_ending: LineEnding,
    pub encoding: TextEncoding,
    pub language: Language,
    pub cursor_char_index: usize,
    pub selection_char_range: Option<(usize, usize)>,
    pub pinned: bool,
    pub overwrite_mode: bool,
    pub bookmarks: BTreeSet<usize>,
    // Fold ranges: (start_line, end_line) inclusive, 0-based. While any entry exists,
    // the editor is shown read-only to guarantee the underlying buffer can't desync
    // from the collapsed display (see ui/editor_view.rs).
    pub folds: BTreeSet<(usize, usize)>,
    // Alt+Shift+Up/Down and Alt+Shift+Click column-mode multi-caret / box-selection state.
    pub multi_cursor_active: bool,
    pub multi_cursor_column: usize,
    pub multi_cursor_lines: Vec<usize>,
    pub multi_cursor_origin_line: usize,
    pub multi_cursor_drag_anchor: Option<(usize, usize)>,
    pub multi_cursor_start_column: usize,
    // True only while the current column position came from the Alt+Shift+Click/drag
    // mouse gesture, which allows placing/typing past a short line's real end (Notepad++
    // virtual space). Keyboard-driven movement (arrow keys, Alt+Shift+Up/Down) clears this
    // so it clamps to real characters like before, per explicit user request.
    pub multi_cursor_virtual_space: bool,
    // Independent (non-column-aligned) extra carets added via Ctrl+Click, Notepad++-style
    // multi-editing. Char indices into `text`, NOT including the main `cursor_char_index`.
    pub extra_carets: Vec<usize>,
    // "View Log (tail -f)" mode: periodically re-reads `path` on disk (see
    // `editor_view::poll_log_tail`) and appends any bytes written since the last poll,
    // like `tail -f`. `log_read_bytes` is how many bytes of the file have already been
    // consumed; `log_pending_bytes` holds a trailing partial multi-byte character held
    // back from the last poll (so it isn't decoded until its remaining bytes arrive);
    // `log_follow` controls whether the view auto-scrolls to the bottom as content
    // arrives; `log_last_poll` throttles the disk check to roughly once per interval
    // instead of once per frame.
    pub is_log_view: bool,
    pub log_read_bytes: u64,
    pub log_pending_bytes: Vec<u8>,
    pub log_follow: bool,
    pub log_last_poll: Option<std::time::Instant>,
}

impl Document {
    pub fn untitled(index: usize) -> Self {
        Self {
            id: 0,
            title: format!("new {}", index),
            path: None,
            text: String::new(),
            dirty: false,
            cached_char_count: 0,
            cached_line_count: 1,
            cached_cursor_line: 1,
            cached_cursor_col: 1,
            cached_galley: None,
            cached_galley_text_len: 0,
            cached_galley_word_wrap: false,
            cached_galley_wrap_width_bits: 0,
            cached_galley_font_size_bits: 0,
            cached_galley_pixels_per_point_bits: 0,
            cached_baked_selection_base: None,
            cached_baked_selection_range: None,
            cached_baked_selection_result: None,
            line_ending: LineEnding::Lf,
            encoding: TextEncoding::Utf8,
            language: Language::PlainText,
            cursor_char_index: 0,
            selection_char_range: None,
            pinned: false,
            overwrite_mode: false,
            bookmarks: BTreeSet::new(),
            folds: BTreeSet::new(),
            multi_cursor_active: false,
            multi_cursor_column: 0,
            multi_cursor_lines: Vec::new(),
            multi_cursor_origin_line: 0,
            multi_cursor_drag_anchor: None,
            multi_cursor_start_column: 0,
            multi_cursor_virtual_space: false,
            extra_carets: Vec::new(),
            is_log_view: false,
            log_read_bytes: 0,
            log_pending_bytes: Vec::new(),
            log_follow: true,
            log_last_poll: None,
        }
    }

    pub fn from_path_bytes(path: PathBuf, bytes: &[u8]) -> Self {
        let (contents, encoding) = TextEncoding::decode(bytes);

        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("untitled")
            .to_string();

        let line_ending = if contents.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };

        let language = Language::from_path(
            path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default(),
        );

        let cached_char_count = contents.chars().count();
        let cached_line_count = contents.lines().count().max(1);

        Self {
            id: 0,
            title: filename,
            path: Some(path),
            text: contents,
            dirty: false,
            cached_char_count,
            cached_line_count,
            cached_cursor_line: 1,
            cached_cursor_col: 1,
            cached_galley: None,
            cached_galley_text_len: 0,
            cached_galley_word_wrap: false,
            cached_galley_wrap_width_bits: 0,
            cached_galley_font_size_bits: 0,
            cached_galley_pixels_per_point_bits: 0,
            cached_baked_selection_base: None,
            cached_baked_selection_range: None,
            cached_baked_selection_result: None,
            line_ending,
            encoding,
            language,
            cursor_char_index: 0,
            selection_char_range: None,
            pinned: false,
            overwrite_mode: false,
            bookmarks: BTreeSet::new(),
            folds: BTreeSet::new(),
            multi_cursor_active: false,
            multi_cursor_column: 0,
            multi_cursor_lines: Vec::new(),
            multi_cursor_origin_line: 0,
            multi_cursor_drag_anchor: None,
            multi_cursor_start_column: 0,
            multi_cursor_virtual_space: false,
            extra_carets: Vec::new(),
            is_log_view: false,
            log_read_bytes: 0,
            log_pending_bytes: Vec::new(),
            log_follow: true,
            log_last_poll: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Recomputes `cached_char_count`/`cached_line_count` from `text` - call after
    /// mutating `text` directly (code that goes through `set_text` gets this for free).
    /// Also invalidates `cached_galley` (see its doc comment) and the baked-selection
    /// cache (which is keyed to a specific `cached_galley` instance), since both were laid
    /// out from the old `text`.
    pub fn recompute_size_caches(&mut self) {
        self.cached_char_count = self.text.chars().count();
        self.cached_line_count = self.text.lines().count().max(1);
        self.cached_galley = None;
        self.cached_baked_selection_base = None;
        self.cached_baked_selection_range = None;
        self.cached_baked_selection_result = None;
    }

    /// Recomputes `cached_cursor_line`/`cached_cursor_col` from `cursor_char_index` - call
    /// after moving the cursor and/or mutating `text` (since a given char index can map to
    /// a different line/column once the text around it changes).
    pub fn recompute_cursor_cache(&mut self) {
        let clamped = self.cursor_char_index.min(self.cached_char_count);
        let (line, col) = crate::text_utils::line_col_for_char_index(&self.text, clamped);
        self.cached_cursor_line = line;
        self.cached_cursor_col = col;
    }

    /// Replaces `text` wholesale and keeps `dirty`/the size caches correctly in sync -
    /// the preferred way for code outside the main editor view (toolbar actions, the
    /// Base64/Hash/JWT/JWS "Insert into Editor" buttons, ...) to replace a tab's content.
    pub fn set_text(&mut self, new_text: String) {
        self.text = new_text;
        self.dirty = true;
        self.recompute_size_caches();
        self.recompute_cursor_cache();
    }

    pub fn display_title(&self) -> String {
        if self.is_dirty() {
            format!("*{}", self.title)
        } else {
            self.title.clone()
        }
    }

    pub fn bytes_for_saving(&self) -> Vec<u8> {
        let newline_normalized = match self.line_ending {
            LineEnding::Lf => self.text.replace("\r\n", "\n"),
            LineEnding::Crlf => {
                let lf_normalized = self.text.replace("\r\n", "\n");
                lf_normalized.replace('\n', "\r\n")
            }
        };
        self.encoding.encode(&newline_normalized)
    }

    pub fn exit_column_mode(&mut self) {
        self.multi_cursor_active = false;
        self.multi_cursor_lines.clear();
        self.multi_cursor_drag_anchor = None;
    }

    pub fn exit_multi_caret_mode(&mut self) {
        self.extra_carets.clear();
    }
}
