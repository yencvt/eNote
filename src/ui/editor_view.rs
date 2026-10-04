use base64::Engine as _;
use eframe::egui;
use egui::text::CCursor;
use egui::{Id, TextBuffer, TextEdit};
use egui_extras::syntax_highlighting::highlight;

use crate::app::{EditorContextActions, NotePageApp};
use crate::autocomplete::{collect_words, current_word_prefix, suggestions};
use crate::text_utils::{
    byte_index_from_char_index, char_index_for_line_col, line_col_for_char_index,
    read_clipboard_text, replace_char_range, set_editor_selection, slice_char_range,
    word_range_at_char_index,
};

impl NotePageApp {
    /// Above this document size, the editor skips syntect-based syntax highlighting and
    /// falls back to a single plain-text layout section instead (see the `layouter`
    /// closure in `draw_editor`). Syntect's tokenizer re-runs on every edit (and its
    /// output - one layout section per token - gets cloned and re-spliced with the
    /// selection-background overlay on *every single repaint*, not just edits), so for a
    /// "heavy" file this was the main source of stutter/freezes, especially once a
    /// selection existed (e.g. right after double-clicking a word) since every repaint
    /// while a selection is visible re-splices that overlay across however many sections
    /// syntax highlighting produced. 200 KB keeps ordinary source files colored while
    /// capping the worst case to a small, constant number of sections.
    const SYNTAX_HIGHLIGHT_MAX_BYTES: usize = 200_000;

