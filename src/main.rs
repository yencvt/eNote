// Without this, Windows runs the app with the "console" subsystem, which pops up a
// background cmd.exe-style console window alongside the real (egui) GUI window every
// time it launches - including when opened via the "Open with eNote" Explorer entry.
// Harmless on non-Windows targets, where this attribute is simply ignored.
#![windows_subsystem = "windows"]

mod app;
mod autocomplete;
mod datetime_convert;
mod diff;
mod document;
mod encoding;
mod find_replace;
mod formatter;
mod hashing;
mod jws;
mod jwt;
mod language;
mod session;
mod shell_integration;
mod text_utils;
mod ui;

use std::path::PathBuf;

use eframe::egui;

use app::NotePageApp;

fn main() -> eframe::Result<()> {
    // Support being launched with a file path argument - e.g. from the "Open with eNote"
    // Explorer right-click entry (see `shell_integration`), or just `notepagepp file.txt`
    // from a shell. Ignores anything that looks like a flag (starts with `-`).
    let file_to_open = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));

    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1200.0, 760.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "eNote",
        options,
        Box::new(move |cc| {
            let mut app = NotePageApp::new(cc);
            if let Some(path) = file_to_open {
                app.open_file_from_cli(PathBuf::from(path));
            }
            Ok(Box::new(app))
        }),
    )
}

impl eframe::App for NotePageApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_shortcuts(ctx);
        self.draw_menu_bar(ctx);
        self.draw_toolbar(ctx);
        self.draw_status_bar(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.split_panes.len() >= 2 {
                self.draw_split_view(ui);
            } else {
                self.draw_tabs(ui);
                self.draw_find_panel(ctx, ui);
                self.draw_goto_panel(ctx, ui);
                self.draw_editor(ui);
            }
        });

        self.draw_compare_picker(ctx);
        self.draw_autocomplete_popup(ctx);
        self.draw_about_window(ctx);
        self.draw_plugins_admin_window(ctx);
        self.draw_base64_dialog(ctx);
        self.draw_hash_key_dialog(ctx);
        self.draw_bcrypt_hash_dialog(ctx);
        self.draw_bcrypt_verify_dialog(ctx);
        self.draw_jwt_dialog(ctx);
        self.draw_jws_dialog(ctx);
        self.draw_jws_verify_dialog(ctx);
        self.draw_convert_dialog(ctx);

        if self.pending_goto {
            self.pending_goto = false;
            self.goto_line_column(ctx);
        }

        // Periodic session backup so unsaved work (an "Untitled" tab, or unsaved edits
        // to a file) survives a crash too, not just a clean exit via `on_exit` below.
        self.maybe_autosave_session(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_session();
    }
}
