use eframe::egui;

use crate::app::NotePageApp;
use crate::text_utils::{byte_index_from_char_index, slice_char_range};

impl NotePageApp {
    pub fn draw_toolbar(&mut self, ctx: &egui::Context) {
        if !self.show_toolbar {
            return;
        }
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("🆕 New").on_hover_text("New (Ctrl+N)").clicked() {
                    self.add_new_tab();
                }
                if ui
                    .button("📂 Open")
                    .on_hover_text("Open (Ctrl+O)")
                    .clicked()
                {
                    self.open_file();
                }
                if ui
                    .button("💾 Save")
                    .on_hover_text("Save (Ctrl+S)")
                    .clicked()
                {
                    self.save_current();
                }
                ui.separator();
                if ui.button("✂ Cut").on_hover_text("Cut selection").clicked() {
                    self.copy_or_cut_selection(ctx, true);
                }
                if ui
                    .button("📋 Copy")
                    .on_hover_text("Copy selection")
                    .clicked()
                {
                    self.copy_or_cut_selection(ctx, false);
                }
                if ui
                    .button("📥 Paste")
                    .on_hover_text("Paste from clipboard")
                    .clicked()
                {
                    self.request_paste = true;
                }
                ui.separator();
                if ui
                    .button("🔍 Find")
                    .on_hover_text("Find/Replace (Ctrl+F)")
                    .clicked()
                {
                    self.show_find_panel = true;
                }
                if ui
                    .button("🔢 Go To")
                    .on_hover_text("Go to line (Ctrl+G)")
                    .clicked()
                {
                    self.show_goto_panel = true;
                }
                if ui
                    .button("📐 Format")
                    .on_hover_text("Format Document (Ctrl+Alt+L)")
                    .clicked()
                {
                    self.request_format();
                }
                ui.separator();
                if ui.button("➖").on_hover_text("Zoom out").clicked() {
                    self.zoom_out(ctx);
                }
                if ui.button("➕").on_hover_text("Zoom in").clicked() {
                    self.zoom_in(ctx);
                }
                ui.separator();
                ui.checkbox(&mut self.word_wrap, "↩ Wrap");
            });
        });
    }

    fn copy_or_cut_selection(&mut self, ctx: &egui::Context, cut: bool) {
        let tab = self.current_tab();
        let Some((start, end)) = tab.selection_char_range else {
            self.status_message = "Nothing selected".to_string();
            return;
        };
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let text = tab.text.clone();
        let selected_text = slice_char_range(&text, start..end);
        ctx.copy_text(selected_text);
        if cut {
            let byte_start = byte_index_from_char_index(&text, start);
            let byte_end = byte_index_from_char_index(&text, end);
            let tab = self.current_tab_mut();
            tab.text.replace_range(byte_start..byte_end, "");
            tab.dirty = true;
            tab.recompute_size_caches();
            tab.cursor_char_index = start;
            tab.recompute_cursor_cache();
            tab.selection_char_range = None;
            self.status_message = "Cut".to_string();
        } else {
            self.status_message = "Copied".to_string();
        }
    }
}