    /// Builds a naive fold map for the active tab: any line whose trimmed text ends with
    /// one of `{ : ( [` starts a block that runs until indentation returns to its level
    /// (or less). This is a best-effort heuristic, not a real language-aware folder.
    pub fn fold_all_current_tab(&mut self) {
        let text = self.current_tab().text.clone();
        let lines: Vec<&str> = text.split('\n').collect();
        let mut folds = std::collections::BTreeSet::new();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_end();
            if trimmed.ends_with(['{', ':', '(', '[']) {
                let indent = line.len() - line.trim_start().len();
                let mut end = i;
                for (j, later) in lines.iter().enumerate().skip(i + 1) {
                    if later.trim().is_empty() {
                        continue;
                    }
                    let later_indent = later.len() - later.trim_start().len();
                    if later_indent <= indent {
                        break;
                    }
                    end = j;
                }
                if end > i {
                    folds.insert((i, end));
                }
            }
        }
        self.current_tab_mut().folds = folds;
        self.status_message =
            "Folded all blocks (heuristic; editing is locked while folded)".to_string();
    }

    /// Gutter to the left of line numbers: bookmark toggle + fold +/- markers. Returns
    /// (bookmark_line_clicked, fold_start_line_clicked).
    fn draw_margin(
        ui: &mut egui::Ui,
        total_lines: usize,
        row_height: f32,
        bookmarks: &std::collections::BTreeSet<usize>,
        fold_starts: &[usize],
    ) -> (Option<usize>, Option<usize>) {
        let mut bookmark_clicked = None;
        let mut fold_clicked = None;

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in 0..total_lines {
                ui.horizontal(|ui| {
                    ui.set_min_height(row_height);
                    ui.set_max_height(row_height);
                    let mark = if bookmarks.contains(&line) {
                        "●"
                    } else {
                        " "
                    };
                    if ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(mark)
                                    .color(egui::Color32::from_rgb(220, 80, 80)),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_text("Toggle bookmark")
                        .clicked()
                    {
                        bookmark_clicked = Some(line);
                    }
                    if fold_starts.contains(&line) {
                        if ui
                            .small_button("-")
                            .on_hover_text("Fold this block")
                            .clicked()
                        {
                            fold_clicked = Some(line);
                        }
                    } else {
                        ui.add_space(16.0);
                    }
                });
            }
        });

        (bookmark_clicked, fold_clicked)
    }

    /// Computes how wide the line-number gutter needs to be for a document with
    /// `total_lines` lines, based on actual glyph metrics at `font_size` - rather than a
    /// one-size-fits-all fixed width (which was both wastefully wide for small/typical
    /// files and, in principle, too narrow for 5+ digit line counts).
    fn line_number_gutter_width(ui: &egui::Ui, total_lines: usize, font_size: f32) -> f32 {
        let font_id = egui::FontId::monospace(font_size);
        // Pad the digit count by one as a cheap safety margin for the "trailing newline"
        // edge case (see `paint_line_numbers_and_current_line`'s doc comment), so the
        // gutter is never one digit too narrow right at a power-of-ten line count.
        let digit_count = (total_lines + 1).to_string().len();
        let digit_width = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));
        digit_count as f32 * digit_width + 6.0
    }

    /// Paints line numbers (right-aligned in `gutter_rect`) and a subtle "current line"
    /// highlight, both positioned from the real text editor's own galley rows - instead of
    /// a separate, independently-sized widget - so they can never drift out of sync with
    /// the text no matter how long the document is. Only the first visual row of each
    /// logical line gets a number (rows that continue a word-wrapped line are left blank),
    /// matching most editors' convention; a document whose text ends with `\n` has one
    /// extra trailing empty row, which correctly gets the next line number too (there
    /// really is a valid, empty final line there).
    ///
    /// When `selected_char_range` is a non-empty selection, every row the selection
    /// touches is highlighted (not just the caret's row) - matching most editors'
    /// convention of highlighting the whole selected block, including lines that are
    /// fully covered because the selection runs through them without starting or ending
    /// there. A row that the selection only reaches via its line-ending `\n` (i.e. the
    /// selection stops exactly at that row's start, selecting nothing visible on it) is
    /// correctly left unhighlighted, same as most editors.
    fn paint_line_numbers_and_current_line(
        ui: &egui::Ui,
        gutter_rect: egui::Rect,
        output: &egui::text_edit::TextEditOutput,
        cursor_char_index: usize,
        selected_char_range: Option<std::ops::Range<usize>>,
        font_size: f32,
    ) {
        let galley = &output.galley;
        let galley_pos = output.galley_pos;
        let font_id = egui::FontId::monospace(font_size);
        let number_color = ui.visuals().weak_text_color();
        let painter = ui.painter();
        let highlight_color = ui.visuals().selection.bg_fill.gamma_multiply(0.18);

        let has_selection = selected_char_range
            .as_ref()
            .is_some_and(|range| range.start != range.end);

        if !has_selection {
            // No selection: highlight just the single row the caret is on. Anchored to
            // the real caret position (via `pos_from_cursor`) rather than a row-range
            // lookup, so word-wrap boundary ambiguity resolves exactly like the caret
            // egui itself draws. `pos_from_cursor` already clamps an out-of-range cursor
            // to the end of the galley internally (see egui's `layout_from_cursor`), so
            // there's no need to pre-clamp against a `text.chars().count()` scan here -
            // that was a full O(document length) scan paid on every frame regardless of
            // whether the cursor was ever actually out of range.
            let cursor = egui::text::CCursor::new(cursor_char_index);
            let caret_rect = galley
                .pos_from_cursor(cursor)
                .translate(galley_pos.to_vec2());
            let highlight_rect = egui::Rect::from_min_max(
                egui::pos2(gutter_rect.min.x, caret_rect.min.y),
                egui::pos2(output.response.rect.right(), caret_rect.max.y),
            );
            painter.rect_filled(highlight_rect, 0.0, highlight_color);
        }

        // Line numbers, plus (when there IS a selection) a highlight band for every row
        // whose char range overlaps the selection - tracked via a running char offset
        // since each `Row` only stores its own glyphs, not its absolute position in the
        // document. Painting (the expensive part - each `painter.text()` call does its
        // own font layout) is skipped for rows outside the visible scroll viewport -
        // `galley.rows` covers the *entire* document (egui's `TextEdit` doesn't virtualize
        // rendering), so without this a long file would pay for laying out a line-number
        // label for every single line on every single frame, even the thousands that
        // aren't currently on screen.
        let clip_rect = ui.clip_rect();
        let mut row_char_start: usize = 0;
        let mut line_number: u64 = 1;
        for (i, row) in galley.rows.iter().enumerate() {
            let row_char_len = row.glyphs.len() + row.ends_with_newline as usize;
            let row_char_end = row_char_start + row_char_len;
            let row_rect = row.rect().translate(galley_pos.to_vec2());
            let row_visible = clip_rect.intersects(row_rect);

            if row_visible && let Some(range) = &selected_char_range {
                if range.start != range.end
                    && range.start < row_char_end
                    && row_char_start < range.end
                {
                    let band = egui::Rect::from_min_max(
                        egui::pos2(gutter_rect.min.x, row_rect.min.y),
                        egui::pos2(output.response.rect.right(), row_rect.max.y),
                    );
                    painter.rect_filled(band, 0.0, highlight_color);
                }
            }

            let starts_new_line = i == 0 || galley.rows[i - 1].ends_with_newline;
            if starts_new_line {
                if row_visible {
                    painter.text(
                        egui::pos2(gutter_rect.right() - 3.0, row_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        line_number.to_string(),
                        font_id.clone(),
                        number_color,
                    );
                }
                line_number += 1;
            }

            row_char_start = row_char_end;
        }
    }

    /// If `persisted_selection` is `Some`, returns a galley with that selection's
    /// highlight baked in via egui's own `paint_text_selection` - the exact same function
    /// `TextEdit` itself uses internally, so the result is pixel-identical to egui's
    /// native, focused-widget selection rendering, instead of a hand-rolled overlay with a
    /// visibly different style. This is only meant to be used on a frame where the widget
    /// is known to definitely *not* be focused/interactive (so egui's own native painting
    /// - which only runs when there's a live `cursor_range` - wouldn't run at all), to
    /// keep a selection visible while e.g. the right-click menu or a dialog is open.
    ///
    /// Reuses `cached_bake` (base galley + range + result, as last computed) whenever
    /// `galley` is the *exact same* `Arc` as the base it was computed from (checked via
    /// `Arc::ptr_eq`, not content equality - cheap, O(1)) and the range hasn't changed,
    /// instead of recomputing. This matters because `paint_text_selection` works by
    /// `Arc::make_mut`-ing the galley, and since `Galley` derives `Clone` at the struct
    /// level, that clones its *entire* `Vec<PlacedRow>` - one entry per line in the whole
    /// document, not just the selected part - every time it's called on a galley that's
    /// shared (which it always is here, since the original stays in `cached_galley`).
    /// Without this memoization, keeping a selection open (e.g. via the right-click menu)
    /// redid that whole-document-sized clone on *every single frame* the menu stayed open,
    /// which is what made opening it stutter on larger files.
    ///
    /// Never mutates the input `galley` itself - only a clone - so the original stays
    /// safe to cache (see `can_try_reuse_galley` and `last_clean_galley` in `draw_editor`).
    fn bake_persisted_selection_if_needed(
        galley: std::sync::Arc<egui::Galley>,
        ui: &egui::Ui,
        persisted_selection: Option<(usize, usize)>,
        cached_bake: &Option<(
            std::sync::Arc<egui::Galley>,
            (usize, usize),
            std::sync::Arc<egui::Galley>,
        )>,
    ) -> (
        std::sync::Arc<egui::Galley>,
        Option<(
            std::sync::Arc<egui::Galley>,
            (usize, usize),
            std::sync::Arc<egui::Galley>,
        )>,
    ) {
        let Some(range) = persisted_selection else {
            return (galley, None);
        };
        if let Some((base, cached_range, result)) = cached_bake
            && *cached_range == range
            && std::sync::Arc::ptr_eq(base, &galley)
        {
            return (result.clone(), cached_bake.clone());
        }
        let mut baked = galley.clone();
        let cursor_range = egui::text_selection::CCursorRange::two(
            egui::text::CCursor::new(range.0),
            egui::text::CCursor::new(range.1),
        );
        egui::text_selection::visuals::paint_text_selection(
            &mut baked,
            ui.visuals(),
            &cursor_range,
            None,
        );
        (baked.clone(), Some((galley, range, baked)))
    }

    /// Default polling interval for "View Log (tail -f)" tabs - frequent enough to feel
    /// realtime, but throttled so a live log tab doesn't re-stat/re-read its file on
    /// every single repaint (which can happen many times a second while, e.g., the mouse
    /// is moving over the window).
    const LOG_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);

    /// For a "View Log (tail -f)" tab (see `Document::is_log_view`), checks the on-disk
    /// file for bytes written since the last poll and appends any new content straight
    /// onto `text`. Returns `true` if anything was appended, so the caller can refresh the
    /// size/line caches without marking the tab dirty - a growing log file isn't an
    /// unsaved edit. Handles the file shrinking (e.g. `logrotate` truncating or replacing
    /// it) by re-reading it from scratch, and keeps a held-back trailing partial
    /// multi-byte character (`log_pending_bytes`) across polls so a read landing mid
    /// character doesn't corrupt the decode.
    fn poll_log_tail(&mut self, active: usize, text: &mut String, ctx: &egui::Context) -> bool {
        if !self.tabs[active].is_log_view {
            return false;
        }
        // Keep the UI repainting periodically on its own, so newly appended lines show up
        // even while the user isn't doing anything else that would otherwise trigger one.
        ctx.request_repaint_after(Self::LOG_POLL_INTERVAL);

        let now = std::time::Instant::now();
        if let Some(last) = self.tabs[active].log_last_poll
            && now.duration_since(last) < Self::LOG_POLL_INTERVAL
        {
            return false;
        }
        self.tabs[active].log_last_poll = Some(now);

        let Some(path) = self.tabs[active].path.clone() else {
            return false;
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            return false;
        };
        let len = metadata.len();
        let read_pos = self.tabs[active].log_read_bytes;

        if len < read_pos {
            // The file shrank - most likely rotated/truncated by an external process
            // (e.g. logrotate) - so the previous byte offset no longer means anything.
            // Simplest correct behavior: reload it from scratch and start tailing again.
            return match std::fs::read(&path) {
                Ok(bytes) => {
                    let encoding = self.tabs[active].encoding;
                    *text = encoding.decode_with(&bytes);
                    self.tabs[active].log_read_bytes = bytes.len() as u64;
                    self.tabs[active].log_pending_bytes.clear();
                    self.status_message = format!(
                        "Log file was rotated/truncated - reloaded {}",
                        path.display()
                    );
                    true
                }
                Err(_) => false,
            };
        }

        if len == read_pos {
            return false;
        }

        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(&path) else {
            return false;
        };
        if file.seek(SeekFrom::Start(read_pos)).is_err() {
            return false;
        }
        let mut new_bytes = Vec::new();
        if file.read_to_end(&mut new_bytes).is_err() || new_bytes.is_empty() {
            return false;
        }
        self.tabs[active].log_read_bytes = read_pos + new_bytes.len() as u64;

        let encoding = self.tabs[active].encoding;
        let mut pending = std::mem::take(&mut self.tabs[active].log_pending_bytes);
        let appended = crate::text_utils::decode_log_increment(encoding, &mut pending, &new_bytes);
        self.tabs[active].log_pending_bytes = pending;

        if appended.is_empty() {
            return false;
        }
        text.push_str(&appended);
        true
    }

    pub fn draw_editor(&mut self, ui: &mut egui::Ui) {
        let active = self.active_tab;

        // Folded tabs are shown read-only via a collapsed projection of the text, so the
        // real buffer can never desync from what's displayed. Editing requires unfolding
        // first (View > Unfold All, or there is currently no per-block unfold button -
        // known simplification, see repo notes).
        if !self.tabs[active].folds.is_empty() {
            self.draw_folded_readonly_view(ui);
            return;
        }

        let language = self.tabs[active].language;
        let word_wrap = self.word_wrap;
        // `mem::take` instead of `.clone()`: we hand the buffer to the `TextEdit` below and
        // write it straight back into `self.tabs[active].text` at the end of this function
        // (nothing else reads the tab's `text` field in between), so there's no need to
        // pay for an extra full-document copy on every single frame - that cost was
        // previously doubled by a now-removed full-text snapshot used to detect edits (see
        // `text_mutated_this_frame` below), and both added up fast on large files since
        // this function runs every repaint, not just on edits.
        let mut text = std::mem::take(&mut self.tabs[active].text);
        let log_appended_this_frame = self.poll_log_tail(active, &mut text, ui.ctx());
        // Explicit "did anything actually edit the text this frame" flag, set at each
        // mutation site below (pre-widget actions like autocomplete-accept/multi-cursor
        // typing, and `output.response.changed()` for the widget's own native
        // typing/backspace/paste/IME after it runs) - used to gate autocomplete and to
        // keep `dirty`/the size caches in sync. Replaces an earlier approach that cloned
        // the *entire* document text at the start of the frame just to compare it against
        // the final text at the end - two extra O(document length) full-buffer costs
        // (clone + comparison) paid on *every single frame*, edit or not. Also set (but
        // without marking the tab dirty, see below) when the "Reload" button on a live
        // log tab's control bar clears the buffer for a fresh full re-read, so the galley
        // cache doesn't keep showing the old (now-stale) content for a frame.
        let mut text_mutated_this_frame = false;
        if self.tabs[active].is_log_view {
            ui.horizontal(|ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 90, 90),
                    "🔴 Live Log (tail -f)",
                );
                if let Some(path) = &self.tabs[active].path {
                    ui.label(egui::RichText::new(path.display().to_string()).weak());
                }
                ui.checkbox(&mut self.tabs[active].log_follow, "Follow")
                    .on_hover_text("Auto-scroll to the bottom as new content arrives");
                if ui
                    .small_button("Reload")
                    .on_hover_text("Re-read the whole file from the start")
                    .clicked()
                {
                    // Reset tracking so the next poll re-reads the entire file - also
                    // clear the buffer itself here, otherwise that re-read would be
                    // appended after (duplicating) the content already shown.
                    text.clear();
                    text_mutated_this_frame = true;
                    self.tabs[active].log_read_bytes = 0;
                    self.tabs[active].log_pending_bytes.clear();
                    self.tabs[active].log_last_poll = None;
                }
                if ui
                    .small_button("Stop Watching")
                    .on_hover_text("Keep the tab open as a normal (non-tailing) file")
                    .clicked()
                {
                    self.tabs[active].is_log_view = false;
                    self.status_message = "Stopped watching for log changes".to_string();
                }
            });
            ui.separator();
        }
        // keep `dirty`/the size caches in sync. Replaces an earlier approach that cloned
        // the *entire* document text at the start of the frame just to compare it against
        // the final text at the end - two extra O(document length) full-buffer costs
        // (clone + comparison) paid on *every single frame*, edit or not.
        let mut text_mutated_this_frame = false;
        let visible_height = ui.available_height();
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let min_visible_rows = (visible_height / row_height).ceil().max(1.0) as usize;
        let code_theme = self.code_theme.clone();
        let tab_id = self.tabs[active].id;
        let editor_id = Id::new(format!("editor-{}", tab_id));
        let previous_cursor_char_index = self.tabs[active].cursor_char_index;
        let mut cursor_char_index = previous_cursor_char_index;
        let previous_selection = self.tabs[active].selection_char_range;
        let mut selection_char_range = previous_selection;
        let mut actions = EditorContextActions::default();

        if self.editor_context_menu_open && self.editor_context_menu_tab != active {
            self.editor_context_menu_open = false;
        }

        // Freeze the TextEdit's own pointer handling while the context menu is open, or
        // during the exact frame a right-click is detected. egui's TextEdit collapses the
        // selection on ANY pointer press (including the secondary button) when interactive,
        // so disabling interaction here is what prevents the caret/selection from being
        // disturbed by right-clicking or by clicking menu items.
        let menu_open_here =
            self.editor_context_menu_open && self.editor_context_menu_tab == active;
        let pending_secondary_freeze = !menu_open_here
            && ui.input(|i| i.pointer.secondary_pressed())
            && self.last_editor_rect.is_some_and(|rect| {
                ui.input(|i| i.pointer.interact_pos())
                    .is_some_and(|pos| rect.contains(pos))
            });
        // Alt+drag (Shift optional) starts/continues a Notepad++-style column (box)
        // selection with the mouse: the box spans from the caret's position *before* the
        // gesture to wherever the pointer currently is. Freeze the TextEdit's own
        // click-and-drag handling for the whole gesture (including the frame the button is
        // released on) so it never installs its own single-range selection/cursor on top
        // of ours, and so the pre-click caret position isn't disturbed before we can read
        // it as the anchor.
        let column_drag_continuing = self.tabs[active].multi_cursor_drag_anchor.is_some();
        let column_drag_starting = !menu_open_here
            && !column_drag_continuing
            && ui.input(|i| i.modifiers.alt && i.pointer.primary_pressed())
            && self.last_editor_rect.is_some_and(|rect| {
                ui.input(|i| i.pointer.interact_pos())
                    .is_some_and(|pos| rect.contains(pos))
            });
        if column_drag_starting {
            // Temporary diagnostic: confirms the Alt+click gesture was actually detected
            // at the input layer (mouse/modifier/rect), independent of what renders after.
            self.status_message = "Column mode: Alt+click detected".to_string();
        }
        let freeze_interaction = menu_open_here
            || pending_secondary_freeze
            || column_drag_continuing
            || column_drag_starting;

        // Ctrl+click adds an independent extra caret at the clicked position, Notepad++
        // "multi-editing" style (distinct from the column/box mode above: these carets are
        // NOT aligned to one shared column, each keeps typing at its own position). Freeze
        // the TextEdit's own click handling on that frame so it doesn't also move the
        // single internal cursor there.
        let ctrl_click_starting = !menu_open_here
            && !column_drag_starting
            && !column_drag_continuing
            && ui.input(|i| i.modifiers.ctrl && !i.modifiers.alt && i.pointer.primary_pressed())
            && self.last_editor_rect.is_some_and(|rect| {
                ui.input(|i| i.pointer.interact_pos())
                    .is_some_and(|pos| rect.contains(pos))
            });
        let freeze_interaction = freeze_interaction || ctrl_click_starting;

        // Double-click "select word": handled entirely ourselves (via
        // `word_range_at_char_index`, below, after the real click position is known from
        // the galley) instead of letting egui's `TextEdit` do its own built-in
        // double-click handling, which re-selects the word by scanning *backward* from
        // the whole-document-reversed text on every single double-click - an
        // O(document length) cost paid no matter where the click lands, which is what
        // made double-clicking to select a word freeze the UI on large files. Freezing
        // interaction here suppresses egui's own (slow) handling for this frame.
        let double_click_starting = !menu_open_here
            && !column_drag_starting
            && !column_drag_continuing
            && !ctrl_click_starting
            && ui.input(|i| {
                i.pointer
                    .button_double_clicked(egui::PointerButton::Primary)
            })
            && self.last_editor_rect.is_some_and(|rect| {
                ui.input(|i| i.pointer.interact_pos())
                    .is_some_and(|pos| rect.contains(pos))
            });
        let freeze_interaction = freeze_interaction || double_click_starting;

        // Used for the gutter's multi-line highlight band below.
        let gutter_selection_range = previous_selection
            .map(|(start, end)| if start <= end { start..end } else { end..start });

        let has_editor_focus = ui.memory(|m| m.has_focus(editor_id));
        // Whether this frame is one where `editor.show()` below definitely *won't* report
        // a live `cursor_range` (because the widget isn't both focused and interactive) -
        // known for certain ahead of time, since both of these are already decided above.
        // On such a frame, egui's own native selection-highlight painting (which only runs
        // when there's a live `cursor_range`) won't happen at all, so a persisted
        // selection - the context menu being open, say - would otherwise just vanish while
        // it's up. See `persisted_selection_to_bake`, captured into the `layouter` closure
        // below, for how this is handled without a separate, visually-different overlay.
        let definitely_not_focused_this_frame = freeze_interaction || !has_editor_focus;
        let persisted_selection_to_bake = if definitely_not_focused_this_frame {
            previous_selection
                .map(|(s, e)| if s <= e { (s, e) } else { (e, s) })
                .filter(|(s, e)| s != e)
        } else {
            None
        };

        // Alt+Shift+Down/Up: Notepad++-style column mode, placing one caret per line.
        let alt_shift_down = has_editor_focus
            && ui.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers {
                        alt: true,
                        shift: true,
                        ..Default::default()
                    },
                    egui::Key::ArrowDown,
                )
            });
        let alt_shift_up = has_editor_focus
            && ui.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers {
                        alt: true,
                        shift: true,
                        ..Default::default()
                    },
                    egui::Key::ArrowUp,
                )
            });

        if alt_shift_down || alt_shift_up {
            let total_lines = text.lines().count().max(1);
            let tab = &mut self.tabs[active];
            if !tab.multi_cursor_active {
                let (line, col) = line_col_for_char_index(&text, tab.cursor_char_index);
                tab.multi_cursor_origin_line = line.saturating_sub(1);
                tab.multi_cursor_column = col.saturating_sub(1);
                tab.multi_cursor_start_column = tab.multi_cursor_column;
                tab.multi_cursor_lines = vec![tab.multi_cursor_origin_line];
                tab.multi_cursor_active = true;
            }
            // Keyboard-driven: clamp to real characters like before (no virtual space).
            tab.multi_cursor_virtual_space = false;
            if alt_shift_down {
                if let Some(&last) = tab.multi_cursor_lines.last() {
                    if last + 1 < total_lines {
                        tab.multi_cursor_lines.push(last + 1);
                    }
                }
            } else if let Some(&first) = tab.multi_cursor_lines.first() {
                if first > 0 {
                    tab.multi_cursor_lines.insert(0, first - 1);
                }
            }
        }

        // Plain (no-modifier) arrow keys move the column/line block instead of exiting
        // column mode, like Notepad++: Left/Right shifts the shared column, Up/Down shifts
        // the whole active line range by one. Consumed here so the frozen-free TextEdit
        // doesn't also move its own single internal cursor with the same keypress.
        if self.tabs[active].multi_cursor_active {
            let (move_left, move_right, move_up, move_down) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                )
            });
            let total_lines = text.lines().count().max(1);
            let tab = &mut self.tabs[active];
            if move_left || move_right || move_up || move_down {
                // Keyboard-driven movement: back to clamping on real characters only.
                tab.multi_cursor_virtual_space = false;
            }
            if move_left {
                tab.multi_cursor_column = tab.multi_cursor_column.saturating_sub(1);
                tab.multi_cursor_start_column = tab.multi_cursor_column;
            }
            if move_right {
                tab.multi_cursor_column += 1;
                tab.multi_cursor_start_column = tab.multi_cursor_column;
            }
            if move_up && tab.multi_cursor_lines.first().is_some_and(|&l| l > 0) {
                for line in tab.multi_cursor_lines.iter_mut() {
                    *line -= 1;
                }
                tab.multi_cursor_origin_line = tab.multi_cursor_origin_line.saturating_sub(1);
            }
            if move_down
                && tab
                    .multi_cursor_lines
                    .last()
                    .is_some_and(|&l| l + 1 < total_lines)
            {
                for line in tab.multi_cursor_lines.iter_mut() {
                    *line += 1;
                }
                tab.multi_cursor_origin_line += 1;
            }
        }

        // Escape or Enter exits column mode, like Notepad++ (plain arrow keys instead move
        // the block - handled above).
        if self.tabs[active].multi_cursor_active {
            let cancel_key =
                ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter));
            if cancel_key {
                let origin_line = self.tabs[active].multi_cursor_origin_line;
                let column = self.tabs[active].multi_cursor_column;
                let idx = char_index_for_line_col(&text, origin_line + 1, column + 1);
                let tab = &mut self.tabs[active];
                tab.exit_column_mode();
                tab.cursor_char_index = idx;
                set_editor_selection(ui.ctx(), editor_id, idx, idx);
                cursor_char_index = idx;
            }
        }

        // Escape, or a plain (non-ctrl) navigation key, drops the extra Ctrl+click carets
        // and returns to a single caret at the current primary position.
        if !self.tabs[active].extra_carets.is_empty() {
            let cancel_key = ui.input(|i| {
                i.key_pressed(egui::Key::Escape)
                    || ((i.key_pressed(egui::Key::ArrowUp)
                        || i.key_pressed(egui::Key::ArrowDown)
                        || i.key_pressed(egui::Key::ArrowLeft)
                        || i.key_pressed(egui::Key::ArrowRight)
                        || i.key_pressed(egui::Key::Enter))
                        && !i.modifiers.ctrl)
            });
            if cancel_key {
                self.tabs[active].exit_multi_caret_mode();
            }
        }

        // Autocomplete popup from last frame: intercept Tab/Enter/Escape/Up/Down BEFORE
        // the TextEdit below gets them, otherwise it would insert a real Tab/newline first
        // (since those keys are never drained for it otherwise) and "accepting" would be
        // too late - the popup state used here is from the PREVIOUS frame's suggestions,
        // which is fine since word-wise state barely changes between adjacent frames.
        // `autocomplete_just_resolved` suppresses the end-of-frame refresh below so
        // accepting/dismissing doesn't immediately reopen the popup just because the
        // accepted word itself also edited the buffer.
        let mut autocomplete_just_resolved = false;
        if self.autocomplete.active {
            let (accept, dismiss, down, up) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                )
            });
            if down {
                self.autocomplete.selected = (self.autocomplete.selected + 1)
                    .min(self.autocomplete.suggestions.len().saturating_sub(1));
            }
            if up {
                self.autocomplete.selected = self.autocomplete.selected.saturating_sub(1);
            }
            if dismiss {
                self.autocomplete.active = false;
                autocomplete_just_resolved = true;
            } else if accept {
                if let Some(word) = self
                    .autocomplete
                    .suggestions
                    .get(self.autocomplete.selected)
                    .cloned()
                {
                    let prefix_len = self.autocomplete.prefix.chars().count();
                    let start = self
                        .autocomplete
                        .anchor_char_index
                        .saturating_sub(prefix_len);
                    let end = self.autocomplete.anchor_char_index;
                    cursor_char_index = replace_char_range(&mut text, start..end, &word);
                    set_editor_selection(ui.ctx(), editor_id, cursor_char_index, cursor_char_index);
                    text_mutated_this_frame = true;
                }
                self.autocomplete.active = false;
                autocomplete_just_resolved = true;
            }
        }

        // Insert key toggles overwrite ("OVR") mode, shown in the status bar.
        if ui.input(|i| i.key_pressed(egui::Key::Insert)) {
            self.tabs[active].overwrite_mode = !self.tabs[active].overwrite_mode;
        }

        // While column mode is active, mirror typed characters/Backspace/Delete across every
        // active line ourselves (clamped to each line's own length), instead of letting the
        // TextEdit apply them only to a single cursor.
        if self.tabs[active].multi_cursor_active {
            let mut typed_chars: Vec<char> = Vec::new();
            let mut backspace = false;
            let mut delete = false;
            ui.input_mut(|i| {
                i.events.retain(|event| match event {
                    egui::Event::Text(s) => {
                        typed_chars.extend(s.chars());
                        false
                    }
                    egui::Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        ..
                    } => {
                        backspace = true;
                        false
                    }
                    egui::Event::Key {
                        key: egui::Key::Delete,
                        pressed: true,
                        ..
                    } => {
                        delete = true;
                        false
                    }
                    _ => true,
                });
            });

            if !typed_chars.is_empty() || backspace || delete {
                let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
                let cursor_lines = self.tabs[active].multi_cursor_lines.clone();
                let mut column = self.tabs[active].multi_cursor_column;
                let start_column = self.tabs[active].multi_cursor_start_column;
                let virtual_space = self.tabs[active].multi_cursor_virtual_space;

                let (sel_lo, sel_hi) = if start_column <= column {
                    (start_column, column)
                } else {
                    (column, start_column)
                };
                let collapsed_range = sel_lo != sel_hi;
                if collapsed_range {
                    for &line_idx in &cursor_lines {
                        if let Some(line) = lines.get_mut(line_idx) {
                            let line_chars = line.chars().count();
                            let lo = sel_lo.min(line_chars);
                            let hi = sel_hi.min(line_chars);
                            if lo < hi {
                                let byte_start = byte_index_from_char_index(line, lo);
                                let byte_end = byte_index_from_char_index(line, hi);
                                line.replace_range(byte_start..byte_end, "");
                            }
                        }
                    }
                    column = sel_lo;
                }

                for c in typed_chars {
                    for &line_idx in &cursor_lines {
                        if let Some(line) = lines.get_mut(line_idx) {
                            let line_chars = line.chars().count();
                            // Notepad++ virtual space (mouse-driven column only): pad
                            // short/empty lines with spaces up to the target column before
                            // inserting, so the typed text lands in the same visual column
                            // on every active line. Keyboard-driven column mode clamps to
                            // each line's real end instead, like before.
                            let col = if virtual_space {
                                column
                            } else {
                                column.min(line_chars)
                            };
                            if col > line_chars {
                                line.push_str(&" ".repeat(col - line_chars));
                            }
                            let byte_pos = byte_index_from_char_index(line, col);
                            line.insert(byte_pos, c);
                        }
                    }
                    column += 1;
                }

                if backspace && !collapsed_range {
                    for &line_idx in &cursor_lines {
                        if let Some(line) = lines.get_mut(line_idx) {
                            let line_chars = line.chars().count();
                            // Only delete a real character when the (possibly clamped)
                            // caret is within the line's actual text; in virtual space
                            // there is nothing there to delete, so just let the caret move
                            // left (via the shared `column -= 1` below) without touching
                            // this line at all.
                            let col = if virtual_space {
                                column
                            } else {
                                column.min(line_chars)
                            };
                            if col > 0 && col <= line_chars {
                                let byte_start = byte_index_from_char_index(line, col - 1);
                                let byte_end = byte_index_from_char_index(line, col);
                                line.replace_range(byte_start..byte_end, "");
                            }
                        }
                    }
                    column = column.saturating_sub(1);
                }

                if delete && !collapsed_range {
                    for &line_idx in &cursor_lines {
                        if let Some(line) = lines.get_mut(line_idx) {
                            let line_chars = line.chars().count();
                            let clamped = line_chars.min(column);
                            if clamped < line_chars {
                                let byte_start = byte_index_from_char_index(line, clamped);
                                let byte_end = byte_index_from_char_index(line, clamped + 1);
                                line.replace_range(byte_start..byte_end, "");
                            }
                        }
                    }
                }

                text = lines.join("\n");
                text_mutated_this_frame = true;
                self.tabs[active].multi_cursor_column = column;
                self.tabs[active].multi_cursor_start_column = column;
            }
        } else if !self.tabs[active].extra_carets.is_empty() {
            // Independent (non-column) multi-caret editing: mirror typed/Backspace/Delete
            // at every caret's own global char position. Processing strictly from the
            // highest char index down to the lowest means each caret's own edit can never
            // shift the still-unprocessed (lower) carets' stored positions, so no further
            // offset bookkeeping is needed between carets.
            let mut typed_chars: Vec<char> = Vec::new();
            let mut backspace = false;
            let mut delete = false;
            ui.input_mut(|i| {
                i.events.retain(|event| match event {
                    egui::Event::Text(s) => {
                        typed_chars.extend(s.chars());
                        false
                    }
                    egui::Event::Key {
                        key: egui::Key::Backspace,
                        pressed: true,
                        ..
                    } => {
                        backspace = true;
                        false
                    }
                    egui::Event::Key {
                        key: egui::Key::Delete,
                        pressed: true,
                        ..
                    } => {
                        delete = true;
                        false
                    }
                    _ => true,
                });
            });

            if !typed_chars.is_empty() || backspace || delete {
                let mut positions: Vec<usize> = self.tabs[active].extra_carets.clone();
                positions.push(self.tabs[active].cursor_char_index);
                positions.sort_unstable_by(|a, b| b.cmp(a));

                for c in typed_chars {
                    for pos in positions.iter_mut() {
                        let byte_pos = byte_index_from_char_index(&text, *pos);
                        text.insert(byte_pos, c);
                        *pos += 1;
                    }
                }
                if backspace {
                    for pos in positions.iter_mut() {
                        if *pos > 0 {
                            let byte_start = byte_index_from_char_index(&text, *pos - 1);
                            let byte_end = byte_index_from_char_index(&text, *pos);
                            text.replace_range(byte_start..byte_end, "");
                            *pos -= 1;
                        }
                    }
                }
                if delete {
                    let total_chars = text.chars().count();
                    for pos in positions.iter_mut() {
                        if *pos < total_chars {
                            let byte_start = byte_index_from_char_index(&text, *pos);
                            let byte_end = byte_index_from_char_index(&text, *pos + 1);
                            text.replace_range(byte_start..byte_end, "");
                        }
                    }
                }

                // The primary caret is whichever entry is now the largest (most recently
                // added caret wins display priority, matching the click order).
                positions.sort_unstable_by(|a, b| b.cmp(a));
                let tab = &mut self.tabs[active];
                tab.cursor_char_index = positions[0];
                tab.extra_carets = positions[1..].to_vec();
                set_editor_selection(
                    ui.ctx(),
                    editor_id,
                    tab.cursor_char_index,
                    tab.cursor_char_index,
                );
                cursor_char_index = tab.cursor_char_index;
                text_mutated_this_frame = true;
            }
        }

        // Pending paste from the toolbar button (the right-click context menu handles its
        // own paste directly below via `actions.paste`).
        if self.request_paste {
            self.request_paste = false;
            actions.paste = true;
        }

        let bookmarks_snapshot = self.tabs[active].bookmarks.clone();
        let fold_starts: Vec<usize> = Vec::new(); // folds disable editing entirely (see above)

        // Scanned once here - after the pre-widget edits above that can change the line
        // count within this very frame (multi-cursor typing, accepting an autocomplete
        // suggestion) - and reused below for both the gutter/margin sizing and the
        // `TextEdit`'s `desired_rows`, instead of two separate O(document length) scans.
        // (Cut/paste/format/select-all are applied further down, *after* this frame's
        // already been laid out with the pre-edit text, same as before this change - they
        // take effect starting next frame.)
        let total_lines = text.lines().count().max(1);
        let row_count = total_lines.max(min_visible_rows);

        // Whether any input event this frame looks like it could edit the text once the
        // widget processes it below - checked *before* `editor.show()` runs (so these
        // events are still unconsumed, and any already consumed above by the multi-
        // cursor/column-mode key handling correctly no longer count) - letting the
        // layouter closure below know, without any guesswork, whether this is safe to
        // treat as a "nothing changed" frame.
        let pending_editing_event = ui.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Cut
                        | egui::Event::Ime(_)
                        | egui::Event::Key {
                            key: egui::Key::Backspace
                                | egui::Key::Delete
                                | egui::Key::Enter
                                | egui::Key::Tab,
                            pressed: true,
                            ..
                        }
                )
            })
        });
        // Safe to reuse the previous frame's laid-out galley wholesale - skipping both the
        // `LayoutJob` rebuild (which copies the *entire* document text) and egui's own
        // internal layout-cache lookup (which hashes that same text) - only when nothing
        // that would've changed it happened: no pre-widget mutation (typing via
        // multi-cursor/extra-caret mode, accepting an autocomplete suggestion, ...), no
        // pending keystroke/paste/IME event left for the widget to apply natively.
        // Deliberately does NOT also require `!freeze_interaction`: being frozen
        // (`interactive(false)`) means the native widget can't process *any* input this
        // frame regardless, so it can't have mutated the text either - the explicit
        // `text_mutated_this_frame`/`pending_editing_event` checks above already cover
        // every real mutation source on their own. Previously requiring it too forced a
        // full, expensive rebuild on the *exact* frame a right-click freezes the widget
        // (right as the context menu is about to open) even though nothing about the text
        // had changed, which is what caused a visible stutter/hitch right when opening it.
        // Notably, the current *selection* isn't part of this at all: it no longer
        // affects the laid-out galley (see the comment on `gutter_selection_range` above),
        // so extending a selection by dragging no longer defeats this cache either. This
        // is what makes just scrolling, idling, or dragging out a selection in a large
        // file cheap: the exact same `Arc<Galley>` from last frame is reused at ~zero cost
        // instead of redoing an O(document length) rebuild for no reason.
        let can_try_reuse_galley = !text_mutated_this_frame && !pending_editing_event;
        let cached_galley = self.tabs[active].cached_galley.clone();
        let cached_galley_text_len = self.tabs[active].cached_galley_text_len;
        let cached_galley_word_wrap = self.tabs[active].cached_galley_word_wrap;
        let cached_galley_wrap_width_bits = self.tabs[active].cached_galley_wrap_width_bits;
        let cached_galley_font_size_bits = self.tabs[active].cached_galley_font_size_bits;
        let cached_galley_pixels_per_point_bits =
            self.tabs[active].cached_galley_pixels_per_point_bits;
        let font_size_bits = self.font_size.to_bits();
        let pixels_per_point_bits = ui.ctx().pixels_per_point().to_bits();
        // Memoizes `bake_persisted_selection_if_needed`'s (potentially expensive, see its
        // doc comment) result across frames.
        let cached_baked_selection = self.tabs[active]
            .cached_baked_selection_base
            .clone()
            .zip(self.tabs[active].cached_baked_selection_range)
            .zip(self.tabs[active].cached_baked_selection_result.clone())
            .map(|((base, range), result)| (base, range, result));

        egui::ScrollArea::both()
            .id_salt(format!("editor_root-{}", tab_id))
            .stick_to_bottom(self.tabs[active].is_log_view && self.tabs[active].log_follow)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let (bookmark_clicked, _fold_clicked) = Self::draw_margin(
                        ui,
                        total_lines,
                        row_height,
                        &bookmarks_snapshot,
                        &fold_starts,
                    );
                    if let Some(line) = bookmark_clicked {
                        let tab = &mut self.tabs[active];
                        if !tab.bookmarks.remove(&line) {
                            tab.bookmarks.insert(line);
                        }
                    }

                    // Reserve the gutter's horizontal space now (so it's laid out to the
                    // left of the editor, as before), but paint the actual digits - and
                    // the current-line highlight - afterwards, once the real text editor's
                    // galley exists: using the exact same row positions as the real text
                    // guarantees perfect alignment no matter the document length or
                    // whether word-wrap splits a line into multiple visual rows. (The old
                    // approach used a *second*, independently-sized TextEdit for the
                    // numbers, which could drift out of sync with the real editor over a
                    // long document, making the last lines' numbers disappear.)
                    let gutter_width =
                        Self::line_number_gutter_width(ui, total_lines, self.font_size);
                    let (gutter_rect, _) = ui.allocate_exact_size(
                        egui::vec2(gutter_width, total_lines as f32 * row_height),
                        egui::Sense::hover(),
                    );
                    ui.separator();

                    // The layouter below only receives `wrap_width` from egui at call
                    // time (derived from the widget's actual rendered width, which we
                    // don't otherwise know outside the closure), so its last-seen value is
                    // captured here to later save alongside the galley cache.
                    let last_wrap_width = std::rc::Rc::new(std::cell::Cell::new(f32::INFINITY));
                    let last_wrap_width_writer = last_wrap_width.clone();

                    // Captures the galley exactly as *our* layouter builds/returns it,
                    // before egui's `TextEdit` gets a chance to mutate a clone of it with
                    // its own native selection-highlight painting (see the doc comment on
                    // the cache-saving code below `editor.show()` for why that distinction
                    // matters).
                    let last_clean_galley: std::rc::Rc<
                        std::cell::Cell<Option<std::sync::Arc<egui::Galley>>>,
                    > = std::rc::Rc::new(std::cell::Cell::new(None));
                    let last_clean_galley_writer = last_clean_galley.clone();

                    // Mirrors `last_clean_galley` above, but for
                    // `bake_persisted_selection_if_needed`'s memoized result (see its doc
                    // comment for why memoizing it matters).
                    #[allow(clippy::type_complexity)]
                    let last_baked_selection: std::rc::Rc<
                        std::cell::Cell<
                            Option<(
                                std::sync::Arc<egui::Galley>,
                                (usize, usize),
                                std::sync::Arc<egui::Galley>,
                            )>,
                        >,
                    > = std::rc::Rc::new(std::cell::Cell::new(None));
                    let last_baked_selection_writer = last_baked_selection.clone();

                    let mut layouter =
                        move |ui: &egui::Ui, text_buffer: &dyn TextBuffer, wrap_width: f32| {
                            let content = text_buffer.as_str();

                            let effective_wrap_width =
                                if word_wrap { wrap_width } else { f32::INFINITY };
                            last_wrap_width_writer.set(effective_wrap_width);
                            if can_try_reuse_galley
                                && let Some(galley) = &cached_galley
                                && cached_galley_text_len == content.len()
                                && cached_galley_word_wrap == word_wrap
                                && cached_galley_wrap_width_bits == effective_wrap_width.to_bits()
                                && cached_galley_font_size_bits == font_size_bits
                                && cached_galley_pixels_per_point_bits == pixels_per_point_bits
                            {
                                last_clean_galley_writer.set(Some(galley.clone()));
                                let (result, new_bake) = Self::bake_persisted_selection_if_needed(
                                    galley.clone(),
                                    ui,
                                    persisted_selection_to_bake,
                                    &cached_baked_selection,
                                );
                                last_baked_selection_writer.set(new_bake);
                                return result;
                            }

                            // Syntax highlighting (via syntect) tokenizes the *entire*
                            // document, and this layouter runs every repaint - not just on
                            // edits - including ones triggered merely by the blinking
                            // caret or dragging out a selection. On a large file that
                            // makes every frame redo an expensive full-document tokenize
                            // (plus clone a `LayoutJob` with one section per token), which
                            // is what made big files stutter/freeze, especially once a
                            // selection existed (e.g. right after double-clicking a word)
                            // since that also re-splices the selection-background overlay
                            // every frame. Past the threshold, fall back to a single plain
                            // section - still correctly wrapped/line-broken, just without
                            // per-token coloring - which keeps every frame's cost roughly
                            // constant regardless of how large the file is.
                            let mut job = if content.len() > Self::SYNTAX_HIGHLIGHT_MAX_BYTES {
                                let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                                egui::text::LayoutJob::simple(
                                    content.to_owned(),
                                    font_id,
                                    ui.visuals().text_color(),
                                    f32::INFINITY,
                                )
                            } else {
                                highlight(
                                    ui.ctx(),
                                    ui.style(),
                                    &code_theme,
                                    content,
                                    language.syntect_name(),
                                )
                            };
                            job.wrap.max_width = effective_wrap_width;
                            let fresh_galley = ui.fonts_mut(|f| f.layout_job(job));
                            last_clean_galley_writer.set(Some(fresh_galley.clone()));
                            let (result, new_bake) = Self::bake_persisted_selection_if_needed(
                                fresh_galley,
                                ui,
                                persisted_selection_to_bake,
                                &cached_baked_selection,
                            );
                            last_baked_selection_writer.set(new_bake);
                            result
                        };

                    let editor = TextEdit::multiline(&mut text)
                        .id(editor_id)
                        .desired_width(if word_wrap { f32::INFINITY } else { 4000.0 })
                        .desired_rows(row_count)
                        .font(egui::TextStyle::Monospace)
                        .lock_focus(true)
                        .interactive(!freeze_interaction)
                        // egui's default `TextEdit` frame draws a border that changes
                        // color/style based on focus state each frame (bright
                        // `selection.stroke` while focused, dimmer otherwise) - most code
                        // editors (including Notepad++) don't wrap the whole text area in
                        // a decorative border like this, and since our focus/interactive
                        // state legitimately changes often (right-click menu, dialogs,
                        // column mode, ...), it was very visibly flashing on each of those.
                        .frame(false)
                        .layouter(&mut layouter);

                    let select_all_shortcut = !freeze_interaction
                        && ui.memory(|m| m.has_focus(editor_id))
                        && ui.input_mut(|i| {
                            i.consume_shortcut(&egui::KeyboardShortcut::new(
                                egui::Modifiers::CTRL,
                                egui::Key::A,
                            ))
                        });

                    let output = editor.show(ui);
                    self.last_editor_rect = Some(output.response.rect);
                    // `changed()` reliably reflects the widget's own native edits this
                    // frame (typing, backspace, IME, native Ctrl+V/X) - covers the
                    // mutation paths that happen *inside* `show()` itself, which the
                    // pre-widget flag-setting above can't see.
                    text_mutated_this_frame = text_mutated_this_frame || output.response.changed();

                    // Save the galley + the exact inputs it was laid out from, so a later
                    // frame where nothing relevant changed (see `can_try_reuse_galley`
                    // above) can reuse it directly instead of redoing an O(document
                    // length) rebuild. Updated unconditionally (whether this frame reused
                    // a previous cache entry or freshly rebuilt) so it always reflects the
                    // current, correct state.
                    //
                    // Deliberately saves `last_clean_galley` (captured straight from our
                    // layouter above), NOT `output.galley`: when there's an active
                    // selection, `TextEdit` paints its own native selection highlight by
                    // mutating a *clone* of whatever galley the layouter returned (via
                    // `Arc::make_mut`, egui's `paint_text_selection`) - and that mutation
                    // is purely additive, never clearing old highlighting on a later frame
                    // with no/a different selection. Caching `output.galley` would permanently
                    // bake in whatever selection happened to exist on the frame it was
                    // cached, since our own cache-hit path (above) doesn't go through that
                    // repaint step again - which was exactly the bug where the dark native
                    // selection highlight never cleared on later frames.
                    {
                        let tab = &mut self.tabs[active];
                        tab.cached_galley = last_clean_galley
                            .take()
                            .or_else(|| Some(output.galley.clone()));
                        tab.cached_galley_text_len = text.len();
                        tab.cached_galley_word_wrap = word_wrap;
                        tab.cached_galley_wrap_width_bits = last_wrap_width.get().to_bits();
                        tab.cached_galley_font_size_bits = font_size_bits;
                        tab.cached_galley_pixels_per_point_bits = pixels_per_point_bits;
                        match last_baked_selection.take() {
                            Some((base, range, result)) => {
                                tab.cached_baked_selection_base = Some(base);
                                tab.cached_baked_selection_range = Some(range);
                                tab.cached_baked_selection_result = Some(result);
                            }
                            None => {
                                tab.cached_baked_selection_base = None;
                                tab.cached_baked_selection_range = None;
                                tab.cached_baked_selection_result = None;
                            }
                        }
                    }

                    if let Some(cursor_range) = output.cursor_range {
                        cursor_char_index = cursor_range.primary.index;
                    }

                    // Prefer this exact frame's live selection (from `output.cursor_range`)
                    // over the previous-frame snapshot used for the text-background overlay
                    // above, so the gutter's multi-line highlight tracks a mouse-drag
                    // selection without a one-frame lag; fall back to the snapshot only
                    // when egui didn't report a live range (e.g. focus elsewhere).
                    let live_selection_range = output.cursor_range.map(|cursor_range| {
                        let range = cursor_range.as_sorted_char_range();
                        range.start..range.end
                    });
                    let gutter_selection_range =
                        live_selection_range.or_else(|| gutter_selection_range.clone());

                    Self::paint_line_numbers_and_current_line(
                        ui,
                        gutter_rect,
                        &output,
                        cursor_char_index,
                        gutter_selection_range.clone(),
                        self.font_size,
                    );

                    // Real screen position of the caret, so the autocomplete popup can anchor
                    // to it instead of a fixed offset from the editor's corner.
                    // `pos_from_cursor` safely clamps an out-of-range index to the end of
                    // the galley on its own (see the comment on the current-line highlight
                    // above), so no `text.chars().count()` pre-clamp is needed here either.
                    {
                        let caret_rect = output
                            .galley
                            .pos_from_cursor(CCursor::new(cursor_char_index));
                        self.autocomplete_screen_pos =
                            Some(output.galley_pos + caret_rect.left_bottom().to_vec2());
                    }

                    // Translate the mouse position into a line/column via the laid-out galley,
                    // then build the column *box selection* between the fixed anchor (the caret
                    // position from just before this gesture started) and wherever the pointer
                    // currently is (Notepad++'s Alt+Shift+click box-selection behavior).
                    if column_drag_starting || column_drag_continuing {
                        if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                            let rel = pos - output.galley_pos;
                            let char_index = output.galley.cursor_from_pos(rel).index;
                            let (line, col) = line_col_for_char_index(&text, char_index);
                            let line0 = line.saturating_sub(1);
                            let col0 = col.saturating_sub(1);

                            let tab = &mut self.tabs[active];
                            let (anchor_line, anchor_col) = if column_drag_starting {
                                let (a_line, a_col) =
                                    line_col_for_char_index(&text, tab.cursor_char_index);
                                let anchor = (a_line.saturating_sub(1), a_col.saturating_sub(1));
                                tab.multi_cursor_drag_anchor = Some(anchor);
                                anchor
                            } else {
                                tab.multi_cursor_drag_anchor.unwrap_or((line0, col0))
                            };
                            let (lo, hi) = if anchor_line <= line0 {
                                (anchor_line, line0)
                            } else {
                                (line0, anchor_line)
                            };
                            tab.multi_cursor_lines = (lo..=hi).collect();
                            tab.multi_cursor_start_column = anchor_col;
                            tab.multi_cursor_column = col0;
                            tab.multi_cursor_origin_line = anchor_line;
                            tab.multi_cursor_active = true;
                            // Mouse-driven: allow placing/typing past short lines' real end.
                            tab.multi_cursor_virtual_space = true;
                        }
                    }
                    if self.tabs[active].multi_cursor_drag_anchor.is_some()
                        && ui.input(|i| i.pointer.primary_released())
                    {
                        self.tabs[active].multi_cursor_drag_anchor = None;
                    }

                    // Double-click "select word", computed ourselves via the cheap,
                    // bounded `word_range_at_char_index` instead of egui's own (expensive
                    // on large files) built-in handling - see `double_click_starting`'s
                    // doc comment above for why.
                    if double_click_starting
                        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                    {
                        let rel = pos - output.galley_pos;
                        let clicked_index = output.galley.cursor_from_pos(rel).index;
                        if let Some((start, end)) = word_range_at_char_index(&text, clicked_index) {
                            set_editor_selection(ui.ctx(), editor_id, start, end);
                            cursor_char_index = end;
                            selection_char_range = Some((start, end));
                        } else {
                            set_editor_selection(ui.ctx(), editor_id, clicked_index, clicked_index);
                            cursor_char_index = clicked_index;
                            selection_char_range = None;
                        }
                    }

                    // Ctrl+click adds an independent extra caret at the clicked position
                    // (Notepad++ multi-editing), seeded with whatever the single caret
                    // position was right before this click.
                    if ctrl_click_starting {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            let rel = pos - output.galley_pos;
                            let clicked_index = output.galley.cursor_from_pos(rel).index;
                            let tab = &mut self.tabs[active];
                            let previous_primary = tab.cursor_char_index;
                            if clicked_index != previous_primary
                                && !tab.extra_carets.contains(&clicked_index)
                            {
                                tab.extra_carets.push(previous_primary);
                                tab.cursor_char_index = clicked_index;
                                cursor_char_index = clicked_index;
                                set_editor_selection(
                                    ui.ctx(),
                                    editor_id,
                                    clicked_index,
                                    clicked_index,
                                );
                            }
                        }
                    }
                    if !self.tabs[active].extra_carets.is_empty()
                        && !ctrl_click_starting
                        && output.response.clicked_by(egui::PointerButton::Primary)
                    {
                        self.tabs[active].exit_multi_caret_mode();
                    }

                    if select_all_shortcut {
                        let end = text.chars().count();
                        set_editor_selection(ui.ctx(), editor_id, 0, end);
                        cursor_char_index = 0;
                        selection_char_range = Some((0, end));
                    }

                    // `output.cursor_range` is only `Some` when the widget was actually
                    // interactive and focused *this* frame (egui's own rule) - i.e. when it
                    // just processed real keyboard/mouse events, including native
                    // Ctrl+X/Delete/typing-over-a-selection, which collapse the selection to
                    // an empty cursor range without any "primary click" occurring. Trust that
                    // whenever we get it, even when empty, so a native cut/delete properly
                    // clears the stale selection instead of leaving it to be painted over
                    // whatever text slides into the vacated spot. When it's `None` (context
                    // menu open, or focus moved to a dialog/button), fall back to preserving
                    // the previous selection so highlighting doesn't vanish just because
                    // focus moved elsewhere - unless a primary click (which egui still senses
                    // even without focus) explicitly collapsed it.
                    match output.cursor_range {
                        Some(cursor_range) => {
                            let range = cursor_range.as_sorted_char_range();
                            selection_char_range = if range.is_empty() {
                                None
                            } else {
                                Some((range.start, range.end))
                            };
                        }
                        None if output.response.clicked_by(egui::PointerButton::Primary) => {
                            selection_char_range = None;
                        }
                        None => {}
                    }

                    if self.tabs[active].multi_cursor_active
                        && output.response.clicked_by(egui::PointerButton::Primary)
                    {
                        self.tabs[active].exit_column_mode();
                    }

                    if self.tabs[active].multi_cursor_active {
                        let painter = ui.painter();
                        let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 170, 0));
                        let fill = egui::Color32::from_rgba_unmultiplied(255, 170, 0, 70);
                        let column = self.tabs[active].multi_cursor_column;
                        let start_column = self.tabs[active].multi_cursor_start_column;
                        let (sel_lo, sel_hi) = if start_column <= column {
                            (start_column, column)
                        } else {
                            (column, start_column)
                        };
                        let virtual_space = self.tabs[active].multi_cursor_virtual_space;
                        let lines_for_rendering: Vec<&str> = text.split('\n').collect();
                        // Notepad++ allows the column caret/selection to sit past the end of a
                        // short (or empty) line in "virtual space" - but only while the column
                        // came from the Alt+Shift+Click/drag mouse gesture; keyboard-driven
                        // movement clamps to each line's real end like before.
                        let font_id = egui::FontId::monospace(self.font_size);
                        let char_width = ui.fonts_mut(|f| f.glyph_width(&font_id, ' ')).max(1.0);
                        let caret_rect_for = |line_idx: usize, target_col: usize| -> egui::Rect {
                            let line_chars = lines_for_rendering
                                .get(line_idx)
                                .map_or(0, |l| l.chars().count());
                            let col = if virtual_space {
                                target_col
                            } else {
                                target_col.min(line_chars)
                            };
                            if col <= line_chars {
                                let idx = char_index_for_line_col(&text, line_idx + 1, col + 1);
                                output.galley.pos_from_cursor(CCursor::new(idx))
                            } else {
                                let end_idx =
                                    char_index_for_line_col(&text, line_idx + 1, line_chars + 1);
                                let mut rect = output.galley.pos_from_cursor(CCursor::new(end_idx));
                                let extra = (col - line_chars) as f32 * char_width;
                                rect.min.x += extra;
                                rect.max.x += extra;
                                rect
                            }
                        };
                        for &line_idx in &self.tabs[active].multi_cursor_lines {
                            if sel_lo != sel_hi {
                                let start_rect = caret_rect_for(line_idx, sel_lo);
                                let end_rect = caret_rect_for(line_idx, sel_hi);
                                let top_left = output.galley_pos + start_rect.min.to_vec2();
                                let bottom_right = output.galley_pos
                                    + egui::vec2(end_rect.max.x, start_rect.max.y);
                                painter.rect_filled(
                                    egui::Rect::from_min_max(top_left, bottom_right),
                                    0.0,
                                    fill,
                                );
                            } else {
                                let rect = caret_rect_for(line_idx, column);
                                let top = output.galley_pos + rect.min.to_vec2();
                                let bottom = output.galley_pos + rect.max.to_vec2();
                                painter.line_segment([top, bottom], stroke);
                            }
                        }
                    }

                    // Draw a caret line for every independent extra Ctrl+click caret (the
                    // primary caret is already painted by the TextEdit itself).
                    if !self.tabs[active].extra_carets.is_empty() {
                        let painter = ui.painter();
                        let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(90, 170, 255));
                        for &idx in &self.tabs[active].extra_carets {
                            let rect = output.galley.pos_from_cursor(CCursor::new(idx));
                            let top = output.galley_pos + rect.min.to_vec2();
                            let bottom = output.galley_pos + rect.max.to_vec2();
                            painter.line_segment([top, bottom], stroke);
                        }
                    }

                    let secondary_click_pos = ui.input(|i| {
                        if i.pointer.secondary_clicked() {
                            i.pointer.interact_pos()
                        } else {
                            None
                        }
                    });
                    if !menu_open_here
                        && let Some(pos) = secondary_click_pos
                        && output.response.rect.contains(pos)
                    {
                        self.editor_context_menu_open = true;
                        self.editor_context_menu_tab = active;
                        self.editor_context_menu_pos = pos;
                    }
                });
            });

        let effective_selection = selection_char_range.or(previous_selection);
        let has_selection = effective_selection.is_some();
        self.draw_editor_context_menu(ui.ctx(), active, has_selection, &mut actions);

        let selected_chars = effective_selection
            .map(|(start, end)| if start <= end { start..end } else { end..start })
            .unwrap_or(cursor_char_index..cursor_char_index);

        if actions.copy && !selected_chars.is_empty() {
            let selected_text = slice_char_range(&text, selected_chars.clone());
            ui.ctx().copy_text(selected_text);
            set_editor_selection(
                ui.ctx(),
                editor_id,
                selected_chars.start,
                selected_chars.end,
            );
            selection_char_range = Some((selected_chars.start, selected_chars.end));
            self.status_message = "Copied".to_string();
        }

        if actions.cut && !selected_chars.is_empty() {
            let selected_text = slice_char_range(&text, selected_chars.clone());
            ui.ctx().copy_text(selected_text);
            cursor_char_index = replace_char_range(&mut text, selected_chars.clone(), "");
            set_editor_selection(ui.ctx(), editor_id, cursor_char_index, cursor_char_index);
            selection_char_range = None;
            text_mutated_this_frame = true;
            self.status_message = "Cut".to_string();
        }

        if actions.delete && !selected_chars.is_empty() {
            cursor_char_index = replace_char_range(&mut text, selected_chars.clone(), "");
            set_editor_selection(ui.ctx(), editor_id, cursor_char_index, cursor_char_index);
            selection_char_range = None;
            text_mutated_this_frame = true;
            self.status_message = "Deleted selection".to_string();
        }

        if actions.paste {
            match read_clipboard_text() {
                Some(clipboard_text) => {
                    let replace_range = if selected_chars.is_empty() {
                        cursor_char_index..cursor_char_index
                    } else {
                        selected_chars.clone()
                    };
                    cursor_char_index =
                        replace_char_range(&mut text, replace_range, &clipboard_text);
                    set_editor_selection(ui.ctx(), editor_id, cursor_char_index, cursor_char_index);
                    selection_char_range = None;
                    text_mutated_this_frame = true;
                    self.status_message = "Pasted".to_string();
                }
                None => self.status_message = "Clipboard is empty or unavailable".to_string(),
            }
        }

        if actions.select_all {
            let end = text.chars().count();
            cursor_char_index = 0;
            set_editor_selection(ui.ctx(), editor_id, 0, end);
            selection_char_range = Some((0, end));
            self.status_message = "Selected all".to_string();
        }

        if actions.base64_encode
            || actions.base64_decode
            || self.pending_base64_encode
            || self.pending_base64_decode
        {
            // Unlike the old instant in-place replace, this now opens an editable
            // Base64 dialog (like the JWS tools): the selection (or whole doc) just
            // prefills the input box and is immediately encoded/decoded for familiar
            // feedback, but nothing in the document changes until "Insert into Editor".
            let want_encode = actions.base64_encode || self.pending_base64_encode;
            self.pending_base64_encode = false;
            self.pending_base64_decode = false;

            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            self.base64_target_tab = active;
            self.base64_target_range = (target_range.start, target_range.end);
            self.base64_input = slice_char_range(&text, target_range);
            self.base64_output = Some(if want_encode {
                Ok(base64::engine::general_purpose::STANDARD.encode(self.base64_input.as_bytes()))
            } else {
                base64::engine::general_purpose::STANDARD
                    .decode(self.base64_input.trim())
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .map_err(|e| format!("Not valid Base64: {e}"))
            });
            self.show_base64_dialog = true;
        }

        if let Some(algo) = actions.request_hash.or(self.pending_hash) {
            // Snapshot the selection (or whole doc, like Base64 above) now as a starting
            // point for the (freely editable) input box; the key dialog is drawn on a
            // later frame after the user types a key and clicks Compute.
            self.pending_hash = None;
            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            self.hash_dialog_algo = Some(algo);
            self.hash_dialog_tab = active;
            self.hash_dialog_range = (target_range.start, target_range.end);
            self.hash_input = slice_char_range(&text, target_range);
            self.hash_key_input.clear();
            self.hash_output = None;
            self.show_hash_dialog = true;
        }

        if actions.request_bcrypt_hash || self.pending_bcrypt_hash {
            // Snapshot the selection (or whole doc) now as a starting point for the
            // (freely editable) plaintext box; the cost dialog is drawn on a later frame
            // once the user confirms the cost factor.
            self.pending_bcrypt_hash = false;
            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            self.bcrypt_hash_tab = active;
            self.bcrypt_hash_range = (target_range.start, target_range.end);
            self.bcrypt_hash_input = slice_char_range(&text, target_range);
            self.bcrypt_cost_input = crate::hashing::DEFAULT_BCRYPT_COST.to_string();
            self.bcrypt_hash_error = None;
            self.bcrypt_hash_output = None;
            self.show_bcrypt_hash_dialog = true;
        }

        if actions.request_bcrypt_verify || self.pending_bcrypt_verify {
            // The selection (or whole doc) prefills the stored bcrypt hash to check
            // against (freely editable, so you can paste a hash from elsewhere too); the
            // candidate password is typed into the dialog separately.
            self.pending_bcrypt_verify = false;
            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            self.bcrypt_verify_source = slice_char_range(&text, target_range).trim().to_string();
            self.bcrypt_verify_candidate.clear();
            self.bcrypt_verify_result = None;
            self.show_bcrypt_verify_dialog = true;
        }

        if actions.request_jwt_decode || self.pending_jwt_decode {
            // The selection (or whole doc) prefills the raw JWT/JWS to inspect (freely
            // editable - paste a different token and click Decode); decoding happens right
            // away so the dialog shows a result immediately.
            self.pending_jwt_decode = false;
            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            let source = slice_char_range(&text, target_range).trim().to_string();
            self.jwt_decoded = Some(crate::jwt::decode_jwt(&source));
            self.jwt_source = source;
            self.jwt_secret_input.clear();
            self.jwt_verify_result = None;
            self.show_jwt_dialog = true;
        }

        if actions.request_jws_generator || self.pending_jws_generator {
            // Prefill the payload box with the selection (or whole doc, like the other
            // utilities) so you can sign text already in the editor; falls back to a
            // sample claims object (matching the reference site) when there's nothing to
            // prefill with. The key box always starts empty since a key should never be
            // auto-populated from document text.
            self.pending_jws_generator = false;
            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            let source = slice_char_range(&text, target_range).trim().to_string();
            self.jws_payload_input = if source.is_empty() {
                r#"{"sub":"1234567890","name":"John Doe","iat":1516239022}"#.to_string()
            } else {
                source
            };
            self.jws_key_input.clear();
            self.jws_result = None;
            self.show_jws_dialog = true;
        }

        if actions.request_jws_verify || self.pending_jws_verify {
            // Like "Decode JWT...", the selection (or whole doc) prefills the JWS to
            // check (freely editable); the key is typed into the dialog separately, so
            // nothing is verified until the user supplies one and clicks Verify.
            self.pending_jws_verify = false;
            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            self.jws_verify_token = slice_char_range(&text, target_range).trim().to_string();
            self.jws_verify_key_input.clear();
            self.jws_verify_detached_payload.clear();
            self.jws_verify_result = None;
            self.show_jws_verify_dialog = true;
        }

        if actions.request_convert || self.pending_convert {
            // Prefills the input box from the selection (or whole doc) like the other
            // Utilities tools, but the value is only used as a starting point - the mode,
            // unit and format pickers let the user convert it however they like. Doesn't
            // auto-convert on open since a sensible default mode/format can't be guessed.
            self.pending_convert = false;
            let target_range = if selected_chars.is_empty() {
                0..text.chars().count()
            } else {
                selected_chars.clone()
            };
            self.convert_target_tab = active;
            self.convert_target_range = (target_range.start, target_range.end);
            let source = slice_char_range(&text, target_range).trim().to_string();
            if !source.is_empty() {
                self.convert_input = source;
            }
            self.convert_output = None;
            self.show_convert_dialog = true;
        }

        if actions.format_document || self.request_format_document {
            // Unlike Base64/Hash, formatting always applies to the whole document (like
            // Postman's Beautify), so there's no selection-vs-whole-doc branch here.
            self.request_format_document = false;
            let language = self.tabs[active].language;
            match crate::formatter::format_document(language, &text) {
                Ok(formatted) if formatted != text => {
                    text = formatted;
                    set_editor_selection(ui.ctx(), editor_id, 0, 0);
                    selection_char_range = None;
                    cursor_char_index = 0;
                    text_mutated_this_frame = true;
                    self.status_message = format!("Formatted document ({})", language.label());
                }
                Ok(_) => {
                    self.status_message = format!("{} is already formatted", language.label());
                }
                Err(message) => {
                    self.status_message = message;
                }
            }
        }

        if actions.find || actions.replace {
            self.show_find_panel = true;
        }
        if actions.goto {
            self.show_goto_panel = true;
        }
        if actions.toggle_bookmark {
            let (line, _) = line_col_for_char_index(&text, cursor_char_index);
            let line0 = line.saturating_sub(1);
            let tab = &mut self.tabs[active];
            if !tab.bookmarks.remove(&line0) {
                tab.bookmarks.insert(line0);
            }
        }
        if actions.pin_toggle {
            if let Some(tab) = self.tabs.get_mut(active) {
                tab.pinned = !tab.pinned;
                self.status_message = if tab.pinned {
                    "Tab pinned".to_string()
                } else {
                    "Tab unpinned".to_string()
                };
            }
        }

        // Word-based autocomplete: refresh suggestions for next frame from the now-final
        // cursor position (accept/dismiss/navigate keys were already handled pre-show).
        // - An actual edit this frame (typing/backspace/paste/cut/format/inserted
        //   results/...) recomputes suggestions from the new prefix, showing/updating the
        //   popup as needed.
        // - The caret moving WITHOUT an edit (clicking into a word, arrow-key navigation,
        //   switching tabs, ...) dismisses it - this is what keeps the popup from popping
        //   up just because the caret happens to land inside a word.
        // - Otherwise (an idle frame - nothing moved or changed, e.g. while the user is
        //   just looking at the open popup or hovering a suggestion with the mouse) leaves
        //   `autocomplete.active` untouched, so it doesn't vanish one frame after showing.
        let text_changed_this_frame = !autocomplete_just_resolved && text_mutated_this_frame;
        let cursor_moved_without_edit = !autocomplete_just_resolved
            && !text_changed_this_frame
            && cursor_char_index != previous_cursor_char_index;
        if text_changed_this_frame {
            self.refresh_autocomplete_suggestions(&text, cursor_char_index, language);
        } else if cursor_moved_without_edit {
            self.autocomplete.active = false;
        }

        self.tabs[active].text = text;
        self.tabs[active].cursor_char_index = cursor_char_index;
        self.tabs[active].selection_char_range = selection_char_range;

        // Keep `dirty` and the status bar's cached length/line-count/caret-position in
        // sync explicitly, paying the O(document length) recompute cost only on frames
        // that actually changed something - not on every single frame regardless of
        // activity, which is what made just having a large file open (scrolling,
        // clicking around, idling) noticeably heavier than it needed to be.
        if text_mutated_this_frame {
            self.tabs[active].dirty = true;
            self.tabs[active].recompute_size_caches();
        } else if log_appended_this_frame {
            // New content tailed in from disk isn't an unsaved edit - don't mark dirty,
            // just refresh the caches (length/line count, galley) it invalidates.
            self.tabs[active].recompute_size_caches();
        }
        if text_mutated_this_frame || cursor_char_index != previous_cursor_char_index {
            self.tabs[active].recompute_cursor_cache();
        }

        if actions.close_tab {
            self.close_tab(active);
        }
    }

    /// Read-only projection used whenever the active tab has any fold collapsed: hidden
    /// line ranges are replaced by a single placeholder line so the real buffer is never
    /// touched while folded (see `Document.folds` doc comment).
    fn draw_folded_readonly_view(&mut self, ui: &mut egui::Ui) {
        let active = self.active_tab;
        let text = self.tabs[active].text.clone();
        let folds = self.tabs[active].folds.clone();
        let lines: Vec<&str> = text.split('\n').collect();

        let mut display = String::new();
        let mut i = 0usize;
        while i < lines.len() {
            if let Some(&(start, end)) = folds.iter().find(|(s, _)| *s == i) {
                display.push_str(lines[i]);
                display.push_str(&format!("  ⋯ [{} lines folded] ⋯\n", end - start));
                i = end + 1;
            } else {
                display.push_str(lines[i]);
                display.push('\n');
                i += 1;
            }
        }
        display.pop();

        ui.label("This tab is folded (read-only). Use View > Unfold All to resume editing.");
        egui::ScrollArea::both()
            .id_salt(format!("folded_root-{}", self.tabs[active].id))
            .show(ui, |ui| {
                let mut scratch = display;
                let row_count = scratch.lines().count().max(1);
                ui.add(
                    TextEdit::multiline(&mut scratch)
                        .id_salt(format!("folded-{}", self.tabs[active].id))
                        .font(egui::TextStyle::Monospace)
                        .interactive(false)
                        .desired_width(f32::INFINITY)
                        .desired_rows(row_count),
                );
            });
    }

    /// Recomputes suggestions for NEXT frame from the (already finalized, post-`show()`)
    /// cursor position. Accept/dismiss/navigate keys are handled separately, BEFORE
    /// `editor.show()`, using last frame's state (see the pre-show block above) - doing it
    /// here instead would be too late, since Tab/Enter would already have been inserted as
    /// a literal character by the TextEdit this same frame.
    ///
    /// Only called when this frame actually edited the document (see the call site's doc
    /// comment) - so, unlike before, merely clicking the caret into an existing word (with
    /// no edit) won't pop the menu up.
    fn refresh_autocomplete_suggestions(
        &mut self,
        text: &str,
        cursor_char_index: usize,
        language: crate::language::Language,
    ) {
        match current_word_prefix(text, cursor_char_index) {
            Some(p) if p.len() >= 2 => {
                let words = collect_words(text);
                let list = suggestions(&p, language, &words, 8);
                self.autocomplete.active = !list.is_empty();
                self.autocomplete.prefix = p;
                self.autocomplete.suggestions = list;
                self.autocomplete.selected = self
                    .autocomplete
                    .selected
                    .min(self.autocomplete.suggestions.len().saturating_sub(1));
                self.autocomplete.anchor_char_index = cursor_char_index;
            }
            _ => {
                self.autocomplete.active = false;
            }
        }
    }

    pub fn draw_autocomplete_popup(&mut self, ctx: &egui::Context) {
        if !self.autocomplete.active || self.autocomplete.suggestions.is_empty() {
            return;
        }
        let Some(pos) = self.autocomplete_screen_pos else {
            return;
        };
        let mut accepted_word = None;
        egui::Area::new(Id::new("autocomplete_popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(140.0);
                    for (i, word) in self.autocomplete.suggestions.clone().iter().enumerate() {
                        let selected = i == self.autocomplete.selected;
                        if ui.selectable_label(selected, word).clicked() {
                            accepted_word = Some(word.clone());
                        }
                    }
                });
            });

        if let Some(word) = accepted_word {
            let active = self.active_tab;
            let editor_id = Id::new(format!("editor-{}", self.tabs[active].id));
            let prefix_len = self.autocomplete.prefix.chars().count();
            let anchor = self.autocomplete.anchor_char_index;
            let start = anchor.saturating_sub(prefix_len);
            let text = &mut self.tabs[active].text;
            let new_index = replace_char_range(text, start..anchor, &word);
            self.tabs[active].cursor_char_index = new_index;
            self.tabs[active].selection_char_range = None;
            set_editor_selection(ctx, editor_id, new_index, new_index);
            self.autocomplete.active = false;
        }
    }

    fn draw_editor_context_menu(
        &mut self,
        ctx: &egui::Context,
        active_tab: usize,
        has_selection: bool,
        actions: &mut EditorContextActions,
    ) {
        if !self.editor_context_menu_open || self.editor_context_menu_tab != active_tab {
            return;
        }

        let mut should_close = false;
        let area_response = egui::Area::new(Id::new("editor_context_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(self.editor_context_menu_pos)
            .interactable(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(180.0);

                    if ui
                        .add_enabled(has_selection, egui::Button::new("✂ Cut"))
                        .clicked()
                    {
                        actions.cut = true;
                        should_close = true;
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("📋 Copy"))
                        .clicked()
                    {
                        actions.copy = true;
                        should_close = true;
                    }
                    if ui.button("📥 Paste").clicked() {
                        actions.paste = true;
                        should_close = true;
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("✖ Delete"))
                        .clicked()
                    {
                        actions.delete = true;
                        should_close = true;
                    }

                    ui.separator();
                    if ui.button("☑ Select All").clicked() {
                        actions.select_all = true;
                        should_close = true;
                    }

                    ui.separator();
                    ui.menu_button("⚙ Utilities", |ui| {
                        ui.menu_button("📝 Base64", |ui| {
                            if ui.button("➡ Encode").clicked() {
                                actions.base64_encode = true;
                                should_close = true;
                                ui.close();
                            }
                            if ui.button("⬅ Decode").clicked() {
                                actions.base64_decode = true;
                                should_close = true;
                                ui.close();
                            }
                        });

                        ui.menu_button("# Hash", |ui| {
                            for algo in [
                                crate::hashing::HashAlgorithm::Md5,
                                crate::hashing::HashAlgorithm::Sha1,
                                crate::hashing::HashAlgorithm::Sha256,
                                crate::hashing::HashAlgorithm::Sha384,
                                crate::hashing::HashAlgorithm::Sha512,
                            ] {
                                if ui.button(algo.label()).clicked() {
                                    actions.request_hash = Some(algo);
                                    should_close = true;
                                    ui.close();
                                }
                            }
                        });

                        ui.menu_button("🔒 Bcrypt", |ui| {
                            if ui.button("🔑 Hash...").clicked() {
                                actions.request_bcrypt_hash = true;
                                should_close = true;
                                ui.close();
                            }
                            if ui.button("✅ Verify...").clicked() {
                                actions.request_bcrypt_verify = true;
                                should_close = true;
                                ui.close();
                            }
                        });

                        if ui
                            .button("🔓 Decode JWT / JWS...")
                            .on_hover_text("Parse a JWT/JWS into its header, payload, registered claims, and signature")
                            .clicked()
                        {
                            actions.request_jwt_decode = true;
                            should_close = true;
                            ui.close();
                        }
                        if ui
                            .button("✍ JWS Generator...")
                            .on_hover_text("Sign a payload into a compact JWS/JWT using HMAC, RSA, RSA-PSS, or ECDSA")
                            .clicked()
                        {
                            actions.request_jws_generator = true;
                            should_close = true;
                            ui.close();
                        }
                        if ui
                            .button("✅ Verify JWS...")
                            .on_hover_text("Check a JWS/JWT's signature against a secret or public key")
                            .clicked()
                        {
                            actions.request_jws_verify = true;
                            should_close = true;
                            ui.close();
                        }
                        if ui
                            .button("🕒 Convert...")
                            .on_hover_text("Convert between Unix timestamps, date/time values, and custom formatted strings")
                            .clicked()
                        {
                            actions.request_convert = true;
                            should_close = true;
                            ui.close();
                        }

                        ui.separator();
                        if ui
                            .button("📐 Format Document\tCtrl+Alt+L")
                            .on_hover_text("Beautify JSON/XML/HTML/CSS, or reindent brace-based code")
                            .clicked()
                        {
                            actions.format_document = true;
                            should_close = true;
                            ui.close();
                        }
                    });

                    ui.separator();
                    if ui.button("🔍 Find").clicked() {
                        actions.find = true;
                        should_close = true;
                    }
                    if ui.button("🔄 Replace").clicked() {
                        actions.replace = true;
                        should_close = true;
                    }
                    if ui.button("🔢 Go To").clicked() {
                        actions.goto = true;
                        should_close = true;
                    }

                    ui.separator();
                    if ui.button("🔖 Toggle Bookmark").clicked() {
                        actions.toggle_bookmark = true;
                        should_close = true;
                    }

                    ui.separator();
                    if ui
                        .button(if self.tabs[active_tab].pinned {
                            "📍 Unpin Tab"
                        } else {
                            "📌 Pin Tab"
                        })
                        .clicked()
                    {
                        actions.pin_toggle = true;
                        should_close = true;
                    }
                    if ui.button("✖ Close Tab").clicked() {
                        actions.close_tab = true;
                        should_close = true;
                    }
                });
            });

        let close_by_escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let close_by_outside_click = ctx
            .input(|i| {
                if i.pointer.primary_clicked() || i.pointer.secondary_clicked() {
                    i.pointer.interact_pos()
                } else {
                    None
                }
            })
            .is_some_and(|pos| !area_response.response.rect.contains(pos));

        if should_close || close_by_escape || close_by_outside_click {
            self.editor_context_menu_open = false;
        }
    }
}
