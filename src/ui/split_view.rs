use eframe::egui;
use egui::{TextBuffer, TextEdit};
use egui_extras::syntax_highlighting::highlight;

use crate::app::NotePageApp;
use crate::diff::RowKind;
use crate::text_utils::overlay_background_only;

impl NotePageApp {
    /// Draws the central panel's content when split into 2+ panes (Compare or plain
    /// split view). Each pane is a simplified editor (no column mode/multi-caret/
    /// bookmarks margin - those stay exclusive to the single-tab `draw_editor` view).
    pub fn draw_split_view(&mut self, ui: &mut egui::Ui) {
        let panes = self.split_panes.clone();
        let pane_count = panes.len();
        if pane_count < 2 {
            self.split_panes.clear();
            return;
        }

        ui.horizontal(|ui| {
            if self.compare_mode && pane_count == 2 {
                let total = self.compare_result.as_ref().map_or(0, |d| d.count());
                ui.label(format!("Compare: {total} difference block(s) (read-only)"));
                ui.label(
                    egui::RichText::new("■ - missing here")
                        .color(egui::Color32::from_rgb(230, 100, 100)),
                );
                ui.label(
                    egui::RichText::new("■ + extra/different here")
                        .color(egui::Color32::from_rgb(100, 200, 120)),
                );
                if ui.button("⟲ Recompute").clicked() {
                    self.recompute_compare();
                }
            } else {
                ui.label(format!("Split view: {pane_count} panes"));
            }
            if ui.button("Unsplit").clicked() {
                self.unsplit_view();
            }
        });
        ui.separator();

        if self.compare_mode && pane_count == 2 {
            self.draw_compare_panes(ui, panes[0], panes[1]);
        } else {
            self.draw_plain_split_panes(ui, &panes);
        }
    }

    /// Plain multi-pane split (no diff): each pane is freely editable, bound directly to
    /// its tab's real text, with a simple line-number gutter and no scroll sync.
    fn draw_plain_split_panes(&mut self, ui: &mut egui::Ui, panes: &[usize]) {
        let tab_titles: Vec<String> = self.tabs.iter().map(|t| t.display_title()).collect();
        let mut tab_change: Option<(usize, usize)> = None;

        ui.columns(panes.len(), |columns| {
            for (pane_index, column_ui) in columns.iter_mut().enumerate() {
                let tab_index = panes[pane_index];
                column_ui.horizontal(|ui| {
                    ui.label(format!("Pane {}:", pane_index + 1));
                    egui::ComboBox::from_id_salt(("split_pane_tab", pane_index))
                        .selected_text(tab_titles.get(tab_index).cloned().unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for (i, title) in tab_titles.iter().enumerate() {
                                if ui.selectable_label(i == tab_index, title).clicked() {
                                    tab_change = Some((pane_index, i));
                                }
                            }
                        });
                });
                Self::draw_editable_pane(column_ui, &mut self.tabs[tab_index].text, pane_index);
            }
        });

