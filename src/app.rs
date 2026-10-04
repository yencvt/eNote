use std::fs;
use std::path::PathBuf;

use eframe::egui;
use egui_extras::syntax_highlighting::CodeTheme;
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

use crate::autocomplete::AutocompleteState;
use crate::document::Document;
use crate::encoding::TextEncoding;
use crate::find_replace::FindReplaceState;
use crate::hashing::HashAlgorithm;
use crate::language::Language;
use crate::session::{SessionData, SessionTab, session_file_path};

/// Which conversion the "Convert..." Utilities dialog performs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ConvertMode {
    #[default]
    TimestampToDateTime,
    DateTimeToTimestamp,
    ReformatDateTime,
}

impl ConvertMode {
    pub const ALL: [ConvertMode; 3] = [
        ConvertMode::TimestampToDateTime,
        ConvertMode::DateTimeToTimestamp,
        ConvertMode::ReformatDateTime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConvertMode::TimestampToDateTime => "Timestamp -> Date/Time",
            ConvertMode::DateTimeToTimestamp => "Date/Time -> Timestamp",
            ConvertMode::ReformatDateTime => "Reformat Date/Time",
        }
    }
}

#[derive(Default)]
pub struct EditorContextActions {
    pub copy: bool,
    pub cut: bool,
    pub paste: bool,
    pub delete: bool,
    pub select_all: bool,
    pub find: bool,
    pub replace: bool,
    pub goto: bool,
    pub pin_toggle: bool,
    pub close_tab: bool,
    pub toggle_bookmark: bool,
    pub base64_encode: bool,
    pub base64_decode: bool,
    pub request_hash: Option<HashAlgorithm>,
    pub format_document: bool,
    pub request_bcrypt_hash: bool,
    pub request_bcrypt_verify: bool,
    pub request_jwt_decode: bool,
    pub request_jws_generator: bool,
    pub request_jws_verify: bool,
    pub request_convert: bool,
}

