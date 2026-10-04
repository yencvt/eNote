use eframe::egui;

use crate::app::NotePageApp;

impl NotePageApp {
    pub fn draw_tabs(&mut self, ui: &mut egui::Ui) {
        let mut ordered_indices = Vec::with_capacity(self.tabs.len());
        for (idx, tab) in self.tabs.iter().enumerate() {
            if tab.pinned {
                ordered_indices.push(idx);
            }
        }
        for (idx, tab) in self.tabs.iter().enumerate() {
            if !tab.pinned {
                ordered_indices.push(idx);
            }
        }

        let mut select_tab = None;
        let mut toggle_pin = None;
        let mut close_tab = None;

        ui.horizontal_wrapped(|ui| {
            for index in ordered_indices {
                let mut title = self.tabs[index].display_title();
                let selected = index == self.active_tab;
                let pinned = self.tabs[index].pinned;

                if pinned {
                    title = format!("📌 {title}");
                }

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(selected, title)
                            .on_hover_text("Click to switch tab")
                            .clicked()
                        {
                            select_tab = Some(index);
                        }
                        if ui
                            .small_button(if pinned { "📍" } else { "📌" })
                            .on_hover_text(if pinned {
                                "Unpin this tab"
                            } else {
                                "Pin this tab"
                            })
                            .clicked()
                        {
                            toggle_pin = Some(index);
                        }
                        if ui
                            .small_button("✖")
                            .on_hover_text("Close this tab")
                            .clicked()
                        {
                            close_tab = Some(index);
                        }
                    });
                });
            }
        });

        if let Some(index) = select_tab {
            self.active_tab = index;
        }
        if let Some(index) = toggle_pin {
            if let Some(tab) = self.tabs.get_mut(index) {
                tab.pinned = !tab.pinned;
                self.status_message = if tab.pinned {
                    "Tab pinned".to_string()
                } else {
                    "Tab unpinned".to_string()
                };
            }
        }
        if let Some(index) = close_tab {
            self.close_tab(index);
        }

        ui.separator();
    }
}
