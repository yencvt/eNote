use eframe::egui;
use egui_extras::syntax_highlighting::CodeTheme;

use crate::app::NotePageApp;
use crate::encoding::TextEncoding;
use crate::language::Language;

impl NotePageApp {
    pub fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New\tCtrl+N").clicked() {
                        self.add_new_tab();
                        ui.close();
                    }
                    if ui.button("Open...\tCtrl+O").clicked() {
                        self.open_file();
                        ui.close();
                    }
                    ui.menu_button("Open Recent", |ui| {
                        if self.recent_files.is_empty() {
                            ui.label("No recent files");
                        }
                        let recent = self.recent_files.clone();
                        for path in recent {
                            if ui.button(path.display().to_string()).clicked() {
                                self.open_file_by_path(path);
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .button("View Log (tail -f)...")
                        .on_hover_text(
                            "Open a file and keep watching it for new content as it grows, \
                             like `tail -f`",
                        )
                        .clicked()
                    {
                        self.open_log_file();
                        ui.close();
                    }
                    if ui.button("Save\tCtrl+S").clicked() {
                        self.save_current();
                        ui.close();
                    }
                    if ui.button("Save As...\tCtrl+Shift+S").clicked() {
                        self.save_current_as();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Close\tCtrl+W").clicked() {
                        self.close_active_tab();
                        ui.close();
                    }
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo\tCtrl+Z").clicked() {
                        ui.ctx().memory_mut(|m| m.data.clear());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Cut\tCtrl+X").clicked() {
                        self.editor_context_menu_open = false;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Find...\tCtrl+F").clicked() {
                        self.show_find_panel = true;
                        ui.close();
                    }
                    if ui.button("Replace...\tCtrl+H").clicked() {
                        self.show_find_panel = true;
                        ui.close();
                    }
                    if ui.button("Go to Line...\tCtrl+G").clicked() {
                        self.show_goto_panel = true;
                        ui.close();
                    }
                    ui.separator();
                    let column_label = if self.current_tab().multi_cursor_active {
                        "Exit Column Mode\tEsc"
                    } else {
                        "Column Mode\tAlt+Drag / Alt+Shift+Click"
                    };
                    if ui.button(column_label).clicked() {
                        self.current_tab_mut().exit_column_mode();
                        ui.close();
                    }
                    let multi_caret_label = if self.current_tab().extra_carets.is_empty() {
                        "Multi-Edit Carets\tCtrl+Click"
                    } else {
                        "Exit Multi-Edit Carets\tEsc"
                    };
                    if ui.button(multi_caret_label).clicked() {
                        self.current_tab_mut().exit_multi_caret_mode();
                        ui.close();
                    }
                });

                ui.menu_button("Search", |ui| {
                    if ui.button("Find...\tCtrl+F").clicked() {
                        self.show_find_panel = true;
                        ui.close();
                    }
                    if ui.button("Find Next\tF3").clicked() {
                        self.run_find_next(ctx);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Toggle Bookmark\tCtrl+F2").clicked() {
                        self.toggle_bookmark_on_current_line();
                        ui.close();
                    }
                    if ui.button("Next Bookmark\tF2").clicked() {
                        self.goto_next_bookmark();
                        ui.close();
                    }
                    if ui.button("Previous Bookmark\tShift+F2").clicked() {
                        self.goto_prev_bookmark();
                        ui.close();
                    }
                    if ui.button("Clear All Bookmarks").clicked() {
                        self.clear_all_bookmarks();
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    let wrap_label = if self.word_wrap {
                        "Disable Word Wrap"
                    } else {
                        "Word Wrap"
                    };
                    if ui.button(wrap_label).clicked() {
                        self.word_wrap = !self.word_wrap;
                        ui.close();
                    }
                    if ui.checkbox(&mut self.show_toolbar, "Toolbar").changed() {
                        ui.close();
                    }
                    if ui
                        .checkbox(&mut self.show_status_bar, "Status Bar")
                        .changed()
                    {
                        ui.close();
                    }
                    ui.separator();
                    let theme_label = if self.dark_mode {
                        "Light Theme"
                    } else {
                        "Dark Theme"
                    };
                    if ui.button(theme_label).clicked() {
                        self.set_dark_mode(ctx, !self.dark_mode);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Zoom In\tCtrl++").clicked() {
                        self.zoom_in(ctx);
                        ui.close();
                    }
                    if ui.button("Zoom Out\tCtrl+-").clicked() {
                        self.zoom_out(ctx);
                        ui.close();
                    }
                    if ui.button("Restore Default Zoom\tCtrl+0").clicked() {
                        self.zoom_reset(ctx);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button("Fold All (collapse code blocks on current indent)")
                        .clicked()
                    {
                        self.fold_all_current_tab();
                        ui.close();
                    }
                    if ui.button("Unfold All").clicked() {
                        self.current_tab_mut().folds.clear();
                        ui.close();
                    }
                });

                ui.menu_button("Format", |ui| {
                    if ui.button("Format Document\tCtrl+Alt+L").clicked() {
                        self.request_format();
                        ui.close();
                    }
                    ui.separator();
                    ui.label("Beautifies JSON/XML/HTML/CSS; reindents brace-based code.");
                });

                ui.menu_button("Utilities", |ui| {
                    ui.menu_button("Base64", |ui| {
                        if ui.button("Encode...").clicked() {
                            if !self.utilities_unavailable_in_split_view("Base64") {
                                self.pending_base64_encode = true;
                            }
                            ui.close();
                        }
                        if ui.button("Decode...").clicked() {
                            if !self.utilities_unavailable_in_split_view("Base64") {
                                self.pending_base64_decode = true;
                            }
                            ui.close();
                        }
                    });

                    ui.menu_button("Hash", |ui| {
                        for algo in [
                            crate::hashing::HashAlgorithm::Md5,
                            crate::hashing::HashAlgorithm::Sha1,
                            crate::hashing::HashAlgorithm::Sha256,
                            crate::hashing::HashAlgorithm::Sha384,
                            crate::hashing::HashAlgorithm::Sha512,
                        ] {
                            if ui.button(format!("{}...", algo.label())).clicked() {
                                if !self.utilities_unavailable_in_split_view(algo.label()) {
                                    self.pending_hash = Some(algo);
                                }
                                ui.close();
                            }
                        }
                    });

                    ui.menu_button("Bcrypt", |ui| {
                        if ui.button("Hash...").clicked() {
                            if !self.utilities_unavailable_in_split_view("Bcrypt") {
                                self.pending_bcrypt_hash = true;
                            }
                            ui.close();
                        }
                        if ui.button("Verify...").clicked() {
                            if !self.utilities_unavailable_in_split_view("Bcrypt") {
                                self.pending_bcrypt_verify = true;
                            }
                            ui.close();
                        }
                    });

                    ui.separator();
                    if ui
                        .button("Decode JWT / JWS...")
                        .on_hover_text("Parse a JWT/JWS into its header, payload, registered claims, and signature")
                        .clicked()
                    {
                        if !self.utilities_unavailable_in_split_view("Decode JWT / JWS") {
                            self.pending_jwt_decode = true;
                        }
                        ui.close();
                    }
                    if ui
                        .button("JWS Generator...")
                        .on_hover_text("Sign a payload into a compact JWS/JWT using HMAC, RSA, RSA-PSS, or ECDSA")
                        .clicked()
                    {
                        if !self.utilities_unavailable_in_split_view("JWS Generator") {
                            self.pending_jws_generator = true;
                        }
                        ui.close();
                    }
                    if ui
                        .button("Verify JWS...")
                        .on_hover_text("Check a JWS/JWT's signature against a secret or public key")
                        .clicked()
                    {
                        if !self.utilities_unavailable_in_split_view("Verify JWS") {
                            self.pending_jws_verify = true;
                        }
                        ui.close();
                    }
                    if ui
                        .button("Convert...")
                        .on_hover_text("Convert between Unix timestamps, date/time values, and custom formatted strings")
                        .clicked()
                    {
                        if !self.utilities_unavailable_in_split_view("Convert") {
                            self.pending_convert = true;
                        }
                        ui.close();
                    }
                });

                ui.menu_button("Compare", |ui| {
                    if ui.button("Compare Tabs...").clicked() {
                        self.open_compare_picker();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Split 2 Panes").clicked() {
                        self.split_view(2);
                        ui.close();
                    }
                    if ui.button("Split 3 Panes").clicked() {
                        self.split_view(3);
                        ui.close();
                    }
                    if ui.button("Split 4 Panes").clicked() {
                        self.split_view(4);
                        ui.close();
                    }
                    if ui
                        .add_enabled(!self.split_panes.is_empty(), egui::Button::new("Unsplit"))
                        .clicked()
                    {
                        self.unsplit_view();
                        ui.close();
                    }
                });

                ui.menu_button("Encoding", |ui| {
                    for encoding in TextEncoding::ALL {
                        let selected = self.current_tab().encoding == encoding;
                        if ui.selectable_label(selected, encoding.label()).clicked() {
                            self.set_encoding(encoding);
                            ui.close();
                        }
                    }
                });

                ui.menu_button("Language", |ui| {
                    for lang in Language::ALL {
                        let selected = self.current_tab().language == lang;
                        if ui.selectable_label(selected, lang.label()).clicked() {
                            self.current_tab_mut().language = lang;
                            ui.close();
                        }
                    }
                });

                ui.menu_button("Settings", |ui| {
                    ui.label("Preferences");
                    ui.add(egui::Slider::new(&mut self.font_size, 8.0..=36.0).text("Font size"));
                    if ui.input(|i| i.pointer.any_released()) {
                        let dark = self.dark_mode;
                        self.code_theme = if dark {
                            CodeTheme::dark(self.font_size)
                        } else {
                            CodeTheme::light(self.font_size)
                        };
                    }

                    #[cfg(target_os = "windows")]
                    {
                        ui.separator();
                        let mut registered = crate::shell_integration::is_registered();
                        if ui
                            .checkbox(
                                &mut registered,
                                "Add \"Open with eNote\" to right-click menu",
                            )
                            .on_hover_text(
                                "Registers (or removes) a per-user Windows Explorer context \
                                 menu entry - like Notepad++'s installer option - to open any \
                                 file with this app",
                            )
                            .changed()
                        {
                            let result = if registered {
                                crate::shell_integration::register()
                            } else {
                                crate::shell_integration::unregister()
                            };
                            self.status_message = match result {
                                Ok(()) if registered => {
                                    "Added \"Open with eNote\" to the right-click menu"
                                        .to_string()
                                }
                                Ok(()) => "Removed \"Open with eNote\" from the right-click menu"
                                    .to_string(),
                                Err(message) => message,
                            };
                        }
                    }
                });

                ui.menu_button("Plugins", |ui| {
                    if ui.button("Plugins Admin...").clicked() {
                        self.show_plugins_admin = true;
                        ui.close();
                    }
                });

                ui.menu_button("?", |ui| {
                    if ui.button("About eNote").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });
            });
        });
    }
}