pub struct NotePageApp {
    pub tabs: Vec<Document>,
    pub active_tab: usize,
    pub untitled_counter: usize,
    pub next_tab_id: u64,
    pub status_message: String,
    pub word_wrap: bool,
    pub dark_mode: bool,
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    pub show_about: bool,
    pub show_plugins_admin: bool,
    pub show_find_panel: bool,
    pub show_goto_panel: bool,
    pub find: FindReplaceState,
    pub goto_line_input: String,
    pub goto_column_input: String,
    pub recent_files: Vec<PathBuf>,
    pub session_path: Option<PathBuf>,
    pub code_theme: CodeTheme,
    pub editor_context_menu_open: bool,
    pub editor_context_menu_pos: egui::Pos2,
    pub editor_context_menu_tab: usize,
    pub last_editor_rect: Option<egui::Rect>,
    // State for the "Base64" tool: a standalone text-in/text-out dialog (not auto-applied
    // to the document) - opening it (from the context menu, the top "Utilities" menu, or
    // re-running Encode/Decode inside the dialog) snapshots which tab/char range to offer
    // "Insert into Editor" for, but the input box itself is freely editable/pasteable.
    pub show_base64_dialog: bool,
    pub base64_input: String,
    pub base64_output: Option<Result<String, String>>,
    pub base64_target_tab: usize,
    pub base64_target_range: (usize, usize),
    // State for the "Hash (SHA)" tool: `hash_input` is prefilled from the selection/whole
    // document for convenience but is freely editable, so you can hash arbitrary typed
    // text too. "Insert into Editor" (optional) writes the result back to the tab/range
    // captured when the dialog opened.
    pub show_hash_dialog: bool,
    pub hash_dialog_algo: Option<HashAlgorithm>,
    pub hash_dialog_tab: usize,
    pub hash_dialog_range: (usize, usize),
    pub hash_input: String,
    pub hash_key_input: String,
    pub hash_output: Option<String>,
    // State for the Bcrypt "Hash..." tool: mirrors the SHA/MD5 hash dialog above (editable
    // input, optional "Insert into Editor"), but prompts for a cost factor instead of an
    // HMAC key.
    pub show_bcrypt_hash_dialog: bool,
    pub bcrypt_hash_tab: usize,
    pub bcrypt_hash_range: (usize, usize),
    pub bcrypt_hash_input: String,
    pub bcrypt_cost_input: String,
    pub bcrypt_hash_error: Option<String>,
    pub bcrypt_hash_output: Option<String>,
    // State for the Bcrypt "Verify..." tool: doesn't touch the document, just reports
    // whether the typed candidate password matches the (freely editable, prefilled) hash.
    pub show_bcrypt_verify_dialog: bool,
    pub bcrypt_verify_source: String,
    pub bcrypt_verify_candidate: String,
    pub bcrypt_verify_result: Option<Result<bool, String>>,
    // State for the "JWT > Decode..." context menu flow: a read-only view (like jwt.io)
    // of the captured token's header/payload/signature, with an optional HMAC secret box
    // to verify HS256/384/512 signatures. Never edits the document.
    pub show_jwt_dialog: bool,
    pub jwt_source: String,
    pub jwt_decoded: Option<Result<crate::jwt::JwtDecoded, String>>,
    pub jwt_secret_input: String,
    pub jwt_verify_result: Option<Result<bool, String>>,
    // State for the "JWS Generator..." context menu flow: an interactive signer (like
    // 8gwifi.org's JWS generator) that lets you pick an algorithm, paste a key, and edit a
    // payload, then view the resulting compact JWS. Never edits the document - the
    // selection/whole document is only used to prefill the payload box when it opens.
    pub show_jws_dialog: bool,
    pub jws_algorithm: crate::jws::JwsAlgorithm,
    pub jws_key_input: String,
    pub jws_payload_input: String,
    pub jws_detached: bool,
    pub jws_result: Option<Result<String, String>>,
    // State for the "Verify JWS..." context menu flow: checks a JWS's signature against a
    // key (HMAC secret, or RSA/EC public key - a private key also works). The algorithm is
    // read from the token's own header rather than chosen by the user. If the token has a
    // detached payload (a two-segment `header.signature` token), the user must also paste
    // the original payload to verify against.
    pub show_jws_verify_dialog: bool,
    pub jws_verify_token: String,
    pub jws_verify_key_input: String,
    pub jws_verify_detached_payload: String,
    pub jws_verify_result: Option<Result<bool, String>>,
    // State for the "Convert..." Utilities tool: timestamp <-> date/time <-> custom
    // formatted string, similar in spirit to 8gwifi.org's converters. `convert_input` is
    // prefilled from the selection/whole document but freely editable. Never edits the
    // document unless "Insert into Editor" is used.
    pub show_convert_dialog: bool,
    pub convert_mode: ConvertMode,
    pub convert_input: String,
    pub convert_timestamp_unit: crate::datetime_convert::TimestampUnit,
    pub convert_timezone: crate::datetime_convert::TimeZoneChoice,
    pub convert_input_format: String,
    pub convert_output_format: String,
    pub convert_output: Option<Result<String, String>>,
    pub convert_target_tab: usize,
    pub convert_target_range: (usize, usize),
    // Set by the menu bar/toolbar/shortcut (which don't have access to the editor's local
    // `text` buffer); consumed by editor_view on the next frame, same pattern as
    // `pending_goto`.
    pub request_format_document: bool,
    // Likewise, set by the top "Utilities" menu (which, unlike the editor's right-click
    // context menu, has no `EditorContextActions` to populate) for each Utilities tool;
    // editor_view treats these the same as their `EditorContextActions` counterpart and
    // clears them once consumed.
    pub pending_base64_encode: bool,
    pub pending_base64_decode: bool,
    pub pending_hash: Option<HashAlgorithm>,
    pub pending_bcrypt_hash: bool,
    pub pending_bcrypt_verify: bool,
    pub pending_jwt_decode: bool,
    pub pending_jws_generator: bool,
    pub pending_jws_verify: bool,
    pub pending_convert: bool,
    pub autocomplete: AutocompleteState,
    // Screen-space position of the caret, captured in editor_view right after the galley
    // is available, so the autocomplete popup can anchor to the real cursor instead of a
    // fixed offset from the editor's corner.
    pub autocomplete_screen_pos: Option<egui::Pos2>,
    pub font_size: f32,
    // Set by `jump_to_line` (bookmark navigation); consumed by editor_view on the next
    // frame to move the real caret since that requires the TextEdit's egui state.
    pub pending_goto: bool,
    pub request_paste: bool,
    // Non-empty (2+ entries) means the central panel shows these tab indices side by side
    // instead of the single active tab. Each entry is a tab index into `self.tabs`.
    pub split_panes: Vec<usize>,
    // Only meaningful when `split_panes.len() == 2`: overlays line-level diff highlighting
    // (computed from `crate::diff`) on both panes.
    pub compare_mode: bool,
    pub compare_result: Option<crate::diff::DiffResult>,
    pub show_compare_picker: bool,
    pub compare_pick_left: usize,
    pub compare_pick_right: usize,
    // Shared scroll position as a FRACTION (0.0-1.0) of each pane's own scrollable range,
    // not a raw pixel offset - needed because the two compared files can have very
    // different line counts/heights, and syncing by raw pixels made the shorter pane's
    // scroll clamp and fight the longer pane every frame. `compare_pane_max_range` caches
    // each pane's `content_size.y - viewport_height` from the PREVIOUS frame (needed
    // because `vertical_scroll_offset` must be set before `.show()`, i.e. before this
    // frame's content size is known). `compare_scroll_force` is set for exactly one frame
    // after any pane's own scrolling diverges from the shared fraction, so the other
    // pane(s) catch up.
    pub compare_scroll_fraction: f32,
    pub compare_scroll_force: bool,
    pub compare_pane_max_range: [f32; 2],
    // `ctx.input(|i| i.time)` (seconds since startup) at the last periodic session
    // autosave - see `maybe_autosave_session`'s doc comment for why this exists on top
    // of the on-exit save in `on_exit`.
    pub last_session_save_time: f64,
}

