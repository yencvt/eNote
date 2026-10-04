use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::document::LineEnding;
use crate::encoding::TextEncoding;
use crate::language::Language;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionData {
    pub recent_files: Vec<String>,
    pub open_tabs: Vec<SessionTab>,
    pub active_file: Option<String>,
    /// Index into `open_tabs` of the active tab - the primary way to restore which tab
    /// was focused, since `active_file` alone can't identify an untitled tab.
    pub active_index: usize,
    pub word_wrap: bool,
    pub dark_mode: bool,
    pub show_toolbar: bool,
    pub show_status_bar: bool,
}

/// A snapshot of one open tab, persisted on every exit and restored on the next launch -
/// including its *actual buffer content*, so unsaved changes (or an entirely unsaved
/// "Untitled" tab) survive closing and reopening the app, just like Notepad++'s session
/// backup.
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionTab {
    /// Empty for a tab that was never saved to disk (an "Untitled" tab).
    pub path: String,
    pub pinned: bool,
    /// Display title for a never-saved tab (e.g. `"new 2"`); irrelevant when `path` is
    /// set, since the title is derived from the file name instead.
    pub title: String,
    /// The buffer's content at the time the session was saved - may differ from what's
    /// on disk (or, for a never-saved tab, be entirely new content) when `dirty` is true.
    pub text: String,
    /// Whether `text` had unsaved changes when the session was persisted. If false (a
    /// clean, saved tab), restoring re-reads the file from disk instead of using `text`,
    /// so any changes made to the file *outside* the app in the meantime aren't masked.
    pub dirty: bool,
    pub cursor_char_index: usize,
    pub language: Language,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
}

pub fn session_file_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|base| base.join("eNote").join("session.json"))
}