        if let Some((pane_index, new_tab)) = tab_change {
            self.split_panes[pane_index] = new_tab;
        }
    }

    fn draw_editable_pane(ui: &mut egui::Ui, text: &mut String, pane_index: usize) {
        let weak_color = ui.visuals().weak_text_color();
        egui::ScrollArea::both()
            .id_salt(("split_pane_scroll", pane_index))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let total_lines = text.lines().count().max(1);
                    let mut gutter_text = (1..=total_lines)
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let mut gutter_layouter =
                        move |ui: &egui::Ui, text_buffer: &dyn TextBuffer, _wrap_width: f32| {
                            let job = egui::text::LayoutJob::single_section(
                                text_buffer.as_str().to_owned(),
                                egui::TextFormat::simple(egui::FontId::monospace(13.0), weak_color),
                            );
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                    ui.add(
                        TextEdit::multiline(&mut gutter_text)
                            .id_salt(("split_pane_gutter", pane_index))
                            .font(egui::TextStyle::Monospace)
                            .interactive(false)
                            .frame(false)
                            .desired_width(44.0)
                            .desired_rows(total_lines)
                            .layouter(&mut gutter_layouter),
                    );
                    ui.separator();

                    let mut layouter =
                        move |ui: &egui::Ui, text_buffer: &dyn TextBuffer, wrap_width: f32| {
                            let mut job = highlight(
                                ui.ctx(),
                                ui.style(),
                                &egui_extras::syntax_highlighting::CodeTheme::from_style(
                                    ui.style(),
                                ),
                                text_buffer.as_str(),
                                "txt",
                            );
                            job.wrap.max_width = wrap_width;
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                    ui.add(
                        TextEdit::multiline(text)
                            .id_salt(("split_pane_editor", pane_index))
                            .font(egui::TextStyle::Monospace)
                            .frame(false)
                            .desired_width(f32::INFINITY)
                            .desired_rows(total_lines)
                            .layouter(&mut layouter),
                    );
                });
            });
    }

    /// Compare mode: renders the two panes from the ALIGNED display text computed in
    /// `self.compare_result` (blank filler rows inserted on whichever side lacks the
    /// corresponding line), so row N is always the same logical position on both sides.
    /// Read-only, since the display text is synthetic and editing it can't be mapped back
    /// to the real document.
    fn draw_compare_panes(&mut self, ui: &mut egui::Ui, left_tab: usize, right_tab: usize) {
        let Some(result) = &self.compare_result else {
            ui.label("Nothing to compare yet.");
            return;
        };
        let left_display = result.left_display.clone();
        let right_display = result.right_display.clone();
        let left_rows = result.left_rows.clone();
        let right_rows = result.right_rows.clone();

        let tab_titles: Vec<String> = self.tabs.iter().map(|t| t.display_title()).collect();
        let mut tab_change: Option<(usize, usize)> = None;

        let forced_fraction = self
            .compare_scroll_force
            .then_some(self.compare_scroll_fraction);
        let mut observed: [Option<(f32, f32)>; 2] = [None, None];
        let panes = [left_tab, right_tab];

        ui.columns(2, |columns| {
            for (pane_index, column_ui) in columns.iter_mut().enumerate() {
                let tab_index = panes[pane_index];
                let (display, rows) = if pane_index == 0 {
                    (&left_display, &left_rows)
                } else {
                    (&right_display, &right_rows)
                };

                column_ui.horizontal(|ui| {
                    ui.label(format!("Pane {}:", pane_index + 1));
                    egui::ComboBox::from_id_salt(("split_pane_tab", pane_index))
                        .selected_text(tab_titles.get(tab_index).cloned().unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for (i, title) in tab_titles.iter().enumerate() {
                                if ui.selectable_label(i == tab_index, title).clicked() {
                                    tab_change = Some((pane_index, i));
                                }
                            }
                        });
                });

                let forced_offset =
                    forced_fraction.map(|f| f * self.compare_pane_max_range[pane_index]);
                let (offset_y, max_range) =
                    Self::draw_compare_pane(column_ui, display, rows, pane_index, forced_offset);
                self.compare_pane_max_range[pane_index] = max_range;
                observed[pane_index] = Some((offset_y, max_range));
            }
        });

        if self.compare_scroll_force {
            self.compare_scroll_force = false;
        } else if let [Some((o0, max0)), Some((o1, max1))] = observed {
            // Ignore a pane that has nothing to scroll (max_range ~ 0) - it can never
            // meaningfully "move", and treating its always-zero fraction as a real change
            // is what caused the two panes to fight each other over the scroll position.
            let fraction0 = (max0 > 1.0).then(|| (o0 / max0).clamp(0.0, 1.0));
            let fraction1 = (max1 > 1.0).then(|| (o1 / max1).clamp(0.0, 1.0));
            let epsilon = 0.002;
            if let Some(f0) = fraction0 {
                if (f0 - self.compare_scroll_fraction).abs() > epsilon {
                    self.compare_scroll_fraction = f0;
                    self.compare_scroll_force = true;
                }
            }
            if !self.compare_scroll_force {
                if let Some(f1) = fraction1 {
                    if (f1 - self.compare_scroll_fraction).abs() > epsilon {
                        self.compare_scroll_fraction = f1;
                        self.compare_scroll_force = true;
                    }
                }
            }
        }

        if let Some((pane_index, new_tab)) = tab_change {
            self.split_panes[pane_index] = new_tab;
            self.recompute_compare();
        }
    }

    /// Draws one read-only, aligned compare pane (gutter with per-row diff marker, then
    /// the aligned display text with diff/filler rows tinted), returning
    /// `(vertical_offset, max_scroll_range)` for cross-pane scroll sync.
    ///
    /// Marking is symmetric and per-row, not per-pane: on EITHER side, a blank `Filler`
    /// row (this file has nothing here, the other side does) is marked red `-` ("missing
    /// compared to the other file"), and a real `Diff` row (this file has content the
    /// other side lacks here) is marked green `+` ("extra/different compared to the other
    /// file"). This means both panes can show both `-` and `+` markers, unlike a plain
    /// git-style left=removed/right=added layout.
    fn draw_compare_pane(
        ui: &mut egui::Ui,
        display: &str,
        rows: &[RowKind],
        pane_index: usize,
        forced_offset: Option<f32>,
    ) -> (f32, f32) {
        let red = egui::Color32::from_rgb(230, 100, 100);
        let green = egui::Color32::from_rgb(100, 200, 120);
        let red_tint = egui::Color32::from_rgba_unmultiplied(220, 60, 60, 60);
        let green_tint = egui::Color32::from_rgba_unmultiplied(60, 180, 90, 60);
        let weak_color = ui.visuals().weak_text_color();
        let total_rows = rows.len().max(1);
        let rows_owned = rows.to_vec();

        let mut scroll_area = egui::ScrollArea::both()
            .id_salt(("compare_pane_scroll", pane_index))
            .auto_shrink([false, false]);
        if let Some(offset) = forced_offset {
            scroll_area = scroll_area.vertical_scroll_offset(offset);
        }

        let output = scroll_area.show(ui, |ui| {
            ui.horizontal_top(|ui| {
                // Gutter: real row number (skipping filler rows, which have none) + marker.
                let mut real_line = 0usize;
                let mut gutter_text = rows_owned
                    .iter()
                    .map(|row| match row {
                        RowKind::Filler => "     -".to_string(),
                        _ => {
                            real_line += 1;
                            let m = if *row == RowKind::Diff { '+' } else { ' ' };
                            format!("{:>4} {}", real_line, m)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let gutter_rows = rows_owned.clone();
                let mut gutter_layouter =
                    move |ui: &egui::Ui, text_buffer: &dyn TextBuffer, _wrap_width: f32| {
                        let mut job = egui::text::LayoutJob::default();
                        for (i, line) in text_buffer.as_str().split('\n').enumerate() {
                            let color = match gutter_rows.get(i) {
                                Some(RowKind::Diff) => green,
                                Some(RowKind::Filler) => red,
                                _ => weak_color,
                            };
                            job.append(
                                line,
                                0.0,
                                egui::TextFormat::simple(egui::FontId::monospace(13.0), color),
                            );
                            job.append(
                                "\n",
                                0.0,
                                egui::TextFormat::simple(egui::FontId::monospace(13.0), color),
                            );
                        }
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                ui.add(
                    TextEdit::multiline(&mut gutter_text)
                        .id_salt(("compare_pane_gutter", pane_index))
                        .font(egui::TextStyle::Monospace)
                        .interactive(false)
                        .frame(false)
                        .desired_width(68.0)
                        .desired_rows(total_rows)
                        .layouter(&mut gutter_layouter),
                );
                ui.separator();

                let mut content = display.to_string();
                let content_rows = rows_owned.clone();
                let mut layouter =
                    move |ui: &egui::Ui, text_buffer: &dyn TextBuffer, _wrap_width: f32| {
                        let text = text_buffer.as_str();
                        let mut job = highlight(
                            ui.ctx(),
                            ui.style(),
                            &egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style()),
                            text,
                            "txt",
                        );
                        // Never wrap: wrapping would add extra visual rows the gutter (one row
                        // per aligned logical row) doesn't know about, desyncing every marker
                        // below it. Long lines scroll horizontally instead.
                        job.wrap.max_width = f32::INFINITY;
                        let mut start = 0usize;
                        for (idx, line) in text.split('\n').enumerate() {
                            if idx >= content_rows.len() {
                                break;
                            }
                            let end = start + line.len();
                            match content_rows[idx] {
                                RowKind::Diff => overlay_background_only(
                                    &mut job,
                                    start..end.max(start + 1).min(text.len()),
                                    green_tint,
                                ),
                                RowKind::Filler => overlay_background_only(
                                    &mut job,
                                    start..end.max(start + 1).min(text.len()),
                                    red_tint,
                                ),
                                RowKind::Equal => {}
                            }
                            start = end + 1;
                        }
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                ui.add(
                    TextEdit::multiline(&mut content)
                        .id_salt(("compare_pane_editor", pane_index))
                        .font(egui::TextStyle::Monospace)
                        .interactive(false)
                        .frame(false)
                        .desired_width(f32::INFINITY)
                        .desired_rows(total_rows)
                        .layouter(&mut layouter),
                );
            });
        });

        let max_range = (output.content_size.y - output.inner_rect.height()).max(0.0);
        (output.state.offset.y, max_range)
    }

    pub fn draw_compare_picker(&mut self, ctx: &egui::Context) {
        if !self.show_compare_picker {
            return;
        }
        let tab_titles: Vec<String> = self.tabs.iter().map(|t| t.display_title()).collect();
        let mut open = self.show_compare_picker;
        let mut start = false;
        egui::Window::new("Compare Tabs")
            .open(&mut open)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Left:");
                    egui::ComboBox::from_id_salt("compare_pick_left")
                        .selected_text(
                            tab_titles
                                .get(self.compare_pick_left)
                                .cloned()
                                .unwrap_or_default(),
                        )
                        .show_ui(ui, |ui| {
                            for (i, title) in tab_titles.iter().enumerate() {
                                ui.selectable_value(&mut self.compare_pick_left, i, title);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Right:");
                    egui::ComboBox::from_id_salt("compare_pick_right")
                        .selected_text(
                            tab_titles
                                .get(self.compare_pick_right)
                                .cloned()
                                .unwrap_or_default(),
                        )
                        .show_ui(ui, |ui| {
                            for (i, title) in tab_titles.iter().enumerate() {
                                ui.selectable_value(&mut self.compare_pick_right, i, title);
                            }
                        });
                });
                ui.separator();
                if ui.button("Compare").clicked() {
                    start = true;
                }
            });
        self.show_compare_picker = open;
        if start {
            self.start_compare(self.compare_pick_left, self.compare_pick_right);
        }
    }
}