impl NotePageApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        // Clicking a button (menu bar, context menu, tab bar, ...) must never silently
        // steal focus from the editor, since egui only paints the selection highlight
        // while the TextEdit has focus.
        cc.egui_ctx.memory_mut(|mem| {
            mem.options.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never;
        });

        let mut app = Self {
            tabs: vec![Document::untitled(1)],
            active_tab: 0,
            untitled_counter: 2,
            next_tab_id: 1,
            status_message: "Ready".to_string(),
            word_wrap: true,
            dark_mode: true,
            show_toolbar: true,
            show_status_bar: true,
            show_about: false,
            show_plugins_admin: false,
            show_find_panel: false,
            show_goto_panel: false,
            find: FindReplaceState::default(),
            goto_line_input: "1".to_string(),
            goto_column_input: "1".to_string(),
            recent_files: Vec::new(),
            session_path: session_file_path(),
            code_theme: CodeTheme::dark(13.0),
            editor_context_menu_open: false,
            editor_context_menu_pos: egui::pos2(0.0, 0.0),
            editor_context_menu_tab: 0,
            last_editor_rect: None,
            show_base64_dialog: false,
            base64_input: String::new(),
            base64_output: None,
            base64_target_tab: 0,
            base64_target_range: (0, 0),
            show_hash_dialog: false,
            hash_dialog_algo: None,
            hash_dialog_tab: 0,
            hash_dialog_range: (0, 0),
            hash_input: String::new(),
            hash_key_input: String::new(),
            hash_output: None,
            show_bcrypt_hash_dialog: false,
            bcrypt_hash_tab: 0,
            bcrypt_hash_range: (0, 0),
            bcrypt_hash_input: String::new(),
            bcrypt_cost_input: crate::hashing::DEFAULT_BCRYPT_COST.to_string(),
            bcrypt_hash_error: None,
            bcrypt_hash_output: None,
            show_bcrypt_verify_dialog: false,
            bcrypt_verify_source: String::new(),
            bcrypt_verify_candidate: String::new(),
            bcrypt_verify_result: None,
            show_jwt_dialog: false,
            jwt_source: String::new(),
            jwt_decoded: None,
            jwt_secret_input: String::new(),
            jwt_verify_result: None,
            show_jws_dialog: false,
            jws_algorithm: crate::jws::JwsAlgorithm::Hs256,
            jws_key_input: String::new(),
            jws_payload_input: String::new(),
            jws_detached: false,
            jws_result: None,
            show_jws_verify_dialog: false,
            jws_verify_token: String::new(),
            jws_verify_key_input: String::new(),
            jws_verify_detached_payload: String::new(),
            jws_verify_result: None,
            show_convert_dialog: false,
            convert_mode: ConvertMode::default(),
            convert_input: String::new(),
            convert_timestamp_unit: crate::datetime_convert::TimestampUnit::Seconds,
            convert_timezone: crate::datetime_convert::TimeZoneChoice::default(),
            convert_input_format: "%Y-%m-%d %H:%M:%S".to_string(),
            convert_output_format: "%Y-%m-%d %H:%M:%S".to_string(),
            convert_output: None,
            convert_target_tab: 0,
            convert_target_range: (0, 0),
            request_format_document: false,
            pending_base64_encode: false,
            pending_base64_decode: false,
            pending_hash: None,
            pending_bcrypt_hash: false,
            pending_bcrypt_verify: false,
            pending_jwt_decode: false,
            pending_jws_generator: false,
            pending_jws_verify: false,
            pending_convert: false,
            autocomplete: AutocompleteState::default(),
            autocomplete_screen_pos: None,
            font_size: 13.0,
            pending_goto: false,
            request_paste: false,
            split_panes: Vec::new(),
            compare_mode: false,
            compare_result: None,
            show_compare_picker: false,
            compare_pick_left: 0,
            compare_pick_right: 0,
            compare_scroll_fraction: 0.0,
            compare_scroll_force: false,
            compare_pane_max_range: [0.0, 0.0],
            last_session_save_time: 0.0,
        };
        app.tabs[0].id = 0;

        app.restore_session();

        if app.dark_mode {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            app.code_theme = CodeTheme::dark(app.font_size);
        } else {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            app.code_theme = CodeTheme::light(app.font_size);
        }

        app
    }

    pub fn current_tab_mut(&mut self) -> &mut Document {
        &mut self.tabs[self.active_tab]
    }

    pub fn current_tab(&self) -> &Document {
        &self.tabs[self.active_tab]
    }

    pub fn allocate_tab_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        id
    }

    pub fn add_new_tab(&mut self) {
        let mut tab = Document::untitled(self.untitled_counter);
        tab.id = self.allocate_tab_id();
        self.tabs.push(tab);
        self.untitled_counter += 1;
        self.active_tab = self.tabs.len() - 1;
        self.status_message = "New file".to_string();
    }

    pub fn open_file(&mut self) {
        if let Some(path) = FileDialog::new().pick_file() {
            self.open_file_by_path(path);
        }
    }

    pub fn open_file_by_path(&mut self, path: PathBuf) {
        match fs::read(&path) {
            Ok(bytes) => {
                let mut tab = Document::from_path_bytes(path.clone(), &bytes);
                tab.id = self.allocate_tab_id();
                self.tabs.push(tab);
                self.active_tab = self.tabs.len() - 1;
                self.track_recent_file(path.clone());
                self.status_message = format!("Opened {}", path.display());
            }
            Err(err) => {
                self.status_message = format!("Failed to open file: {err}");
            }
        }
    }

    /// Opens `path` at startup - e.g. launched via the "Open with eNote" Explorer
    /// right-click entry (see `shell_integration`), or any other command-line invocation
    /// with a file argument. If the only tab open is still the blank, untouched "Untitled"
    /// tab `new()` always creates (i.e. there was no session to restore), replaces it
    /// instead of leaving an extra empty tab sitting around next to the opened file.
    pub fn open_file_from_cli(&mut self, path: PathBuf) {
        if self.tabs.len() == 1 && self.tabs[0].path.is_none() && !self.tabs[0].is_dirty() {
            self.tabs.clear();
        }
        self.open_file_by_path(path);
    }

    /// "View Log (tail -f)...": opens a file picker, then opens the chosen file the same
    /// way `open_file_by_path` does, but also marks the new tab as a live log view (see
    /// `Document`'s `is_log_view`/`log_*` fields and `editor_view::poll_log_tail`) so it
    /// keeps appending newly written bytes and auto-scrolling while it stays open.
    pub fn open_log_file(&mut self) {
        if let Some(path) = FileDialog::new().pick_file() {
            self.open_log_file_by_path(path);
        }
    }

    pub fn open_log_file_by_path(&mut self, path: PathBuf) {
        match fs::read(&path) {
            Ok(bytes) => {
                let mut tab = Document::from_path_bytes(path.clone(), &bytes);
                tab.id = self.allocate_tab_id();
                tab.is_log_view = true;
                tab.log_follow = true;
                tab.log_read_bytes = bytes.len() as u64;
                self.tabs.push(tab);
                self.active_tab = self.tabs.len() - 1;
                self.track_recent_file(path.clone());
                self.status_message = format!("Watching {} for changes (tail -f)", path.display());
            }
            Err(err) => {
                self.status_message = format!("Failed to open log file: {err}");
            }
        }
    }

    pub fn save_current(&mut self) {
        let path = self.current_tab().path.clone();
        if let Some(path) = path {
            self.save_to_path(path);
        } else {
            self.save_current_as();
        }
    }

    pub fn save_current_as(&mut self) {
        let title = self.current_tab().title.clone();
        let dialog = FileDialog::new().set_file_name(&title);
        if let Some(path) = dialog.save_file() {
            self.save_to_path(path);
        }
    }

    fn save_to_path(&mut self, path: PathBuf) {
        let data = self.current_tab().bytes_for_saving();
        match fs::write(&path, data) {
            Ok(_) => {
                let tab = self.current_tab_mut();
                tab.path = Some(path.clone());
                if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                    tab.title = filename.to_string();
                }
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    tab.language = Language::from_path(ext);
                }
                tab.dirty = false;
                self.track_recent_file(path.clone());
                self.status_message = format!("Saved {}", path.display());
            }
            Err(err) => {
                self.status_message = format!("Failed to save file: {err}");
            }
        }
    }

    pub fn close_active_tab(&mut self) {
        self.close_tab(self.active_tab);
    }

    pub fn close_tab(&mut self, tab_index: usize) {
        if self.tabs.is_empty() || tab_index >= self.tabs.len() {
            return;
        }

        if self.tabs[tab_index].is_dirty() {
            let result = MessageDialog::new()
                .set_level(MessageLevel::Warning)
                .set_title("Unsaved changes")
                .set_description("This file has unsaved changes. Close anyway?")
                .set_buttons(MessageButtons::YesNo)
                .show();

            if result != MessageDialogResult::Yes {
                return;
            }
        }

        self.tabs.remove(tab_index);
        if self.tabs.is_empty() {
            let mut tab = Document::untitled(self.untitled_counter);
            tab.id = self.allocate_tab_id();
            self.tabs.push(tab);
            self.untitled_counter += 1;
            self.active_tab = 0;
        } else if tab_index < self.active_tab {
            self.active_tab -= 1;
        } else if tab_index == self.active_tab && self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }

        self.status_message = "Tab closed".to_string();
    }

    /// Requests that the active tab be reformatted on the next frame (see
    /// `request_format_document`'s doc comment). Routed through here so every trigger
    /// (menu, toolbar, shortcut) shares the same split-view guard: `draw_editor`, which
    /// consumes the flag, isn't rendered while a split/compare view is showing.
    pub fn request_format(&mut self) {
        if self.split_panes.len() >= 2 {
            self.status_message =
                "Format Document isn't available while split view is active".to_string();
            return;
        }
        self.request_format_document = true;
    }

    /// Guard shared by the top "Utilities" menu's tool entries: like `request_format`,
    /// their `pending_*` flags are only consumed by `draw_editor`, which isn't rendered
    /// while a split/compare view is showing. Returns `true` (and sets a status message)
    /// when the tool can't run right now, so callers can skip setting their flag.
    pub fn utilities_unavailable_in_split_view(&mut self, tool: &str) -> bool {
        if self.split_panes.len() >= 2 {
            self.status_message = format!("{tool} isn't available while split view is active");
            true
        } else {
            false
        }
    }

    pub fn apply_shortcuts(&mut self, ctx: &egui::Context) {
        let (
            save_as,
            save,
            new_tab,
            open,
            close,
            find,
            replace,
            goto,
            bookmark,
            next_bookmark,
            prev_bookmark,
            zoom_in,
            zoom_out,
            zoom_reset,
            format_document,
        ) = ctx.input_mut(|input| {
            (
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers {
                        ctrl: true,
                        shift: true,
                        ..Default::default()
                    },
                    egui::Key::S,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::S,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::N,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::O,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::W,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::F,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::H,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::G,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::F2,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::NONE,
                    egui::Key::F2,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::SHIFT,
                    egui::Key::F2,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::Plus,
                )) || input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::Equals,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::Minus,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::Num0,
                )),
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers {
                        ctrl: true,
                        alt: true,
                        ..Default::default()
                    },
                    egui::Key::L,
                )),
            )
        });

        if save_as {
            self.save_current_as();
        } else if save {
            self.save_current();
        }
        if new_tab {
            self.add_new_tab();
        }
        if open {
            self.open_file();
        }
        if close {
            self.close_active_tab();
        }
        if find || replace {
            self.show_find_panel = true;
        }
        if goto {
            self.show_goto_panel = true;
        }
        if bookmark {
            self.toggle_bookmark_on_current_line();
        }
        if next_bookmark {
            self.goto_next_bookmark();
        }
        if prev_bookmark {
            self.goto_prev_bookmark();
        }
        if zoom_in {
            self.zoom_in(ctx);
        }
        if zoom_out {
            self.zoom_out(ctx);
        }
        if zoom_reset {
            self.zoom_reset(ctx);
        }
        if format_document {
            self.request_format();
        }
    }

    pub fn track_recent_file(&mut self, path: PathBuf) {
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        if self.recent_files.len() > 12 {
            self.recent_files.truncate(12);
        }
    }

    pub fn restore_session(&mut self) {
        let Some(path) = &self.session_path else {
            return;
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };
        let Ok(session) = serde_json::from_str::<SessionData>(&contents) else {
            return;
        };

        self.word_wrap = session.word_wrap;
        self.dark_mode = session.dark_mode;
        self.show_toolbar = session.show_toolbar;
        self.show_status_bar = session.show_status_bar;

        self.recent_files = session
            .recent_files
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect::<Vec<_>>();

        let restored_tabs = session
            .open_tabs
            .into_iter()
            .filter_map(Self::restore_tab)
            .collect::<Vec<_>>();

        if !restored_tabs.is_empty() {
            self.tabs = restored_tabs;
            for tab in &mut self.tabs {
                tab.id = self.next_tab_id;
                self.next_tab_id += 1;
            }
            self.active_tab = if session.active_index < self.tabs.len() {
                session.active_index
            } else if let Some(active_file) = session.active_file {
                let active_path = PathBuf::from(active_file);
                self.tabs
                    .iter()
                    .position(|t| t.path.as_ref().is_some_and(|p| p == &active_path))
                    .unwrap_or(0)
            } else {
                0
            };
            self.status_message = "Session restored".to_string();
        }
    }

    /// Reconstructs one tab from its persisted snapshot. Unlike a plain re-open from
    /// disk, this restores any *unsaved* content exactly as it was - either an
    /// entirely never-saved "Untitled" tab (`entry.path` empty, so `entry.text` is the
    /// only copy of its content), or unsaved edits to a file that's still on disk
    /// (`entry.dirty`) - so closing the app without saving no longer loses that work,
    /// matching Notepad++'s session backup. A *clean* (saved, unmodified) file-backed tab
    /// is simply re-opened from disk instead of trusting the (redundant) persisted text,
    /// so it still picks up any changes made to the file outside the app meanwhile.
    fn restore_tab(entry: SessionTab) -> Option<Document> {
        if entry.path.is_empty() {
            let mut tab = Document::untitled(0);
            tab.title = if entry.title.is_empty() {
                "Untitled".to_string()
            } else {
                entry.title
            };
            let dirty = !entry.text.is_empty();
            tab.text = entry.text;
            tab.dirty = dirty;
            tab.recompute_size_caches();
            tab.pinned = entry.pinned;
            tab.cursor_char_index = entry.cursor_char_index.min(tab.cached_char_count);
            tab.recompute_cursor_cache();
            tab.language = entry.language;
            tab.encoding = entry.encoding;
            tab.line_ending = entry.line_ending;
            return Some(tab);
        }

        let path = PathBuf::from(&entry.path);
        let bytes = fs::read(&path).ok()?;
        let mut tab = Document::from_path_bytes(path, &bytes);
        tab.pinned = entry.pinned;
        if entry.dirty {
            tab.text = entry.text;
            tab.dirty = true;
            tab.recompute_size_caches();
            tab.language = entry.language;
            tab.encoding = entry.encoding;
            tab.line_ending = entry.line_ending;
            tab.cursor_char_index = entry.cursor_char_index.min(tab.cached_char_count);
            tab.recompute_cursor_cache();
        }
        Some(tab)
    }

    pub fn persist_session(&self) {
        let Some(path) = &self.session_path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let data = SessionData {
            recent_files: self
                .recent_files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            open_tabs: self
                .tabs
                .iter()
                .map(|tab| {
                    let dirty = tab.is_dirty();
                    let untitled = tab.path.is_none();
                    SessionTab {
                        path: tab
                            .path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        pinned: tab.pinned,
                        title: tab.title.clone(),
                        // Only worth persisting the actual buffer when it holds content
                        // that would otherwise be lost (never-saved, or unsaved edits) -
                        // a clean file-backed tab is just re-read from disk on restore.
                        text: if untitled || dirty {
                            tab.text.clone()
                        } else {
                            String::new()
                        },
                        dirty,
                        cursor_char_index: tab.cursor_char_index,
                        language: tab.language,
                        encoding: tab.encoding,
                        line_ending: tab.line_ending,
                    }
                })
                .collect(),
            active_file: self
                .current_tab()
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            active_index: self.active_tab,
            word_wrap: self.word_wrap,
            dark_mode: self.dark_mode,
            show_toolbar: self.show_toolbar,
            show_status_bar: self.show_status_bar,
        };

        if let Ok(serialized) = serde_json::to_string_pretty(&data) {
            let _ = fs::write(path, serialized);
        }
    }

    /// Periodically re-persists the session (same data as `persist_session`, including
    /// each dirty/untitled tab's actual buffer) every few seconds while the app is
    /// running - not just on a clean exit. Without this, a crash (or the OS/user killing
    /// the process) would lose unsaved work exactly like it did before, since `on_exit`
    /// never gets a chance to run; Notepad++'s own session backup saves continuously for
    /// the same reason.
    pub fn maybe_autosave_session(&mut self, ctx: &egui::Context) {
        const AUTOSAVE_INTERVAL_SECS: f64 = 5.0;
        let now = ctx.input(|i| i.time);
        if now - self.last_session_save_time >= AUTOSAVE_INTERVAL_SECS {
            self.persist_session();
            self.last_session_save_time = now;
        }
    }

    /// Returns the current tab's caret position as (line, column), from the cache
    /// maintained in `editor_view::draw_editor` (updated whenever the caret moves or the
    /// text changes) instead of recomputing it from scratch here - this used to run an
    /// O(document length) scan on every single frame just to paint the status bar.
    pub fn line_col(&self) -> (usize, usize) {
        let tab = self.current_tab();
        (tab.cached_cursor_line, tab.cached_cursor_col)
    }

    pub fn toggle_bookmark_on_current_line(&mut self) {
        let (line, _) = self.line_col();
        let line0 = line.saturating_sub(1);
        let tab = self.current_tab_mut();
        if !tab.bookmarks.remove(&line0) {
            tab.bookmarks.insert(line0);
        }
    }

    pub fn goto_next_bookmark(&mut self) {
        let (line, _) = self.line_col();
        let current = line.saturating_sub(1);
        let tab = self.current_tab();
        if let Some(&next) = tab.bookmarks.iter().find(|&&l| l > current) {
            self.jump_to_line(next + 1);
        } else if let Some(&first) = tab.bookmarks.iter().next() {
            self.jump_to_line(first + 1);
        }
    }

    pub fn goto_prev_bookmark(&mut self) {
        let (line, _) = self.line_col();
        let current = line.saturating_sub(1);
        let tab = self.current_tab();
        if let Some(&prev) = tab.bookmarks.iter().rev().find(|&&l| l < current) {
            self.jump_to_line(prev + 1);
        } else if let Some(&last) = tab.bookmarks.iter().next_back() {
            self.jump_to_line(last + 1);
        }
    }

    pub fn clear_all_bookmarks(&mut self) {
        self.current_tab_mut().bookmarks.clear();
    }

    /// Requests a cursor jump that `editor_view::draw_editor` applies on the next frame.
    pub fn jump_to_line(&mut self, line: usize) {
        self.goto_line_input = line.to_string();
        self.goto_column_input = "1".to_string();
        self.pending_goto = true;
    }

    pub fn set_dark_mode(&mut self, ctx: &egui::Context, dark: bool) {
        self.dark_mode = dark;
        if dark {
            ctx.set_visuals(egui::Visuals::dark());
            self.code_theme = CodeTheme::dark(self.font_size);
        } else {
            ctx.set_visuals(egui::Visuals::light());
            self.code_theme = CodeTheme::light(self.font_size);
        }
    }

    /// Applies a newly-chosen encoding from the "Encoding" menu. For a file-backed tab,
    /// this re-reads the file's raw bytes from disk and re-decodes them using exactly the
    /// chosen encoding (not the auto-detection `decode` uses when first opening a file),
    /// so e.g. picking "ANSI (Windows-1252)" on a file that was mis-detected as UTF-8
    /// actually reinterprets the bytes and reloads the content - matching Notepad++'s
    /// "pick an encoding to reload with" behavior - instead of just relabeling already-
    /// decoded text, which silently left it unable to display or save correctly. Prompts
    /// for confirmation first if there are unsaved changes, since reloading discards them.
    /// For a never-saved (untitled) tab there's nothing on disk to reinterpret, so this
    /// just records the encoding to use whenever it's eventually saved.
    pub fn set_encoding(&mut self, encoding: TextEncoding) {
        let tab = self.current_tab();
        let Some(path) = tab.path.clone() else {
            self.current_tab_mut().encoding = encoding;
            self.status_message = format!("Encoding set to {}", encoding.label());
            return;
        };

        if tab.is_dirty() {
            let proceed = MessageDialog::new()
                .set_level(MessageLevel::Warning)
                .set_title("Unsaved changes")
                .set_description(
                    "Reloading with a different encoding will discard unsaved changes. Continue?",
                )
                .set_buttons(MessageButtons::YesNo)
                .show();
            if proceed != MessageDialogResult::Yes {
                return;
            }
        }

        match fs::read(&path) {
            Ok(bytes) => {
                let text = encoding.decode_with(&bytes);
                let line_ending = if text.contains("\r\n") {
                    crate::document::LineEnding::Crlf
                } else {
                    crate::document::LineEnding::Lf
                };
                let tab = self.current_tab_mut();
                tab.text = text;
                tab.encoding = encoding;
                tab.line_ending = line_ending;
                tab.dirty = false;
                tab.cursor_char_index = 0;
                tab.selection_char_range = None;
                tab.recompute_size_caches();
                tab.recompute_cursor_cache();
                self.status_message = format!("Reloaded with encoding {}", encoding.label());
            }
            Err(err) => {
                self.status_message = format!("Failed to reload file: {err}");
            }
        }
    }

    pub fn zoom_in(&mut self, ctx: &egui::Context) {
        self.font_size = (self.font_size + 1.0).min(36.0);
        self.refresh_theme_font(ctx);
    }

    pub fn zoom_out(&mut self, ctx: &egui::Context) {
        self.font_size = (self.font_size - 1.0).max(8.0);
        self.refresh_theme_font(ctx);
    }

    pub fn zoom_reset(&mut self, ctx: &egui::Context) {
        self.font_size = 13.0;
        self.refresh_theme_font(ctx);
    }

    fn refresh_theme_font(&mut self, _ctx: &egui::Context) {
        self.code_theme = if self.dark_mode {
            CodeTheme::dark(self.font_size)
        } else {
            CodeTheme::light(self.font_size)
        };
    }

    /// Splits the central panel into `count` side-by-side panes (2-4), seeded with the
    /// current active tab and whichever other tabs are open, cycling if there aren't
    /// enough distinct tabs. Turns off compare mode (only valid for exactly 2 panes).
    pub fn split_view(&mut self, count: usize) {
        let count = count.clamp(2, 4).min(self.tabs.len().max(1));
        self.split_panes = (0..count)
            .map(|i| (self.active_tab + i) % self.tabs.len())
            .collect();
        if count != 2 {
            self.compare_mode = false;
            self.compare_result = None;
        }
        self.status_message = format!("Split into {count} panes");
    }

    pub fn unsplit_view(&mut self) {
        self.split_panes.clear();
        self.compare_mode = false;
        self.compare_result = None;
        self.status_message = "Back to single view".to_string();
    }

    /// Opens the Compare picker dialog, defaulting to the current tab vs. the next one.
    pub fn open_compare_picker(&mut self) {
        self.compare_pick_left = self.active_tab;
        self.compare_pick_right = if self.tabs.len() > 1 {
            (self.active_tab + 1) % self.tabs.len()
        } else {
            self.active_tab
        };
        self.show_compare_picker = true;
    }

    pub fn start_compare(&mut self, left: usize, right: usize) {
        self.split_panes = vec![left, right];
        self.compare_mode = true;
        self.compare_scroll_fraction = 0.0;
        self.compare_scroll_force = true;
        self.compare_pane_max_range = [0.0, 0.0];
        self.recompute_compare();
        self.show_compare_picker = false;
    }

    pub fn recompute_compare(&mut self) {
        if !self.compare_mode || self.split_panes.len() != 2 {
            self.compare_result = None;
            return;
        }
        let left_text = &self.tabs[self.split_panes[0]].text;
        let right_text = &self.tabs[self.split_panes[1]].text;
        let result = crate::diff::diff_texts(left_text, right_text);
        self.status_message = format!("Compare: {} difference block(s)", result.count());
        self.compare_result = Some(result);
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn restore_tab_rebuilds_an_untitled_tab_from_its_persisted_text() {
        let entry = SessionTab {
            path: String::new(),
            pinned: true,
            title: "new 2".to_string(),
            text: "unsaved thoughts".to_string(),
            dirty: true,
            cursor_char_index: 5,
            language: Language::Rust,
            encoding: TextEncoding::Utf8,
            line_ending: crate::document::LineEnding::Lf,
        };

        let tab = NotePageApp::restore_tab(entry).expect("untitled tabs always restore");
        assert!(tab.path.is_none());
        assert_eq!(tab.title, "new 2");
        assert_eq!(tab.text, "unsaved thoughts");
        assert!(tab.is_dirty(), "never-saved content should show as unsaved");
        assert!(tab.pinned);
        assert_eq!(tab.cursor_char_index, 5);
        assert_eq!(tab.language, Language::Rust);
    }

    #[test]
    fn restore_tab_restores_unsaved_edits_over_the_on_disk_content() {
        let path = temp_file("notepagepp_test_restore_dirty.txt", "original disk content");
        let entry = SessionTab {
            path: path.to_string_lossy().to_string(),
            pinned: false,
            title: String::new(),
            text: "edited but never saved".to_string(),
            dirty: true,
            cursor_char_index: 0,
            language: Language::PlainText,
            encoding: TextEncoding::Utf8,
            line_ending: crate::document::LineEnding::Lf,
        };

        let tab = NotePageApp::restore_tab(entry).expect("file still exists on disk");
        assert_eq!(tab.text, "edited but never saved");
        assert!(
            tab.is_dirty(),
            "unsaved edits must still show as unsaved after restoring"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn restore_tab_rereads_disk_for_a_clean_tab() {
        let path = temp_file("notepagepp_test_restore_clean.txt", "saved content");
        let entry = SessionTab {
            path: path.to_string_lossy().to_string(),
            pinned: false,
            title: String::new(),
            text: String::new(),
            dirty: false,
            cursor_char_index: 0,
            language: Language::PlainText,
            encoding: TextEncoding::Utf8,
            line_ending: crate::document::LineEnding::Lf,
        };

        let tab = NotePageApp::restore_tab(entry).expect("file still exists on disk");
        assert_eq!(tab.text, "saved content");
        assert!(!tab.is_dirty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn restore_tab_skips_a_file_backed_entry_whose_file_is_gone() {
        let entry = SessionTab {
            path: "C:/definitely/does/not/exist/notepagepp.txt".to_string(),
            pinned: false,
            title: String::new(),
            text: String::new(),
            dirty: false,
            cursor_char_index: 0,
            language: Language::PlainText,
            encoding: TextEncoding::Utf8,
            line_ending: crate::document::LineEnding::Lf,
        };

        assert!(NotePageApp::restore_tab(entry).is_none());
    }
}
