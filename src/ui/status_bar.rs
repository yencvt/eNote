use eframe::egui;

use crate::app::NotePageApp;

impl NotePageApp {
    pub fn draw_status_bar(&mut self, ctx: &egui::Context) {
        if !self.show_status_bar {
            return;
        }
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .show(ctx, |ui| {
                let (line, col) = self.line_col();
                let tab = self.current_tab();
                // Cached (updated only when text/cursor actually change - see
                // `editor_view::draw_editor`), instead of `tab.text.chars().count()` /
                // `tab.text.lines().count()` here - full O(document length) scans that
                // used to run every single frame just to paint these two numbers.
                let length = tab.cached_char_count;
                let lines = tab.cached_line_count;
                let sel_len = tab
                    .selection_char_range
                    .map(|(s, e)| s.abs_diff(e))
                    .unwrap_or(0);
                let mode = if tab.overwrite_mode { "OVR" } else { "INS" };

                ui.horizontal(|ui| {
                    ui.label(&self.status_message);
                    ui.separator();
                    ui.label(format!("length : {}", length));
                    ui.separator();
                    ui.label(format!("lines : {}", lines));
                    ui.separator();
                    ui.label(format!("Ln : {}", line));
                    ui.separator();
                    ui.label(format!("Col : {}", col));
                    ui.separator();
                    ui.label(format!("Sel : {}", sel_len));
                    ui.separator();
                    ui.label(tab.line_ending.label());
                    ui.separator();
                    ui.label(tab.encoding.label());
                    ui.separator();
                    ui.label(format!("{:.0}%", self.font_size / 13.0 * 100.0));
                    ui.separator();
                    ui.label(mode);
                });
            });
    }
}
