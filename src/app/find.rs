//! Search every file in the reader's folder.
//!
//! There is no index to keep in sync with: the query is a background scan over the files
//! the index already knows, using the ripgrep engine. The sheet is a result list, not a
//! second reading mode: choosing a hit puts the reader on that line and gets out of the
//! way.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    vec2, Align2, Color32, CornerRadius, Frame, Margin, Pos2, Rect, RichText, ScrollArea, Sense,
    Stroke, Ui,
};
use zenith_core::find as engine;
use zenith_core::find::Hit;

use super::widgets::*;
use super::*;

const DEBOUNCE: Duration = Duration::from_millis(90);
const SHEET_MAX: f32 = 780.0;

#[derive(Default)]
pub(super) struct FindState {
    pub(super) open: bool,
    /// What the field contains right now.
    pub(super) query: String,
    /// The query whose results are on screen or arriving.
    seen: String,
    hits: Vec<Hit>,
    selected: usize,
    status: Status,
    rx: Option<Receiver<Msg>>,
    cancel: Option<Arc<AtomicBool>>,
    due: Option<Instant>,
    opened_frame: usize,
    truncated: bool,
}

enum Msg {
    Hit(Hit),
    Done(Result<engine::Outcome, String>),
}

#[derive(Default)]
enum Status {
    #[default]
    Idle,
    Waiting,
    Searching,
    Done,
    Failed(String),
}

impl FindState {
    fn open(&mut self, frame: usize) {
        self.cancel_search();
        self.open = true;
        self.query.clear();
        self.seen.clear();
        self.hits.clear();
        self.selected = 0;
        self.status = Status::Idle;
        self.truncated = false;
        self.due = None;
        self.opened_frame = frame;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.due = None;
        self.cancel_search();
    }

    fn cancel_search(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.rx = None;
    }

    /// Change the query, with a short pause so a fast typist does not start a scan per
    /// key. Old results are cleared immediately: they would be about a different query.
    pub(super) fn set_query(&mut self, query: String) {
        self.query = query;
    }

