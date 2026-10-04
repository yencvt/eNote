use eframe::egui;
use egui::Id;
use egui::text::CCursor;

use crate::app::NotePageApp;
use crate::hashing::compute_hash;
use crate::text_utils::{char_index_for_line_col, replace_char_range, set_editor_selection};
use base64::Engine as _;

impl NotePageApp {
    pub fn draw_find_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        if !self.show_find_panel {
            return;
        }

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Find:");
                ui.text_edit_singleline(&mut self.find.find_text);
                if ui.button("Find Next\tF3").clicked() {
                    self.run_find_next(ctx);
                }
                ui.checkbox(&mut self.find.use_regex, "Regex");
                ui.checkbox(&mut self.find.match_case, "Match case");
            });
            ui.horizontal(|ui| {
                ui.label("Replace:");
                ui.text_edit_singleline(&mut self.find.replace_text);
                if ui.button("Replace").clicked() {
                    self.run_replace_one(ctx);
                }
                if ui.button("Replace All").clicked() {
                    self.run_replace_all();
                }
                if ui.button("Close").clicked() {
                    self.show_find_panel = false;
                }
            });
        });
    }

    pub fn run_find_next(&mut self, ctx: &egui::Context) {
        let text = self.current_tab().text.clone();
        match self.find.find_next(&text) {
            Ok(Some((start, end))) => {
                let editor_id = Id::new(format!("editor-{}", self.current_tab().id));
                let start_char = text[..start].chars().count();
                let end_char = text[..end].chars().count();
                if let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) {
                    state.cursor.set_char_range(Some(egui::text::CCursorRange {
                        primary: CCursor::new(end_char),
                        secondary: CCursor::new(start_char),
                        h_pos: None,
                    }));
                    state.store(ctx, editor_id);
                }
                self.current_tab_mut().cursor_char_index = end_char;
                self.current_tab_mut().selection_char_range = Some((start_char, end_char));
                self.status_message = format!("Found at byte {}", start + 1);
            }
            Ok(None) => {
                self.status_message = "No match".to_string();
            }
            Err(e) => {
                self.status_message = e;
            }
        }
    }

    pub fn run_replace_one(&mut self, ctx: &egui::Context) {
        if self.find.last_match.is_none() {
            self.run_find_next(ctx);
        }
        let mut text = self.current_tab().text.clone();
        match self.find.replace_current(&mut text) {
            Ok(true) => {
                self.current_tab_mut().set_text(text);
                self.status_message = "Replaced one match".to_string();
            }
            Ok(false) => self.status_message = "No match to replace".to_string(),
            Err(e) => self.status_message = e,
        }
    }

    pub fn run_replace_all(&mut self) {
        let mut text = self.current_tab().text.clone();
        match self.find.replace_all(&mut text) {
            Ok(count) if count > 0 => {
                self.current_tab_mut().set_text(text);
                self.status_message = format!("Replaced {count} matches");
            }
            Ok(_) => self.status_message = "No matches found".to_string(),
            Err(e) => self.status_message = e,
        }
    }

    pub fn draw_goto_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        if !self.show_goto_panel {
            return;
        }

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Go to line:");
                ui.add(egui::TextEdit::singleline(&mut self.goto_line_input).desired_width(80.0));
                ui.label("Column:");
                ui.add(egui::TextEdit::singleline(&mut self.goto_column_input).desired_width(80.0));

                let submit =
                    ui.button("Go").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                if submit {
                    self.goto_line_column(ctx);
                }
                if ui.button("Close").clicked() {
                    self.show_goto_panel = false;
                }
            });
        });
    }

    pub fn goto_line_column(&mut self, ctx: &egui::Context) {
        let line = self
            .goto_line_input
            .trim()
            .parse::<usize>()
            .unwrap_or(1)
            .max(1);
        let col = self
            .goto_column_input
            .trim()
            .parse::<usize>()
            .unwrap_or(1)
            .max(1);

        let text = self.current_tab().text.clone();
        let target_index = char_index_for_line_col(&text, line, col);
        let editor_id = Id::new(format!("editor-{}", self.current_tab().id));

        if let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) {
            let cursor = CCursor::new(target_index);
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
            state.store(ctx, editor_id);
            self.current_tab_mut().cursor_char_index = target_index;
            self.status_message = format!("Moved to Ln {}, Col {}", line, col);
            self.show_goto_panel = false;
        } else {
            self.status_message =
                "Editor is not focused yet; click editor then try Go To".to_string();
        }
    }

    pub fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        egui::Window::new("About eNote")
            .open(&mut self.show_about)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("eNote");
                ui.label("A Rust/egui-based editor inspired by Notepad++.");
                ui.label("Core editing, tabs, find/replace (regex), bookmarks,");
                ui.label("multi-encoding, column mode, and autocomplete.");
                ui.separator();
                ui.label("Native plugin (DLL) loading is intentionally not supported.");
            });
    }

    pub fn draw_plugins_admin_window(&mut self, ctx: &egui::Context) {
        if !self.show_plugins_admin {
            return;
        }
        egui::Window::new("Plugins Admin")
            .open(&mut self.show_plugins_admin)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Native plugin loading (arbitrary DLLs) is disabled by design:");
                ui.label("it would let any installed plugin execute unrestricted code");
                ui.label("in this process, which is an unacceptable security risk for");
                ui.label("an app that opens untrusted files.");
                ui.label("Built-in equivalents are available from the other menus:");
                ui.label("Search (bookmarks), Encoding, column mode, Utilities.");
            });
    }

    /// Draws the standalone "Base64" tool: an editable input box (prefilled from the
    /// selection/whole document when opened from the editor, but just as usable with
    /// freshly typed/pasted text from the top "Utilities" menu) with Encode/Decode
    /// buttons, a read-only output box with Copy, and an opt-in "Insert into Editor" that
    /// writes the result back to the tab/range captured when the dialog opened. Never
    /// touches the document by itself.
    pub fn draw_base64_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_base64_dialog {
            return;
        }

        let mut window_open = true;
        let mut encode = false;
        let mut decode = false;
        let mut insert = false;
        let mut close = false;

        egui::Window::new("Base64")
            .open(&mut window_open)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label("Input:");
                egui::ScrollArea::vertical()
                    .id_salt("base64_input_scroll")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.base64_input)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                ui.horizontal(|ui| {
                    if ui.button("Encode").clicked() {
                        encode = true;
                    }
                    if ui.button("Decode").clicked() {
                        decode = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });

                if let Some(result) = &self.base64_output {
                    ui.separator();
                    match result {
                        Ok(output) => {
                            ui.horizontal(|ui| {
                                ui.strong("Output");
                                if ui.small_button("Copy").clicked() {
                                    ui.ctx().copy_text(output.clone());
                                }
                            });
                            let mut output_display = output.clone();
                            egui::ScrollArea::vertical()
                                .id_salt("base64_output_scroll")
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut output_display)
                                            .desired_width(f32::INFINITY)
                                            .font(egui::TextStyle::Monospace)
                                            .interactive(false),
                                    );
                                });
                            if ui.button("Insert into Editor").clicked() {
                                insert = true;
                            }
                        }
                        Err(message) => {
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                        }
                    }
                }
            });

        if encode {
            self.base64_output = Some(Ok(
                base64::engine::general_purpose::STANDARD.encode(self.base64_input.as_bytes())
            ));
            self.status_message = "Encoded to Base64".to_string();
        } else if decode {
            self.base64_output = Some(
                base64::engine::general_purpose::STANDARD
                    .decode(self.base64_input.trim())
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .map_err(|e| format!("Not valid Base64: {e}")),
            );
            self.status_message = "Decoded from Base64".to_string();
        }

        if insert {
            if let Some(Ok(output)) = self.base64_output.clone() {
                let (start, end) = self.base64_target_range;
                let tab_idx = self.base64_target_tab;
                let editor_id = Id::new(format!("editor-{}", self.tabs[tab_idx].id));
                let mut text = self.tabs[tab_idx].text.clone();
                let end_char = replace_char_range(&mut text, start..end, &output);
                self.tabs[tab_idx].set_text(text);
                self.tabs[tab_idx].cursor_char_index = end_char;
                self.tabs[tab_idx].recompute_cursor_cache();
                self.tabs[tab_idx].selection_char_range = Some((start, end_char));
                set_editor_selection(ctx, editor_id, start, end_char);
                self.status_message = "Inserted Base64 result into editor".to_string();
            }
        }

        if close || !window_open {
            self.show_base64_dialog = false;
            self.base64_output = None;
        }
    }

    /// Hashes `self.hash_input` (prefilled from the selection/whole document, but freely
    /// editable/pasteable) with an optional HMAC key. Unlike the old flow, Compute no
    /// longer touches the document by itself - the result is shown with Copy and an
    /// opt-in "Insert into Editor" button that writes it back to the tab/range captured
    /// when the dialog opened.
    pub fn draw_hash_key_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_hash_dialog {
            return;
        }
        let Some(algo) = self.hash_dialog_algo else {
            self.show_hash_dialog = false;
            return;
        };

        let mut window_open = true;
        let mut compute = false;
        let mut insert = false;
        let mut close = false;

        egui::Window::new(format!("Hash with {}", algo.label()))
            .open(&mut window_open)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.label("Text to hash:");
                egui::ScrollArea::vertical()
                    .id_salt("hash_input_scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.hash_input)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                ui.add_space(4.0);
                ui.label("Key (optional - leave blank for a plain hash, or enter a key to compute HMAC):");
                let response = ui.add(egui::TextEdit::singleline(&mut self.hash_key_input).desired_width(f32::INFINITY));
                let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                ui.horizontal(|ui| {
                    if ui.button("Compute").clicked() || enter_pressed {
                        compute = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });

                if let Some(digest) = &self.hash_output {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.strong("Result");
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(digest.clone());
                        }
                    });
                    let mut output_display = digest.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut output_display)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace)
                            .interactive(false),
                    );
                    if ui.button("Insert into Editor").clicked() {
                        insert = true;
                    }
                }
            });

        if compute {
            let keyed = !self.hash_key_input.is_empty();
            self.hash_output = Some(compute_hash(algo, &self.hash_input, &self.hash_key_input));
            self.status_message = if keyed {
                format!("Computed HMAC-{}", algo.label())
            } else {
                format!("Computed {}", algo.label())
            };
        }

        if insert {
            if let Some(digest) = self.hash_output.clone() {
                let (start, end) = self.hash_dialog_range;
                let tab_idx = self.hash_dialog_tab;
                let editor_id = Id::new(format!("editor-{}", self.tabs[tab_idx].id));
                let mut text = self.tabs[tab_idx].text.clone();
                let end_char = replace_char_range(&mut text, start..end, &digest);
                self.tabs[tab_idx].set_text(text);
                self.tabs[tab_idx].cursor_char_index = end_char;
                self.tabs[tab_idx].recompute_cursor_cache();
                self.tabs[tab_idx].selection_char_range = Some((start, end_char));
                set_editor_selection(ctx, editor_id, start, end_char);
                self.status_message = "Inserted hash into editor".to_string();
            }
        }

        if close || !window_open {
            self.show_hash_dialog = false;
            self.hash_dialog_algo = None;
            self.hash_key_input.clear();
            self.hash_output = None;
        }
    }

    /// Hashes `self.bcrypt_hash_input` (prefilled from the selection/whole document, but
    /// freely editable) at the given cost factor. Each confirm generates a freshly-salted
    /// hash, so re-running it on the same text will keep yielding a different (still
    /// valid) result. Compute no longer touches the document - "Insert into Editor" writes
    /// the result back to the tab/range captured when the dialog opened.
    pub fn draw_bcrypt_hash_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_bcrypt_hash_dialog {
            return;
        }

        let mut window_open = true;
        let mut compute = false;
        let mut insert = false;
        let mut close = false;

        egui::Window::new("Hash with Bcrypt")
            .open(&mut window_open)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.label("Text to hash:");
                egui::ScrollArea::vertical()
                    .id_salt("bcrypt_hash_input_scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.bcrypt_hash_input)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                ui.add_space(4.0);
                ui.label("Cost factor (4-31, higher is slower/stronger):");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.bcrypt_cost_input).desired_width(60.0),
                );
                let enter_pressed =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(error) = &self.bcrypt_hash_error {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
                }
                ui.horizontal(|ui| {
                    if ui.button("Compute").clicked() || enter_pressed {
                        compute = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });

                if let Some(hash) = &self.bcrypt_hash_output {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.strong("Result");
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(hash.clone());
                        }
                    });
                    let mut output_display = hash.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut output_display)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace)
                            .interactive(false),
                    );
                    if ui.button("Insert into Editor").clicked() {
                        insert = true;
                    }
                }
            });

        if compute {
            match self.bcrypt_cost_input.trim().parse::<u32>() {
                Ok(cost) if (4..=31).contains(&cost) => {
                    match crate::hashing::bcrypt_hash(&self.bcrypt_hash_input, cost) {
                        Ok(hash) => {
                            self.bcrypt_hash_output = Some(hash);
                            self.bcrypt_hash_error = None;
                            self.status_message = format!("Computed bcrypt hash (cost {cost})");
                        }
                        Err(message) => self.bcrypt_hash_error = Some(message),
                    }
                }
                _ => {
                    self.bcrypt_hash_error =
                        Some("Cost must be a whole number between 4 and 31".to_string())
                }
            }
        }

        if insert {
            if let Some(hash) = self.bcrypt_hash_output.clone() {
                let (start, end) = self.bcrypt_hash_range;
                let tab_idx = self.bcrypt_hash_tab;
                let editor_id = Id::new(format!("editor-{}", self.tabs[tab_idx].id));
                let mut text = self.tabs[tab_idx].text.clone();
                let end_char = replace_char_range(&mut text, start..end, &hash);
                self.tabs[tab_idx].set_text(text);
                self.tabs[tab_idx].cursor_char_index = end_char;
                self.tabs[tab_idx].recompute_cursor_cache();
                self.tabs[tab_idx].selection_char_range = Some((start, end_char));
                set_editor_selection(ctx, editor_id, start, end_char);
                self.status_message = "Inserted bcrypt hash into editor".to_string();
            }
        }

        if close || !window_open {
            self.show_bcrypt_hash_dialog = false;
            self.bcrypt_hash_error = None;
            self.bcrypt_hash_output = None;
        }
    }

    /// Lets the user type a candidate password to check against a bcrypt hash - prefilled
    /// from the selection/whole document when "Bcrypt > Verify..." is clicked, but freely
    /// editable/pasteable so a hash from elsewhere can be checked too. This never edits
    /// the document - it just reports a match/no-match (or a parse error if the hash box
    /// isn't a valid hash).
    pub fn draw_bcrypt_verify_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_bcrypt_verify_dialog {
            return;
        }

        let mut window_open = true;
        let mut verify = false;
        let mut close = false;

        egui::Window::new("Verify Bcrypt")
            .open(&mut window_open)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Hash to check against:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.bcrypt_verify_source)
                        .desired_width(300.0)
                        .font(egui::TextStyle::Monospace),
                );
                ui.label("Candidate password:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.bcrypt_verify_candidate)
                        .password(true)
                        .desired_width(300.0),
                );
                let enter_pressed =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                match &self.bcrypt_verify_result {
                    Some(Ok(true)) => {
                        ui.colored_label(
                            egui::Color32::from_rgb(90, 190, 90),
                            "✓ Password matches the hash",
                        );
                    }
                    Some(Ok(false)) => {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 80, 80),
                            "✗ Password does NOT match the hash",
                        );
                    }
                    Some(Err(message)) => {
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                    }
                    None => {}
                }

                ui.horizontal(|ui| {
                    if ui.button("Verify").clicked() || enter_pressed {
                        verify = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });

        if verify {
            let result = crate::hashing::bcrypt_verify(
                &self.bcrypt_verify_candidate,
                &self.bcrypt_verify_source,
            );
            self.status_message = match &result {
                Ok(true) => "Bcrypt verify: password matches".to_string(),
                Ok(false) => "Bcrypt verify: password does NOT match".to_string(),
                Err(message) => message.clone(),
            };
            self.bcrypt_verify_result = Some(result);
        } else if close || !window_open {
            self.show_bcrypt_verify_dialog = false;
            self.bcrypt_verify_result = None;
            self.bcrypt_verify_candidate.clear();
        }
    }

    /// Shows the header/payload/signature captured when "Decode JWT..." was clicked -
    /// modeled on 8gwifi.org's JWS Parser & Decoder, with copyable Header/Payload/
    /// Signature boxes and a JWT Registered Claims table - plus a quick inline HMAC check
    /// and a bridge to the full multi-algorithm "Verify JWS..." tool. Never edits the
    /// document.
    pub fn draw_jwt_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_jwt_dialog {
            return;
        }

        // Copy out what the window needs up front (rather than holding a borrow of
        // `self.jwt_decoded`), since the closure below also needs to mutate
        // `self.jwt_secret_input`/`self.jwt_verify_result`.
        let view = match &self.jwt_decoded {
            Some(Ok(decoded)) => Ok((
                decoded.algorithm.clone(),
                decoded.header_json.clone(),
                decoded.payload_json.clone(),
                decoded.signature_b64.clone(),
                decoded.claims.clone(),
                decoded.claim_notes.clone(),
            )),
            Some(Err(message)) => Err(message.clone()),
            None => {
                self.show_jwt_dialog = false;
                return;
            }
        };

        let mut window_open = true;
        let mut verify = false;
        let mut open_full_verify = false;
        let mut decode = false;
        let mut close = false;

        egui::Window::new("JWS Parser & Decoder")
            .open(&mut window_open)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("JWT/JWS token (paste one and click Decode):");
                let token_response = egui::ScrollArea::vertical()
                    .id_salt("jwt_token_scroll")
                    .max_height(70.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.jwt_source)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        )
                    })
                    .inner;
                let enter_pressed_token =
                    token_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Decode").clicked() || enter_pressed_token {
                    decode = true;
                }

                ui.separator();

                match &view {
                    Ok((
                        algorithm,
                        header_json,
                        payload_json,
                        signature_b64,
                        claims,
                        claim_notes,
                    )) => {
                        ui.label(format!(
                            "Algorithm: {}",
                            algorithm.as_deref().unwrap_or("(unknown)")
                        ));

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.strong("Header");
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(header_json.clone());
                            }
                        });
                        let mut header_display = header_json.clone();
                        egui::ScrollArea::vertical()
                            .id_salt("jwt_header_scroll")
                            .max_height(90.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut header_display)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace)
                                        .interactive(false),
                                );
                            });

                        ui.horizontal(|ui| {
                            ui.strong("Payload");
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(payload_json.clone());
                            }
                        });
                        let mut payload_display = payload_json.clone();
                        egui::ScrollArea::vertical()
                            .id_salt("jwt_payload_scroll")
                            .max_height(180.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut payload_display)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace)
                                        .interactive(false),
                                );
                            });

                        if !claims.is_empty() {
                            ui.separator();
                            ui.strong("JWT Registered Claims");
                            egui::Grid::new("jwt_claims_grid")
                                .num_columns(2)
                                .striped(true)
                                .show(ui, |ui| {
                                    for (label, value) in claims {
                                        ui.label(*label);
                                        ui.label(value);
                                        ui.end_row();
                                    }
                                });
                        }

                        for note in claim_notes {
                            let expired = note.contains("EXPIRED");
                            let color = if expired {
                                egui::Color32::from_rgb(220, 80, 80)
                            } else {
                                egui::Color32::from_rgb(160, 160, 160)
                            };
                            ui.colored_label(color, note);
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.strong("Signature (Base64URL)");
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(signature_b64.clone());
                            }
                        });
                        let mut signature_display = signature_b64.clone();
                        ui.add(
                            egui::TextEdit::multiline(&mut signature_display)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false),
                        );

                        ui.separator();
                        ui.label("Quick verify (HMAC secret, HS256/384/512 only):");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.jwt_secret_input)
                                .password(true)
                                .desired_width(f32::INFINITY),
                        );
                        let enter_pressed =
                            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        match &self.jwt_verify_result {
                            Some(Ok(true)) => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(90, 190, 90),
                                    "✓ Signature matches",
                                );
                            }
                            Some(Ok(false)) => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 80, 80),
                                    "✗ Signature does NOT match",
                                );
                            }
                            Some(Err(message)) => {
                                ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                            }
                            None => {}
                        }

                        ui.horizontal(|ui| {
                            if ui.button("Verify").clicked() || enter_pressed {
                                verify = true;
                            }
                            if ui
                            .button("Verify Signature...")
                            .on_hover_text(
                                "Open the full verifier (HMAC, RSA, RSA-PSS, or ECDSA public key)",
                            )
                            .clicked()
                        {
                            open_full_verify = true;
                        }
                            if ui.button("Close").clicked() {
                                close = true;
                            }
                        });
                    }
                    Err(message) => {
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    }
                }
            });

        if decode {
            let source = self.jwt_source.trim().to_string();
            self.jwt_decoded = Some(crate::jwt::decode_jwt(&source));
            self.jwt_source = source;
            self.jwt_secret_input.clear();
            self.jwt_verify_result = None;
        }

        if verify {
            let result = crate::jwt::verify_jwt_hmac(&self.jwt_source, &self.jwt_secret_input);
            self.status_message = match &result {
                Ok(true) => "JWT verify: signature matches".to_string(),
                Ok(false) => "JWT verify: signature does NOT match".to_string(),
                Err(message) => message.clone(),
            };
            self.jwt_verify_result = Some(result);
        } else if open_full_verify {
            self.jws_verify_token = self.jwt_source.clone();
            self.jws_verify_key_input.clear();
            self.jws_verify_detached_payload.clear();
            self.jws_verify_result = None;
            self.show_jws_verify_dialog = true;
        } else if close || !window_open {
            self.show_jwt_dialog = false;
            self.jwt_decoded = None;
            self.jwt_secret_input.clear();
            self.jwt_verify_result = None;
        }
    }

    /// Draws the "JWS Generator..." tool window (modeled on 8gwifi.org's JWS generator):
    /// an algorithm picker grouped by family (HMAC/RSA PKCS#1/RSA PSS/ECDSA), a key box,
    /// an editable payload box, and a Generate button whose output is shown read-only
    /// with a copy button. Never touches the document.
    pub fn draw_jws_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_jws_dialog {
            return;
        }

        use crate::jws::{JwsAlgorithm, JwsAlgorithmFamily, generate_jws};

        let mut window_open = true;
        let mut generate = false;
        let mut close = false;

        egui::Window::new("JWS Generator")
            .open(&mut window_open)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.label("Algorithm:");
                egui::ComboBox::from_id_salt("jws_algorithm")
                    .width(280.0)
                    .selected_text(format!(
                        "{} - {}",
                        self.jws_algorithm.label(),
                        self.jws_algorithm.description()
                    ))
                    .show_ui(ui, |ui| {
                        for family in [
                            JwsAlgorithmFamily::Hmac,
                            JwsAlgorithmFamily::RsaPkcs1,
                            JwsAlgorithmFamily::RsaPss,
                            JwsAlgorithmFamily::Ecdsa,
                        ] {
                            ui.label(egui::RichText::new(family.label()).strong());
                            ui.label(egui::RichText::new(family.subtitle()).weak().small());
                            for algo in JwsAlgorithm::ALL
                                .into_iter()
                                .filter(|a| a.family() == family)
                            {
                                let mut text = format!("{} - {}", algo.label(), algo.description());
                                if !algo.badge().is_empty() {
                                    text.push_str(&format!(" ({})", algo.badge()));
                                }
                                ui.selectable_value(&mut self.jws_algorithm, algo, text);
                            }
                            ui.separator();
                        }
                    });

                ui.add_space(6.0);
                ui.label(self.jws_algorithm.family().key_hint());
                ui.add(
                    egui::TextEdit::multiline(&mut self.jws_key_input)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );

                ui.add_space(6.0);
                ui.label("Payload (raw text - typically a JSON claims object):");
                egui::ScrollArea::vertical()
                    .id_salt("jws_payload_scroll")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.jws_payload_input)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                ui.add_space(6.0);
                ui.checkbox(&mut self.jws_detached, "Detached payload")
                    .on_hover_text(
                        "Omit the payload from the output (header.signature instead of \
                         header.payload.signature); keep the payload yourself and supply \
                         it separately when verifying",
                    );

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Generate").clicked() {
                        generate = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });

                if let Some(result) = &self.jws_result {
                    ui.separator();
                    match result {
                        Ok(jws) => {
                            ui.label("Generated JWS:");
                            let mut output = jws.clone();
                            egui::ScrollArea::vertical()
                                .id_salt("jws_output_scroll")
                                .max_height(100.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut output)
                                            .desired_width(f32::INFINITY)
                                            .font(egui::TextStyle::Monospace)
                                            .interactive(false),
                                    );
                                });
                            if ui.button("Copy to Clipboard").clicked() {
                                ui.ctx().copy_text(jws.clone());
                                self.status_message = "Copied JWS to clipboard".to_string();
                            }
                        }
                        Err(message) => {
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                        }
                    }
                }
            });

        if generate {
            let result = if self.jws_detached {
                crate::jws::generate_jws_detached(
                    self.jws_algorithm,
                    &self.jws_key_input,
                    &self.jws_payload_input,
                )
            } else {
                generate_jws(
                    self.jws_algorithm,
                    &self.jws_key_input,
                    &self.jws_payload_input,
                )
            };
            self.status_message = match &result {
                Ok(_) if self.jws_detached => {
                    format!("Generated detached {} JWS", self.jws_algorithm.label())
                }
                Ok(_) => format!("Generated {} JWS", self.jws_algorithm.label()),
                Err(message) => message.clone(),
            };
            self.jws_result = Some(result);
        } else if close || !window_open {
            self.show_jws_dialog = false;
            self.jws_result = None;
        }
    }

    /// Draws the "Verify JWS..." tool window: checks a captured JWS's signature against a
    /// key, using the algorithm named in the token's own header (not user-chosen, unlike
    /// the generator). Shows the detected algorithm as soon as the token box parses, and
    /// only asks for the original payload text if the token turns out to have a detached
    /// payload (a two-segment `header.signature` token).
    pub fn draw_jws_verify_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_jws_verify_dialog {
            return;
        }

        use crate::jws::{inspect_jws, verify_jws};

        let preview = inspect_jws(&self.jws_verify_token);

        let mut window_open = true;
        let mut verify = false;
        let mut close = false;

        egui::Window::new("Verify JWS")
            .open(&mut window_open)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("JWS token (header.payload.signature, or header.signature if detached):");
                egui::ScrollArea::vertical()
                    .id_salt("jws_verify_token_scroll")
                    .max_height(100.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.jws_verify_token)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                match &preview {
                    Ok(info) => {
                        ui.label(format!("Detected algorithm: {}", info.alg_label));
                        if info.is_detached {
                            ui.colored_label(
                                egui::Color32::from_rgb(230, 180, 60),
                                "Detached payload - paste the original payload to verify it:",
                            );
                            ui.add(
                                egui::TextEdit::multiline(&mut self.jws_verify_detached_payload)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::TextStyle::Monospace),
                            );
                        }
                    }
                    Err(message) => {
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                    }
                }

                ui.add_space(6.0);
                let key_hint = match &preview {
                    Ok(info) => info
                        .algorithm
                        .map(|a| a.family().verify_key_hint())
                        .unwrap_or("Key (secret or public key, depending on algorithm)"),
                    Err(_) => "Key (secret or public key, depending on algorithm)",
                };
                ui.label(key_hint);
                ui.add(
                    egui::TextEdit::multiline(&mut self.jws_verify_key_input)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );

                if let Some(result) = &self.jws_verify_result {
                    ui.separator();
                    match result {
                        Ok(true) => {
                            ui.colored_label(
                                egui::Color32::from_rgb(90, 190, 90),
                                "✓ Signature is valid",
                            );
                        }
                        Ok(false) => {
                            ui.colored_label(
                                egui::Color32::from_rgb(220, 80, 80),
                                "✗ Signature is NOT valid",
                            );
                        }
                        Err(message) => {
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                        }
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Verify").clicked() {
                        verify = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });

        if verify {
            let detached_payload = if self.jws_verify_detached_payload.is_empty() {
                None
            } else {
                Some(self.jws_verify_detached_payload.as_str())
            };
            let result = verify_jws(
                &self.jws_verify_token,
                &self.jws_verify_key_input,
                detached_payload,
            );
            self.status_message = match &result {
                Ok(true) => "JWS verify: signature is valid".to_string(),
                Ok(false) => "JWS verify: signature is NOT valid".to_string(),
                Err(message) => message.clone(),
            };
            self.jws_verify_result = Some(result);
        } else if close || !window_open {
            self.show_jws_verify_dialog = false;
            self.jws_verify_result = None;
        }
    }

    /// Draws the "Convert..." Utilities tool window: Unix timestamp <-> date/time <->
    /// custom formatted string. `convert_input` is prefilled from the selection/whole
    /// document (if it looks like a plain number or date) but is freely editable.
    pub fn draw_convert_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_convert_dialog {
            return;
        }

        use crate::app::ConvertMode;
        use crate::datetime_convert::{TimeZoneChoice, TimestampUnit, now_timestamp};

        let mut window_open = true;
        let mut convert = false;
        let mut insert = false;
        let mut close = false;

        egui::Window::new("Convert")
            .open(&mut window_open)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.label("Conversion:");
                egui::ComboBox::from_id_salt("convert_mode")
                    .width(280.0)
                    .selected_text(self.convert_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in ConvertMode::ALL {
                            ui.selectable_value(&mut self.convert_mode, mode, mode.label());
                        }
                    });

                ui.add_space(6.0);

                match self.convert_mode {
                    ConvertMode::TimestampToDateTime => {
                        ui.horizontal(|ui| {
                            ui.label("Timestamp:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.convert_input)
                                    .desired_width(220.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                            if ui.button("Now").clicked() {
                                self.convert_input =
                                    now_timestamp(self.convert_timestamp_unit).to_string();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Unit:");
                            egui::ComboBox::from_id_salt("convert_unit")
                                .selected_text(self.convert_timestamp_unit.label())
                                .show_ui(ui, |ui| {
                                    for unit in TimestampUnit::ALL {
                                        ui.selectable_value(
                                            &mut self.convert_timestamp_unit,
                                            unit,
                                            unit.label(),
                                        );
                                    }
                                });
                            ui.label("Timezone:");
                            egui::ComboBox::from_id_salt("convert_tz")
                                .selected_text(self.convert_timezone.label())
                                .show_ui(ui, |ui| {
                                    for tz in TimeZoneChoice::ALL {
                                        ui.selectable_value(
                                            &mut self.convert_timezone,
                                            tz,
                                            tz.label(),
                                        );
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Output format:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.convert_output_format)
                                    .desired_width(200.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });
                    }
                    ConvertMode::DateTimeToTimestamp => {
                        ui.label("Date/Time:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.convert_input)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Input format:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.convert_input_format)
                                    .desired_width(200.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Timezone:");
                            egui::ComboBox::from_id_salt("convert_tz")
                                .selected_text(self.convert_timezone.label())
                                .show_ui(ui, |ui| {
                                    for tz in TimeZoneChoice::ALL {
                                        ui.selectable_value(
                                            &mut self.convert_timezone,
                                            tz,
                                            tz.label(),
                                        );
                                    }
                                });
                            ui.label("Output unit:");
                            egui::ComboBox::from_id_salt("convert_unit")
                                .selected_text(self.convert_timestamp_unit.label())
                                .show_ui(ui, |ui| {
                                    for unit in TimestampUnit::ALL {
                                        ui.selectable_value(
                                            &mut self.convert_timestamp_unit,
                                            unit,
                                            unit.label(),
                                        );
                                    }
                                });
                        });
                    }
                    ConvertMode::ReformatDateTime => {
                        ui.label("Date/Time:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.convert_input)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Input format:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.convert_input_format)
                                    .desired_width(200.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Output format:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.convert_output_format)
                                    .desired_width(200.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });
                    }
                }

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Format examples: %Y-%m-%d %H:%M:%S, %d/%m/%Y, %Y-%m-%dT%H:%M:%S%.3f",
                    )
                    .weak()
                    .small(),
                );

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Convert").clicked() {
                        convert = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });

                if let Some(result) = &self.convert_output {
                    ui.separator();
                    match result {
                        Ok(output) => {
                            ui.horizontal(|ui| {
                                ui.strong("Result");
                                if ui.small_button("Copy").clicked() {
                                    ui.ctx().copy_text(output.clone());
                                }
                            });
                            let mut output_display = output.clone();
                            ui.add(
                                egui::TextEdit::singleline(&mut output_display)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::TextStyle::Monospace)
                                    .interactive(false),
                            );
                            if ui.button("Insert into Editor").clicked() {
                                insert = true;
                            }
                        }
                        Err(message) => {
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), message);
                        }
                    }
                }
            });

        if convert {
            use crate::datetime_convert::{
                reformat_datetime, string_to_timestamp, timestamp_to_string,
            };
            let result = match self.convert_mode {
                ConvertMode::TimestampToDateTime => self
                    .convert_input
                    .trim()
                    .parse::<i64>()
                    .map_err(|e| format!("Not a valid integer timestamp: {e}"))
                    .and_then(|value| {
                        timestamp_to_string(
                            value,
                            self.convert_timestamp_unit,
                            self.convert_timezone,
                            &self.convert_output_format,
                        )
                    }),
                ConvertMode::DateTimeToTimestamp => string_to_timestamp(
                    &self.convert_input,
                    &self.convert_input_format,
                    self.convert_timezone,
                    self.convert_timestamp_unit,
                )
                .map(|ts| ts.to_string()),
                ConvertMode::ReformatDateTime => reformat_datetime(
                    &self.convert_input,
                    &self.convert_input_format,
                    &self.convert_output_format,
                ),
            };
            self.status_message = match &result {
                Ok(_) => "Converted successfully".to_string(),
                Err(message) => message.clone(),
            };
            self.convert_output = Some(result);
        }

        if insert {
            if let Some(Ok(output)) = self.convert_output.clone() {
                let (start, end) = self.convert_target_range;
                let tab_idx = self.convert_target_tab;
                let editor_id = Id::new(format!("editor-{}", self.tabs[tab_idx].id));
                let mut text = self.tabs[tab_idx].text.clone();
                let end_char = replace_char_range(&mut text, start..end, &output);
                self.tabs[tab_idx].set_text(text);
                self.tabs[tab_idx].cursor_char_index = end_char;
                self.tabs[tab_idx].recompute_cursor_cache();
                self.tabs[tab_idx].selection_char_range = Some((start, end_char));
                set_editor_selection(ctx, editor_id, start, end_char);
                self.status_message = "Inserted conversion result into editor".to_string();
            }
        }

        if close || !window_open {
            self.show_convert_dialog = false;
            self.convert_output = None;
        }
    }
}