    fn tick(&mut self, folder: Option<&Folder>, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        if self.query != self.seen {
            self.seen = self.query.clone();
            self.cancel_search();
            self.hits.clear();
            self.selected = 0;
            self.truncated = false;
            if self.query.trim().is_empty() || folder.is_none() {
                self.status = Status::Idle;
                self.due = None;
            } else {
                self.status = Status::Waiting;
                self.due = Some(Instant::now() + DEBOUNCE);
            }
        }
        if matches!(self.status, Status::Idle)
            && !self.query.trim().is_empty()
            && folder.is_some()
            && self.due.is_none()
        {
            // The folder arrived after the sheet was opened.
            self.due = Some(Instant::now());
        }
        if self.due.is_some_and(|due| Instant::now() >= due) {
            self.due = None;
            if let Some(folder) = folder {
                if !self.query.trim().is_empty() {
                    self.start(folder);
                }
            }
        }
        self.drain();
        if matches!(self.status, Status::Waiting | Status::Searching) {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
    }

    fn start(&mut self, folder: &Folder) {
        self.hits.clear();
        self.selected = 0;
        self.truncated = false;
        self.status = Status::Searching;
        let files = folder.files.clone();
        let query = self.query.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        self.cancel = Some(cancel.clone());
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let hit_tx = tx.clone();
            let mut send = |hit| {
                let _ = hit_tx.send(Msg::Hit(hit));
            };
            let result = engine::search_with(&files, &query, &cancel, &mut send);
            let _ = tx.send(Msg::Done(result));
        });
    }

    fn drain(&mut self) {
        loop {
            let received = self.rx.as_ref().map(|rx| rx.try_recv());
            match received {
                None => break,
                Some(Ok(Msg::Hit(hit))) => {
                    self.hits.push(hit);
                    if self.selected >= self.hits.len() {
                        self.selected = self.hits.len().saturating_sub(1);
                    }
                }
                Some(Ok(Msg::Done(result))) => {
                    self.rx = None;
                    match result {
                        Ok(outcome) => {
                            self.hits = outcome.hits;
                            self.truncated = outcome.truncated;
                            self.selected = self.selected.min(self.hits.len().saturating_sub(1));
                            self.status = Status::Done;
                        }
                        Err(e) => self.status = Status::Failed(e),
                    }
                    break;
                }
                Some(Err(TryRecvError::Empty)) => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    self.rx = None;
                    if matches!(self.status, Status::Searching) {
                        self.status = Status::Failed("the search stopped".into());
                    }
                    break;
                }
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.hits.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.hits.len() as i32 - 1;
        self.selected = (self.selected as i32 + delta).clamp(0, last) as usize;
    }

    fn selected_hit(&self) -> Option<Hit> {
        self.hits.get(self.selected).cloned()
    }

    fn files_with_hits(&self) -> usize {
        self.hits
            .iter()
            .map(|hit| hit.rel.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }
}

impl Zenith {
    pub(super) fn open_project_search(&mut self) {
        if self.find.open {
            self.find.close();
            return;
        }
        self.find.open(self.frame);
        self.sheets.palette_open = false;
        self.sheets.search_open = false;
        self.sheets.show_settings = false;
        self.session.show_base = false;
        self.show_panels = false;
        self.show_outline = false;
        self.tree.focus = false;
        self.chords.clear();
    }

    fn open_find_hit(&mut self, hit: Hit) {
        self.find.close();
        self.open_file(&hit.path);
        if let Some(open) = &self.open {
            let line = hit
                .line
                .saturating_sub(1)
                .min(open.doc.lines.len().saturating_sub(1));
            self.caret = vim::landing(&open.doc, line);
            self.pending = Pending::Centre(line);
        }
        self.status = format!("{}:{}", hit.rel, hit.line);
    }

    pub(super) fn find_overlay(&mut self, ctx: &egui::Context) {
        self.find.tick(self.folder.as_ref(), ctx);
        if !self.find.open {
            return;
        }
        let pal = self.palette;
        let screen = ctx.content_rect();
        let width = (SHEET_MAX * self.scale)
            .clamp(560.0, 900.0)
            .min((screen.width() - 40.0).max(360.0));
        let high = self.scale > 1.3 || screen.height() <= 700.0;
        let top = self.sheet_top(screen, high, true);
        let pos = Pos2::new(screen.center().x - width * 0.5, top);
        let row_h = (25.0 * self.scale).clamp(25.0, 34.0);
        let head_h = (22.0 * self.scale).clamp(22.0, 28.0);
        // The sheet is compact while it is empty or deciding, then grows to fit the
        // results rather than reserving a blank half-page for them.
        let rows =
            self.find.hits.len() as f32 * row_h + self.find.files_with_hits() as f32 * head_h;
        let room_left = (screen.height() - top - 150.0).max(110.0);
        let body_h = if self.find.hits.is_empty() {
            74.0
        } else {
            (rows + 18.0)
                .clamp(110.0, 540.0 * self.scale)
                .min(room_left)
        };

        let (down, up, enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
            )
        });
        let ctrl_j = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::J));
        let ctrl_k = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::K));
        if esc {
            self.find.close();
            return;
        }
        if down || ctrl_j {
            self.find.move_selection(1);
        }
        if up || ctrl_k {
            self.find.move_selection(-1);
        }

        let mut chosen: Option<Hit> = if enter {
            self.find.selected_hit()
        } else {
            None
        };
        let mut close = false;
        let mut sheet_rect: Option<Rect> = None;

        let sheet = egui::Window::new("find-every-file")
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .collapsible(false)
            .fixed_pos(pos)
            .fixed_size(vec2(width, 0.0))
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.edge))
                    .corner_radius(CornerRadius::same(SHEET_RADIUS))
                    .inner_margin(Margin::symmetric(16, 12))
                    .shadow(sheet_shadow(&pal)),
            )
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.visuals_mut().extreme_bg_color = pal.panel_deep;
                let mut field = None;
                ui.horizontal(|ui| {
                    field = Some(
                        ui.add(
                            egui::TextEdit::singleline(&mut self.find.query)
                                .hint_text("type a pattern")
                                .font(self.typo.ui_id(16.0 * self.scale))
                                .desired_width(width - 112.0)
                                .frame(Frame::NONE),
                        ),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("EVERY FILE")
                                .font(self.typo.ui_id(8.5 * self.scale))
                                .color(pal.ink_soft)
                                .extra_letter_spacing(0.12),
                        );
                    });
                });
                if let Some(field) = field {
                    if !field.has_focus() {
                        field.request_focus();
                    }
                    if field.changed() {
                        self.find.set_query(self.find.query.clone());
                    }
                }
                ui.add_space(7.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(7.0);

                let (label, ink) = self.find_status(pal, self.folder.is_some());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(label)
                            .font(self.typo.ui_id(10.5 * self.scale))
                            .color(ink),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("esc close")
                                .font(self.typo.ui_id(9.5 * self.scale))
                                .color(pal.ink_soft),
                        );
                    });
                });
                ui.add_space(3.0);

                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), body_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().scroll.floating = false;
                        ui.spacing_mut().scroll.foreground_color = true;
                        ui.spacing_mut().scroll.bar_width = 5.0;
                        ScrollArea::vertical()
                            .id_salt("find-every-file-results")
                            .max_height(body_h)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if self.find.hits.is_empty() {
                                    self.find_empty(ui, pal);
                                } else {
                                    let mut last_rel: Option<&str> = None;
                                    for (i, hit) in self.find.hits.iter().enumerate() {
                                        if last_rel != Some(hit.rel.as_str()) {
                                            if last_rel.is_some() {
                                                ui.add_space(5.0);
                                            }
                                            let (rect, _) = ui.allocate_exact_size(
                                                vec2(ui.available_width(), head_h),
                                                Sense::hover(),
                                            );
                                            let path_font = self.typo.code_id(10.0 * self.scale);
                                            let path = truncate(
                                                &hit.rel,
                                                ((rect.width() - 8.0) / (path_font.size * 0.6))
                                                    as usize,
                                            );
                                            ui.painter().text(
                                                Pos2::new(rect.left() + 4.0, rect.center().y),
                                                Align2::LEFT_CENTER,
                                                path,
                                                path_font,
                                                pal.ink_soft,
                                            );
                                            last_rel = Some(hit.rel.as_str());
                                        }
                                        let selected = i == self.find.selected;
                                        let (rect, response) = ui.allocate_exact_size(
                                            vec2(ui.available_width(), row_h),
                                            Sense::click(),
                                        );
                                        if selected || response.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                CornerRadius::same(ROW_RADIUS),
                                                if selected {
                                                    pal.selection
                                                } else {
                                                    theme::alpha(pal.ink, 8)
                                                },
                                            );
                                        }
                                        if selected {
                                            ui.painter().rect_filled(
                                                Rect::from_min_size(
                                                    Pos2::new(rect.left() + 1.0, rect.top() + 5.0),
                                                    vec2(2.0, (row_h - 10.0).max(2.0)),
                                                ),
                                                CornerRadius::same(1),
                                                pal.accent,
                                            );
                                            response.scroll_to_me(Some(egui::Align::Center));
                                        }
                                        self.paint_find_hit(ui, rect, hit, selected);
                                        if response.clicked() {
                                            chosen = Some(hit.clone());
                                        }
                                    }
                                }
                            });
                    },
                );

                ui.add_space(4.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(5.0);
                ui.label(
                    RichText::new("↑↓ choose · enter open · esc close")
                        .font(self.typo.ui_id(9.5 * self.scale))
                        .color(pal.ink_soft),
                );
                sheet_rect = Some(ui.min_rect());
            });

        if sheet_rect.is_none() {
            sheet_rect = sheet.as_ref().map(|s| s.response.rect);
        }
        if self.frame > self.find.opened_frame
            && ctx.input(|i| i.pointer.primary_clicked())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| sheet_rect.is_none_or(|rect| !rect.contains(p)))
        {
            close = true;
        }
        if close {
            self.find.close();
            return;
        }
        if let Some(hit) = chosen.take() {
            self.open_find_hit(hit);
        }
    }

    /// The one line above the results: truth about the query, not decoration.
    fn find_status(&self, pal: Palette, has_folder: bool) -> (String, Color32) {
        if !has_folder {
            return ("open a folder to search every file".into(), pal.ink_soft);
        }
        match &self.find.status {
            Status::Idle if self.find.query.trim().is_empty() => {
                ("regex · smart case".into(), pal.ink_soft)
            }
            Status::Waiting | Status::Searching => ("searching every file…".into(), pal.ink_soft),
            Status::Failed(e) => (e.clone(), pal.accent),
            Status::Done if self.find.hits.is_empty() => (
                format!("nothing matches “{}”", self.find.query.trim()),
                pal.ink_soft,
            ),
            Status::Done => {
                let files = self.find.files_with_hits();
                let note = if self.find.truncated {
                    format!("first {} matches · {} files", self.find.hits.len(), files)
                } else {
                    format!(
                        "{} match{} · {} file{}",
                        self.find.hits.len(),
                        if self.find.hits.len() == 1 { "" } else { "es" },
                        files,
                        if files == 1 { "" } else { "s" },
                    )
                };
                (note, pal.ink_soft)
            }
            _ => ("type a pattern".into(), pal.ink_soft),
        }
    }

    fn find_empty(&self, ui: &mut Ui, pal: Palette) {
        // The status line already carries the outcome. The body only adds something when
        // there is no outcome yet to read.
        let text = if self.find.query.trim().is_empty() {
            Some("the query is matched against every file in the index")
        } else {
            None
        };
        if let Some(text) = text {
            ui.add_space(14.0);
            ui.label(
                RichText::new(text)
                    .font(self.typo.ui_id(11.5 * self.scale))
                    .color(pal.ink_soft),
            );
        }
    }

    /// One result: line number, then the line with the matched bytes in accent.
    fn paint_find_hit(&self, ui: &mut Ui, rect: Rect, hit: &Hit, selected: bool) {
        let pal = self.palette;
        let font = self.typo.code_id(11.5 * self.scale);
        let number_font = self.typo.code_id(9.5 * self.scale);
        ui.painter().text(
            Pos2::new(rect.left() + 30.0, rect.center().y),
            Align2::RIGHT_CENTER,
            hit.line.to_string(),
            number_font,
            pal.ink_faint,
        );
        let raw = hit.text.trim_start();
        let lead = hit.text.len() - raw.len();
        let text = truncate(raw, ((rect.width() - 48.0) / (font.size * 0.6)) as usize);
        let mut start = hit.start.saturating_sub(lead).min(text.len());
        let mut end = hit.end.saturating_sub(lead).min(text.len());
        while start > 0 && !text.is_char_boundary(start) {
            start -= 1;
        }
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }
        if end < start {
            end = start;
        }
        let base = if selected { pal.ink } else { pal.ink_soft };
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = rect.width() - 46.0;
        job.wrap.max_rows = 1;
        if start > 0 {
            job.append(
                &text[..start],
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: base,
                    ..Default::default()
                },
            );
        }
        job.append(
            &text[start..end],
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: pal.accent,
                ..Default::default()
            },
        );
        if end < text.len() {
            job.append(
                &text[end..],
                0.0,
                TextFormat {
                    font_id: font,
                    color: base,
                    ..Default::default()
                },
            );
        }
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            Pos2::new(rect.left() + 44.0, rect.center().y - galley.size().y * 0.5),
            galley,
            base,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn clearing_the_query_clears_the_previous_results() {
        let mut find = FindState {
            open: true,
            query: "palette".into(),
            seen: "palette".into(),
            ..Default::default()
        };
        find.hits.push(Hit {
            path: PathBuf::from("/tmp/project/src/main.rs"),
            rel: "src/main.rs".into(),
            line: 1,
            text: "palette".into(),
            start: 0,
            end: 7,
        });

        find.query.clear();
        find.tick(None, &egui::Context::default());

        assert!(find.hits.is_empty());
        assert!(matches!(find.status, Status::Idle));
    }
}
