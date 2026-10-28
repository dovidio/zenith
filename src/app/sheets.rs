//! The sheets: launcher, find, and settings.
//!
//! Each is a quiet page over the reading page, with its own keyboard. They share the
//! sheet's chrome and the app's fields; the state and the drawing stay together here.

use super::launch::*;
use super::widgets::*;
use super::*;

/// The state of the three summoned surfaces. Each owns its open flag, its query and its
/// keyboard focus, so a sheet can be read (and reset) without touching the page.
#[derive(Default)]
pub(super) struct Sheets {
    pub(super) show_settings: bool,
    pub(super) settings_tab: usize,
    pub(super) settings_rail_focus: bool,
    pub(super) settings_focus: usize,
    pub(super) keys_query: String,
    pub(super) palette_open: bool,
    pub(super) palette_opened_frame: usize,
    pub(super) palette_files_only: bool,
    pub(super) query: String,
    pub(super) palette_sel: usize,
    pub(super) search_open: bool,
    pub(super) search_query: String,
    pub(super) search_seen: String,
    pub(super) search_hits: Vec<vim::Caret>,
    pub(super) search_idx: usize,
}

/// The palette, faces and measure shared by every settings section.
struct SettingsStyle<'a> {
    pal: &'a Palette,
    font: &'a egui::FontId,
    small: &'a egui::FontId,
    values: &'a egui::FontId,
    content: f32,
    single: bool,
    focus: Option<usize>,
}

impl Zenith {
    /// The launcher's rows: matched files, then the curated or ranked commands.
    ///
    /// Kept apart from the sheet so the drawing and the choosing can be read as two
    /// decisions rather than one long branch.
    fn palette_rows(&self, needle: &str) -> (Vec<LaunchRow>, usize) {
        // Matching files. The list is capped, but the reader is told how many were
        // matched; a silent cap makes a file look absent.
        let (matches, matched_total): (Vec<(i32, usize, Vec<usize>)>, usize) = match &self.folder {
            Some(s) => {
                let mut v: Vec<(i32, usize, Vec<usize>)> = s
                    .files
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        fuzzy::match_path(&f.rel, needle).map(|h| (h.score, i, h.positions))
                    })
                    .collect();
                v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                let total = v.len();
                v.truncate(8);
                (v, total)
            }
            None => (Vec::new(), 0),
        };
        let file_rows: Vec<LaunchRow> = match &self.folder {
            Some(s) => matches
                .iter()
                .map(|(score, idx, positions)| {
                    let f = &s.files[*idx];
                    LaunchRow::File {
                        rel: f.rel.clone(),
                        path: f.path.clone(),
                        marked: self.store.is_favorite(&f.path),
                        positions: positions.clone(),
                        score: *score,
                    }
                })
                .collect(),
            None => Vec::new(),
        };

        let mut rows: Vec<LaunchRow> = Vec::new();
        if self.sheets.palette_files_only {
            // `⌘P` keeps the old, narrow promise: files only, no command menu.
            rows.extend(file_rows.iter().take(8).cloned());
        } else if needle.is_empty() {
            // A curated eight: here, the folder, and the page's own switches. The rest
            // is one query away; the launcher is a result list, not a menu.
            let mut here: Vec<LaunchRow> = Vec::new();
            for action in [Launch::Contents, Launch::Search] {
                if !self.launch_available(action) {
                    continue;
                }
                if let Some(spec) = LAUNCH_COMMANDS.iter().find(|s| s.action == action) {
                    here.push(self.launch_row(spec, 0));
                }
            }

            let mut folder: Vec<LaunchRow> = Vec::new();
            if self.launch_available(Launch::Index) {
                if let Some(spec) = LAUNCH_COMMANDS.iter().find(|s| s.action == Launch::Index) {
                    folder.push(self.launch_row(spec, 0));
                }
            }
            if self.open.is_none() || self.folder.is_none() {
                if let Some(spec) = LAUNCH_COMMANDS
                    .iter()
                    .find(|s| s.action == Launch::OpenFolder)
                {
                    folder.push(self.launch_row(spec, 0));
                }
            }
            if self.launch_available(Launch::Review) {
                if let Some(spec) = LAUNCH_COMMANDS.iter().find(|s| s.action == Launch::Review) {
                    folder.push(self.launch_row(spec, 0));
                }
            }
            folder.extend(file_rows.iter().take(2).cloned());

            let mut zenith: Vec<LaunchRow> = Vec::new();
            for action in [
                Launch::FocusLens,
                Launch::SoftWrap,
                Launch::Settings,
                Launch::Keys,
                Launch::LineNumbers,
                Launch::Distilled,
                Launch::NextTheme,
                Launch::Larger,
                Launch::Smaller,
                Launch::Markdown,
                Launch::PrevTheme,
            ] {
                if !self.launch_available(action) {
                    continue;
                }
                if let Some(spec) = LAUNCH_COMMANDS.iter().find(|s| s.action == action) {
                    zenith.push(self.launch_row(spec, 0));
                }
            }

            rows.extend(here);
            rows.extend(folder);
            let room = 8usize.saturating_sub(rows.len());
            rows.extend(zenith.into_iter().take(room));
        } else {
            for spec in LAUNCH_COMMANDS {
                if !self.launch_available(spec.action) {
                    continue;
                }
                if let Some(score) = launch_command_score(spec, needle) {
                    rows.push(self.launch_row(spec, score));
                }
            }
            rows.extend(file_rows.iter().cloned());
            // A query is a ranked list, not a directory: the best match comes first,
            // and a command beats a file on a tie. The contextual groups belong to the
            // empty state, where they help browsing rather than searching.
            rows.sort_by(|a, b| {
                b.score()
                    .cmp(&a.score())
                    .then_with(|| a.is_file().cmp(&b.is_file()))
                    .then_with(|| a.group().rank().cmp(&b.group().rank()))
            });
            rows.truncate(8);
        }
        (rows, matched_total)
    }

    /// Draw one launcher row. Kept out of the scroll closure so the closure is only
    /// about choosing, and the row is only about drawing.
    fn paint_palette_row(
        &self,
        ui: &mut Ui,
        rect: Rect,
        row: &LaunchRow,
        ink: Color32,
        resp: &egui::Response,
    ) {
        let pal = self.palette;
        let command_font = self.typo.ui_id(13.0 * self.scale);
        let detail_font = self.typo.ui_id(11.5 * self.scale);
        let file_font = self.typo.code_id(12.5 * self.scale);
        let file_dir_font = self.typo.code_id(10.5 * self.scale);
        let key_font = self.typo.code_id(10.5 * self.scale);
        match row {
            LaunchRow::Command { spec, detail, .. } => {
                let label = ui.painter().text(
                    Pos2::new(rect.left() + 10.0 * self.scale, rect.center().y),
                    Align2::LEFT_CENTER,
                    spec.label,
                    command_font.clone(),
                    ink,
                );
                if let Some(detail) = detail {
                    let detail_ink = if detail == "on" {
                        pal.accent
                    } else {
                        pal.ink_soft
                    };
                    ui.painter().text(
                        Pos2::new(label.right() + 8.0 * self.scale, rect.center().y),
                        Align2::LEFT_CENTER,
                        detail,
                        detail_font.clone(),
                        detail_ink,
                    );
                }
                if let Some(shortcut) = spec.shortcut {
                    let galley =
                        ui.painter()
                            .layout_no_wrap(shortcut.to_string(), key_font.clone(), ink);
                    let pad = 5.0 * self.scale;
                    let chip_h = (18.0 * self.scale).clamp(18.0, 24.0);
                    let chip_w = galley.size().x + pad * 2.0;
                    let chip = Rect::from_min_size(
                        Pos2::new(
                            rect.right() - 8.0 * self.scale - chip_w,
                            rect.center().y - chip_h * 0.5,
                        ),
                        vec2(chip_w, chip_h),
                    );
                    ui.painter()
                        .rect_filled(chip, CornerRadius::same(3), pal.panel_deep);
                    ui.painter().rect_stroke(
                        chip,
                        CornerRadius::same(3),
                        Stroke::new(1.0, pal.edge),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().galley(
                        Pos2::new(
                            chip.center().x - galley.size().x * 0.5,
                            chip.center().y - galley.size().y * 0.5,
                        ),
                        galley,
                        ink,
                    );
                }
            }
            LaunchRow::File {
                rel,
                marked,
                positions,
                ..
            } => {
                // A path is one line, not a paragraph. Long names and deep
                // folders used to wrap inside a fixed-height row and paint
                // over the row below; the name and its folder now each get
                // one elided line, with the full path on hover.
                let pad = 6.0 * self.scale;
                let right = if *marked {
                    24.0 * self.scale
                } else {
                    10.0 * self.scale
                };
                let max_w = (rect.width() - pad - right).max(20.0);
                let (dir, name, name_start) = match rel.rfind('/') {
                    Some(i) => (&rel[..i], &rel[i + 1..], i + 1),
                    None => ("", rel.as_str(), 0),
                };

                let mut name_job = egui::text::LayoutJob::default();
                name_job.wrap.max_width = max_w;
                name_job.wrap.max_rows = 1;
                name_job.wrap.break_anywhere = true;
                for (ci, ch) in name.chars().enumerate() {
                    let hit = positions.binary_search(&(name_start + ci)).is_ok();
                    let mut f = TextFormat {
                        font_id: file_font.clone(),
                        color: ink,
                        ..Default::default()
                    };
                    if hit {
                        f.underline = Stroke::new(1.0, pal.accent_soft);
                    }
                    name_job.append(&ch.to_string(), 0.0, f);
                }
                let name_galley = ui.painter().layout_job(name_job);

                let dir_galley = (!dir.is_empty()).then(|| {
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap.max_width = max_w;
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    for (ci, ch) in dir.chars().enumerate() {
                        let hit = positions.binary_search(&ci).is_ok();
                        let mut f = TextFormat {
                            font_id: file_dir_font.clone(),
                            color: pal.ink_soft,
                            ..Default::default()
                        };
                        if hit {
                            f.underline = Stroke::new(1.0, pal.accent_soft);
                        }
                        job.append(&ch.to_string(), 0.0, f);
                    }
                    ui.painter().layout_job(job)
                });

                let gap = 1.0 * self.scale;
                let name_h = name_galley.size().y;
                let block_h = name_h + dir_galley.as_ref().map(|g| gap + g.size().y).unwrap_or(0.0);
                let top = rect.center().y - block_h * 0.5;
                let x = rect.left() + pad;
                ui.painter().galley(Pos2::new(x, top), name_galley, ink);
                if let Some(g) = dir_galley {
                    ui.painter()
                        .galley(Pos2::new(x, top + name_h + gap), g, pal.ink_soft);
                }
                if *marked {
                    ui.painter().text(
                        Pos2::new(rect.right() - 8.0 * self.scale, rect.center().y),
                        Align2::RIGHT_CENTER,
                        self.typo.marks.pick("✦"),
                        file_font.clone(),
                        pal.accent,
                    );
                }
                resp.clone().on_hover_text(rel.as_str());
            }
        }
    }

    /// The launcher's keyboard: arrows move, Enter chooses, Escape closes.
    ///
    /// The selection is a field, not a local, so the pointer and the keys agree on the
    /// same row. Returns the chosen row (if any) and whether the sheet should close.
    fn palette_keys(&mut self, ctx: &egui::Context, row_count: usize) -> (Option<usize>, bool) {
        let mut chosen = None;
        let mut close = false;
        let (kd, ku, kenter, kesc) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
            )
        });
        if kesc {
            close = true;
        }
        if kd && row_count > 0 {
            self.sheets.palette_sel = (self.sheets.palette_sel + 1) % row_count;
        }
        if ku && row_count > 0 {
            self.sheets.palette_sel = self
                .sheets
                .palette_sel
                .checked_sub(1)
                .unwrap_or(row_count.saturating_sub(1));
        }
        if kenter && row_count > 0 {
            chosen = Some(self.sheets.palette_sel.min(row_count - 1));
        }
        (chosen, close)
    }

    pub(super) fn palette_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let screen = ctx.content_rect();
        let width = (560.0 * self.scale)
            .clamp(560.0, 820.0)
            .min((screen.width() - 40.0).max(360.0));
        // The launcher sits a third of the way down, like a spotlight, not against the
        // top edge like a toolbar.
        let high = self.scale > 1.3 || screen.height() <= 700.0;
        let top = self.sheet_top(screen, high, true);
        let pos = Pos2::new(screen.center().x - width * 0.5, top);
        let needle = self.sheets.query.trim().to_string();

        let (rows, matched_total) = self.palette_rows(&needle);

        let row_count = rows.len();
        self.sheets.palette_sel = self.sheets.palette_sel.min(row_count.saturating_sub(1));
        let hint = if self.sheets.palette_files_only {
            "find a file in this folder"
        } else {
            "a file, a view, or an action"
        };

        let mut chosen: Option<usize> = None;
        let mut close = false;
        let input_font = self.typo.ui_id(16.0 * self.scale);

        let launcher = egui::Window::new("launcher")
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
                    .inner_margin(Margin::symmetric(12, 10))
                    .shadow(sheet_shadow(&pal)),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().extreme_bg_color = pal.panel_deep;
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.sheets.query)
                        .hint_text(hint)
                        .font(input_font.clone())
                        .desired_width(width - 24.0)
                        .frame(Frame::NONE),
                );
                if !resp.has_focus() {
                    resp.request_focus();
                }
                if resp.changed() {
                    self.sheets.palette_sel = 0;
                }
                ui.add_space(6.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(6.0);

                let (key_choice, key_close) = self.palette_keys(ctx, row_count);
                if key_choice.is_some() {
                    chosen = key_choice;
                }
                if key_close {
                    close = true;
                }

                let row_h = (28.0 * self.scale).clamp(28.0, 46.0);
                let file_row_h = (44.0 * self.scale).clamp(44.0, 64.0);
                let group_count = if self.sheets.palette_files_only || !needle.is_empty() {
                    0
                } else {
                    let mut count = 0;
                    let mut last: Option<LaunchGroup> = None;
                    for row in &rows {
                        if last != Some(row.group()) {
                            count += 1;
                            last = Some(row.group());
                        }
                    }
                    count
                };
                let rows_h: f32 = rows
                    .iter()
                    .map(|row| (if row.is_file() { file_row_h } else { row_h }) + 2.0)
                    .sum();
                let content_h =
                    rows_h + group_count as f32 * (28.0 + 14.0 * self.scale) + 44.0 * self.scale;
                let max_body = (screen.height() - top - 104.0).max(120.0);
                let body_h = content_h.min(max_body);
                // The sheet has to be allowed to be shorter than its rows at 1.8x.
                // Allocating a fixed body first is what lets the scroll area clip and
                // scroll inside it, instead of collapsing the whole sheet to one row.
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), body_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().scroll.floating = false;
                        ui.spacing_mut().scroll.foreground_color = true;
                        ui.spacing_mut().scroll.bar_width = 5.0;
                        ScrollArea::vertical()
                            .id_salt("launcher-results")
                            .max_height(body_h)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let group_font = self.typo.ui_id(9.5 * self.scale);
                                let mut last_group: Option<LaunchGroup> = None;

                                for (i, row) in rows.iter().enumerate() {
                                    if !self.sheets.palette_files_only && needle.is_empty() {
                                        let group = row.group();
                                        if last_group != Some(group) {
                                            ui.add_space(if last_group.is_none() {
                                                2.0
                                            } else {
                                                9.0
                                            });
                                            paint_tracked(
                                                ui.painter(),
                                                Pos2::new(
                                                    ui.min_rect().left() + 10.0,
                                                    ui.cursor().top() + 2.0,
                                                ),
                                                Align2::LEFT_TOP,
                                                group.label(),
                                                group_font.clone(),
                                                pal.ink_soft,
                                                0.10 * 9.5 * self.scale,
                                            );
                                            ui.add_space((14.0 * self.scale).max(14.0));
                                            last_group = Some(group);
                                        }
                                    }

                                    let selected = i == self.sheets.palette_sel;
                                    let height = if row.is_file() { file_row_h } else { row_h };
                                    let (rect, resp) = ui.allocate_exact_size(
                                        vec2(ui.available_width(), height),
                                        Sense::click(),
                                    );
                                    if selected {
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(4),
                                            pal.selection,
                                        );
                                        ui.painter().rect_filled(
                                            Rect::from_min_size(
                                                Pos2::new(rect.left() + 1.0, rect.top() + 5.0),
                                                vec2(2.0, (height - 10.0).max(2.0)),
                                            ),
                                            CornerRadius::same(1),
                                            pal.accent,
                                        );
                                    } else if resp.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(4),
                                            theme::alpha(pal.ink, 8),
                                        );
                                    }
                                    if selected {
                                        ui.scroll_to_rect(rect, None);
                                    }

                                    let ink = if selected { pal.ink } else { pal.ink_soft };
                                    self.paint_palette_row(ui, rect, row, ink, &resp);
                                    if resp.clicked() {
                                        chosen = Some(i);
                                    }
                                }

                                let files_shown = rows.iter().filter(|r| r.is_file()).count();
                                let commands_shown = rows.len() - files_shown;
                                ui.add_space(6.0 * self.scale);
                                if rows.is_empty() {
                                    ui.painter().text(
                                        Pos2::new(
                                            ui.min_rect().left() + 10.0,
                                            ui.cursor().top() + 3.0,
                                        ),
                                        Align2::LEFT_TOP,
                                        "nothing matches",
                                        self.typo.ui_id(12.5 * self.scale),
                                        pal.ink_soft,
                                    );
                                    ui.add_space(18.0 * self.scale);
                                } else {
                                    let mut note = String::new();
                                    if files_shown > 0 {
                                        if files_shown < matched_total {
                                            note =
                                                format!("{files_shown} of {matched_total} files");
                                        } else {
                                            note = format!(
                                                "{files_shown} file{}",
                                                if matched_total == 1 { "" } else { "s" }
                                            );
                                        }
                                    }
                                    if commands_shown > 0 {
                                        if !note.is_empty() {
                                            note.push_str(" · ");
                                        }
                                        note.push_str(&format!(
                                            "{commands_shown} action{}",
                                            if commands_shown == 1 { "" } else { "s" }
                                        ));
                                    }
                                    ui.painter().text(
                                        Pos2::new(
                                            ui.min_rect().left() + 10.0,
                                            ui.cursor().top() + 3.0,
                                        ),
                                        Align2::LEFT_TOP,
                                        note,
                                        self.typo.ui_id(10.5 * self.scale),
                                        pal.ink_soft,
                                    );
                                    ui.add_space(18.0 * self.scale);
                                }
                            });
                    },
                );
            });

        if !close
            && self.frame > self.sheets.palette_opened_frame
            && ctx.input(|i| i.pointer.primary_clicked())
            && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
                launcher
                    .as_ref()
                    .is_none_or(|window| !window.response.rect.contains(p))
            })
        {
            close = true;
        }

        if let Some(i) = chosen {
            if let Some(row) = rows.get(i).cloned() {
                match row {
                    LaunchRow::Command { spec, .. } => self.launch(spec.action, ctx),
                    LaunchRow::File { path, .. } => self.open_file(&path),
                }
            }
            close = true;
        }
        if close {
            self.sheets.palette_open = false;
        }
    }

    /// `/`: an incremental search, drawn where vim would draw it — the bottom edge.
    pub(super) fn search_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let font = self.typo.ui_id(13.5 * self.scale);

        // Recompute on every keystroke, and jump to the first hit as it appears.
        let changed = self.sheets.search_seen != self.sheets.search_query;
        if changed {
            self.sheets.search_seen = self.sheets.search_query.clone();
            self.sheets.search_hits = self
                .open
                .as_ref()
                .map(|o| vim::matches(&o.doc, &self.sheets.search_query))
                .unwrap_or_default();
            if let Some(first) = self.sheets.search_hits.first().copied() {
                self.caret = first;
                self.ensure_visible();
            }
        }

        let screen = ctx.content_rect();
        let width = (screen.width() * 0.42).clamp(340.0, 680.0);
        let pos = Pos2::new(screen.left() + 14.0, screen.bottom() - 62.0);
        let mut close = false;
        let mut step: Option<bool> = None;

        egui::Window::new("find-in-file")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .fixed_pos(pos)
            .fixed_size(vec2(width, 0.0))
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.edge))
                    .corner_radius(CornerRadius::same(SHEET_RADIUS))
                    .inner_margin(Margin::symmetric(10, 6))
                    .shadow(sheet_shadow(&pal)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("/").font(font.clone()).color(pal.accent));
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.sheets.search_query)
                            .hint_text("search this file")
                            .font(font.clone())
                            .desired_width(width - 110.0)
                            .frame(Frame::NONE),
                    );
                    if !resp.has_focus() {
                        resp.request_focus();
                    }
                    let count = if self.sheets.search_hits.is_empty() {
                        "no match".to_string()
                    } else {
                        format!("{} match(es)", self.sheets.search_hits.len())
                    };
                    ui.label(RichText::new(count).font(font.clone()).color(pal.ink_soft));
                });
                let (enter, shift, esc) = ctx.input(|i| {
                    (
                        i.key_pressed(Key::Enter),
                        i.modifiers.shift,
                        i.key_pressed(Key::Escape),
                    )
                });
                if enter {
                    step = Some(!shift);
                }
                if esc {
                    close = true;
                }
            });

        if let Some(forward) = step {
            self.search_step(forward);
        }
        if close {
            self.sheets.search_open = false;
            self.chords.clear();
        }
    }

    /// How many controls the current settings section has, excluding the reset action.
    pub(super) fn settings_focus_count(&self) -> usize {
        match self.sheets.settings_tab {
            0 => 14,
            1 => self.typo.mono_voices.len() + self.typo.prose_voices.len(),
            2 => theme::THEMES.len(),
            3 => 1,
            _ => 0,
        }
    }

    /// The settings sheet's keyboard. Tab walks the rail, the controls and the reset
    /// action; arrows move; Space and Enter act. Letters are left to the text fields.
    pub(super) fn settings_key(
        &mut self,
        ctx: &egui::Context,
        key: Key,
        shift: bool,
        ctrl: bool,
        cmd: bool,
    ) -> bool {
        if ctrl || cmd {
            return false;
        }
        // A focused text field owns the keyboard.
        if ctx.memory(|m| m.focused().is_some()) {
            return false;
        }
        let tabs = SETTINGS_SECTIONS.len();
        if self.sheets.settings_tab >= tabs {
            self.sheets.settings_tab = tabs - 1;
        }
        let count = self.settings_focus_count();
        let total = count + 1;
        if self.sheets.settings_rail_focus {
            match key {
                Key::ArrowUp | Key::ArrowLeft => {
                    self.sheets.settings_tab = (self.sheets.settings_tab + tabs - 1) % tabs;
                    self.sheets.settings_focus = 0;
                    true
                }
                Key::ArrowDown | Key::ArrowRight => {
                    self.sheets.settings_tab = (self.sheets.settings_tab + 1) % tabs;
                    self.sheets.settings_focus = 0;
                    true
                }
                Key::Tab | Key::Enter | Key::Space => {
                    self.sheets.settings_rail_focus = false;
                    self.sheets.settings_focus = 0;
                    true
                }
                _ => false,
            }
        } else {
            self.sheets.settings_focus = self.sheets.settings_focus.min(total.saturating_sub(1));
            match key {
                Key::Tab => {
                    if shift {
                        if self.sheets.settings_focus == 0 {
                            self.sheets.settings_rail_focus = true;
                        } else {
                            self.sheets.settings_focus -= 1;
                        }
                    } else {
                        self.sheets.settings_focus += 1;
                        if self.sheets.settings_focus >= total {
                            self.sheets.settings_rail_focus = true;
                            self.sheets.settings_focus = 0;
                        }
                    }
                    true
                }
                Key::ArrowUp | Key::ArrowDown => {
                    if key == Key::ArrowDown {
                        if self.sheets.settings_focus < total.saturating_sub(1) {
                            self.sheets.settings_focus += 1;
                        }
                    } else if self.sheets.settings_focus > 0 {
                        self.sheets.settings_focus -= 1;
                    }
                    true
                }
                Key::ArrowLeft | Key::ArrowRight => {
                    let direction = if key == Key::ArrowLeft { -1 } else { 1 };
                    if self.sheets.settings_tab == 0
                        && matches!(self.sheets.settings_focus, 11..=13)
                    {
                        self.settings_adjust(ctx, self.sheets.settings_focus, direction as f32);
                    } else if self.sheets.settings_focus < count {
                        self.sheets.settings_focus = (self.sheets.settings_focus as i32 + direction)
                            .clamp(0, count as i32 - 1)
                            as usize;
                    }
                    true
                }
                Key::Space | Key::Enter => {
                    self.settings_activate(ctx, self.sheets.settings_focus);
                    true
                }
                _ => false,
            }
        }
    }

    /// Activate the focused settings control. `count` itself is the reset action.
    pub(super) fn settings_activate(&mut self, ctx: &egui::Context, focus: usize) {
        let count = self.settings_focus_count();
        if focus >= count {
            let now = ctx.input(|i| i.time);
            let armed = self.reset_asked.is_some_and(|t| now - t < 5.0);
            if armed {
                self.reset_asked = None;
                self.reset_defaults(ctx);
                self.sheets.settings_rail_focus = true;
                self.sheets.settings_focus = 0;
            } else {
                self.reset_asked = Some(now);
            }
            return;
        }
        match self.sheets.settings_tab {
            0 => self.settings_toggle_page(ctx, focus),
            1 => {
                let mono = self.typo.mono_voices.len();
                if focus < mono {
                    self.apply_typography(ctx, focus, self.typo.prose_idx);
                } else {
                    self.apply_typography(ctx, self.typo.mono_idx, focus - mono);
                }
            }
            2 => self.set_theme(ctx, focus),
            3 => {
                self.store.config.lsp.enabled = !self.store.config.lsp.enabled;
                self.store.save_config();
                self.start_discovery();
            }
            _ => {}
        }
    }

    pub(super) fn settings_toggle_page(&mut self, ctx: &egui::Context, focus: usize) {
        let cfg = &mut self.store.config;
        match focus {
            0 => cfg.wrap = !cfg.wrap,
            1 => cfg.guides = !cfg.guides,
            2 => cfg.serif_comments = !cfg.serif_comments,
            3 => cfg.focus_lens = !cfg.focus_lens,
            4 => cfg.line_numbers = !cfg.line_numbers,
            5 => cfg.compress_blank = !cfg.compress_blank,
            6 => cfg.distilled = !cfg.distilled,
            7 => {
                cfg.frameless = !cfg.frameless;
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!cfg.frameless));
            }
            8 => cfg.markdown = !cfg.markdown,
            9 => cfg.code_only = !cfg.code_only,
            10 => cfg.show_hidden = !cfg.show_hidden,
            _ => {}
        }
        self.store.save_config();
        self.style_dirty = true;
        if focus >= 9 {
            let root = self.folder.as_ref().map(|f| f.root.clone());
            if let Some(root) = root {
                let focus_path = self.open.as_ref().map(|o| o.path.clone());
                self.open_folder(&root, focus_path);
            }
        }
    }

    /// Left and right on the three page sliders.
    pub(super) fn settings_adjust(&mut self, ctx: &egui::Context, focus: usize, direction: f32) {
        match focus {
            11 => {
                let next = (self.store.config.measure as f32 + direction).clamp(
                    zenith_core::catalog::MEASURE_MIN as f32,
                    zenith_core::catalog::MEASURE_MAX as f32,
                );
                self.store.config.measure = next.round() as usize;
                self.style_dirty = true;
            }
            12 => {
                self.store.config.leading = (self.store.config.leading + direction * 0.02).clamp(
                    zenith_core::catalog::LEADING_MIN,
                    zenith_core::catalog::LEADING_MAX,
                );
                self.style_dirty = true;
            }
            13 => {
                self.scale = (self.scale + direction * 0.02).clamp(
                    zenith_core::catalog::SCALE_MIN,
                    zenith_core::catalog::SCALE_MAX,
                );
                self.store.config.font_scale = self.scale;
                self.style_dirty = true;
                self.rebuild_metrics(ctx);
            }
            _ => {}
        }
        self.store.save_config();
    }

    /// The Page section: the switches about how text is set, then its three numbers.
    fn settings_page(&mut self, ui: &mut Ui, style: &SettingsStyle) -> (bool, bool) {
        let pal = *style.pal;
        let font = style.font.clone();
        let small = style.small.clone();
        let values = style.values.clone();
        let content = style.content;
        let single = style.single;
        let focus = style.focus;
        let mut changed = false;
        let mut tree_changed = false;
        if single {
            let cfg = &mut self.store.config;
            ui.vertical(|ui| {
                ui.set_width(content);
                let mut base = 0;
                changed |= toggles(
                    ui,
                    &font,
                    &pal,
                    &mut [
                        ("soft wrap", &mut cfg.wrap),
                        ("indentation whispers", &mut cfg.guides),
                        ("comments as prose", &mut cfg.serif_comments),
                        ("focus lens", &mut cfg.focus_lens),
                        ("line numbers", &mut cfg.line_numbers),
                        ("compress blank lines", &mut cfg.compress_blank),
                        ("distilled types", &mut cfg.distilled),
                        ("themed window bar", &mut cfg.frameless),
                        ("render markdown", &mut cfg.markdown),
                    ],
                    focus,
                    &mut base,
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("the tree")
                        .font(small.clone())
                        .color(pal.ink_soft),
                );
                let tree = toggles(
                    ui,
                    &font,
                    &pal,
                    &mut [
                        ("source files only", &mut cfg.code_only),
                        ("include hidden files", &mut cfg.show_hidden),
                    ],
                    focus,
                    &mut base,
                );
                changed |= tree;
                tree_changed |= tree;
            });
        } else {
            let half = (content - 12.0) * 0.5;
            let mut left = 0;
            let mut right = 5;
            let mut tree_base = 9;
            ui.horizontal_top(|ui| {
                let cfg = &mut self.store.config;
                ui.vertical(|ui| {
                    ui.set_width(half);
                    changed |= toggles(
                        ui,
                        &font,
                        &pal,
                        &mut [
                            ("soft wrap", &mut cfg.wrap),
                            ("indentation whispers", &mut cfg.guides),
                            ("comments as prose", &mut cfg.serif_comments),
                            ("focus lens", &mut cfg.focus_lens),
                            ("line numbers", &mut cfg.line_numbers),
                        ],
                        focus,
                        &mut left,
                    );
                });
                ui.vertical(|ui| {
                    ui.set_width(half);
                    changed |= toggles(
                        ui,
                        &font,
                        &pal,
                        &mut [
                            ("compress blank lines", &mut cfg.compress_blank),
                            ("distilled types", &mut cfg.distilled),
                            ("themed window bar", &mut cfg.frameless),
                            ("render markdown", &mut cfg.markdown),
                        ],
                        focus,
                        &mut right,
                    );
                    // The tree's own two questions, which are
                    // about what to read rather than how to set
                    // it. They re-read the folder as they change,
                    // so they still answer on the clicked frame.
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("the tree")
                            .font(small.clone())
                            .color(pal.ink_soft),
                    );
                    let tree = toggles(
                        ui,
                        &font,
                        &pal,
                        &mut [
                            ("source files only", &mut cfg.code_only),
                            ("include hidden files", &mut cfg.show_hidden),
                        ],
                        focus,
                        &mut tree_base,
                    );
                    changed |= tree;
                    tree_changed |= tree;
                });
            });
        }
        ui.add_space(6.0);
        let cfg = &mut self.store.config;
        let mut measure = cfg.measure as f32;
        if line_slider(
            ui,
            &pal,
            &font,
            &values,
            Slider {
                label: "measure",
                value: &mut measure,
                range: zenith_core::catalog::MEASURE_MIN as f32
                    ..=zenith_core::catalog::MEASURE_MAX as f32,
                decimals: 0,
            },
            focus == Some(11),
        ) {
            cfg.measure = measure.round() as usize;
            changed = true;
        }
        changed |= line_slider(
            ui,
            &pal,
            &font,
            &values,
            Slider {
                label: "leading",
                value: &mut cfg.leading,
                range: zenith_core::catalog::LEADING_MIN..=zenith_core::catalog::LEADING_MAX,
                decimals: 2,
            },
            focus == Some(12),
        );
        changed |= line_slider(
            ui,
            &pal,
            &font,
            &values,
            Slider {
                label: "type size",
                value: &mut self.scale,
                range: zenith_core::catalog::SCALE_MIN..=zenith_core::catalog::SCALE_MAX,
                decimals: 2,
            },
            focus == Some(13),
        );
        (changed, tree_changed)
    }

    /// The Type section: the two voices, chosen by name.
    fn settings_type(&mut self, ui: &mut Ui, style: &SettingsStyle) -> (usize, usize) {
        let pal = *style.pal;
        let font = style.font.clone();
        let small = style.small.clone();
        let content = style.content;
        let focus = style.focus;
        let mut new_mono = self.typo.mono_idx;
        let mut new_prose = self.typo.prose_idx;
        let column = (content - 16.0) * 0.5;
        let mono_len = self.typo.mono_voices.len();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(column);
                ui.label(
                    RichText::new("code")
                        .font(small.clone())
                        .color(pal.ink_soft),
                );
                for (i, v) in self.typo.mono_voices.iter().enumerate() {
                    let selected = i == self.typo.mono_idx;
                    let ok = typography::loads(v);
                    let text = RichText::new(v.label.clone())
                        .font(font.clone())
                        .color(if ok { pal.ink_soft } else { pal.ink_faint });
                    let r = ui.selectable_label(selected, text);
                    if focus == Some(i) {
                        ui.painter().rect_stroke(
                            r.rect.shrink(1.0),
                            CornerRadius::same(ROW_RADIUS),
                            Stroke::new(1.0, pal.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if r.clicked() {
                        new_mono = i;
                    }
                    r.on_hover_text(RichText::new(v.note.clone()).font(small.clone()));
                }
            });
            ui.vertical(|ui| {
                ui.set_width(column);
                ui.label(
                    RichText::new("prose")
                        .font(small.clone())
                        .color(pal.ink_soft),
                );
                for (i, v) in self.typo.prose_voices.iter().enumerate() {
                    let selected = i == self.typo.prose_idx;
                    let text = RichText::new(v.label.clone())
                        .font(font.clone())
                        .color(pal.ink_soft);
                    let r = ui.selectable_label(selected, text);
                    if focus == Some(mono_len + i) {
                        ui.painter().rect_stroke(
                            r.rect.shrink(1.0),
                            CornerRadius::same(ROW_RADIUS),
                            Stroke::new(1.0, pal.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if r.clicked() {
                        new_prose = i;
                    }
                    r.on_hover_text(RichText::new(v.note.clone()).font(small.clone()));
                }
            });
        });
        ui.add_space(8.0);
        let chrome = self
            .typo
            .ui_voices
            .first()
            .map(|v| v.label.clone())
            .unwrap_or_else(|| self.typo.ui_label.clone());
        ui.label(
            RichText::new(format!("interface: {chrome}"))
                .font(small.clone())
                .color(pal.ink_soft),
        );
        if self.typo.bundled_fallback {
            ui.label(
                RichText::new("no system fonts found: egui's bundled faces are in use")
                    .font(small.clone())
                    .color(pal.ink_soft),
            );
        }
        (new_mono, new_prose)
    }

    /// The Theme section: every theme shown by being used.
    fn settings_theme(
        &mut self,
        ui: &mut Ui,
        style: &SettingsStyle,
        sample: &[SampleLine],
    ) -> Option<usize> {
        let small = style.small.clone();
        let content = style.content;
        let focus = style.focus;
        let mut theme_choice = None;
        // Each theme is shown by being used: a few lines of the
        // sample, set in that theme's paper and ink. A name and a
        // swatch tell you nothing about how a page reads.
        let tile_w = (content - 14.0) * 0.5;
        for row in theme::THEMES.chunks(2) {
            ui.horizontal(|ui| {
                for p in row {
                    let chosen = p.name == self.palette.name;
                    let tile_h = 84.0 * self.scale.min(1.5);
                    let index = theme::THEMES
                        .iter()
                        .position(|t| t.name == p.name)
                        .unwrap_or(0);
                    let focused = focus == Some(index);
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(tile_w, tile_h), egui::Sense::click());
                    theme_tile(
                        ui,
                        rect,
                        p,
                        sample,
                        TileState { chosen, focused },
                        &self.typo,
                        self.scale,
                    );
                    if response.clicked() {
                        theme_choice = Some(
                            theme::THEMES
                                .iter()
                                .position(|t| t.name == p.name)
                                .unwrap_or(0),
                        );
                    }
                    response.on_hover_text(RichText::new(p.mood).font(small.clone()));
                    ui.add_space(10.0);
                }
            });
            ui.add_space(10.0);
        }
        theme_choice
    }

    /// The Language section: the server switch, what each server is doing, and its path.
    fn settings_language(
        &mut self,
        ui: &mut Ui,
        style: &SettingsStyle,
    ) -> (bool, bool, Option<String>) {
        let pal = *style.pal;
        let font = style.font.clone();
        let small = style.small.clone();
        let content = style.content;
        let focus = style.focus;
        let mut changed = false;
        let mut lsp_changed = false;
        let mut restart: Option<String> = None;
        let mut on = self.store.config.lsp.enabled;
        let mut base = 0;
        if toggles(
            ui,
            &font,
            &pal,
            &mut [("start language servers", &mut on)],
            focus,
            &mut base,
        ) {
            self.store.config.lsp.enabled = on;
            changed = true;
            lsp_changed = true;
        }
        ui.add_space(10.0);

        if self.lsp.kinds.is_empty() {
            ui.label(
                RichText::new(
                    "nothing in this folder speaks a language zenith \
                     has a server for",
                )
                .font(small.clone())
                .color(pal.ink_soft),
            );
        }
        for kind in self.lsp.kinds.clone() {
            let (state, why) = self.server_state(kind.id);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(kind.title)
                        .font(font.clone())
                        .color(pal.ink_soft),
                );
                let ink = if state == "failed" {
                    pal.tone(Tone::Invalid)
                } else {
                    pal.ink_soft
                };
                ui.label(RichText::new(state).font(small.clone()).color(ink));
            });
            if let Some(why) = why {
                ui.label(RichText::new(why).font(small.clone()).color(pal.ink_soft));
            }
            let discovered = self
                .lsp
                .available
                .iter()
                .find(|a| a.kind.id == kind.id)
                .map(|a| display_path(&a.binary));
            let entry = self
                .store
                .config
                .lsp
                .servers
                .entry(kind.id.to_string())
                .or_default();
            ui.visuals_mut().extreme_bg_color = pal.panel_deep;
            // A path is code, and it is edited: set it in the code
            // voice, over a hairline, which is how this program
            // draws every field it has.
            let edit = ui.add(
                egui::TextEdit::singleline(&mut entry.command)
                    .hint_text(
                        discovered
                            .clone()
                            .unwrap_or_else(|| "path to the binary".into()),
                    )
                    .font(self.typo.code_id(11.5 * self.scale))
                    .desired_width(content)
                    .frame(Frame::NONE),
            );
            ui.painter().hline(
                edit.rect.x_range(),
                edit.rect.bottom() + 2.0,
                Stroke::new(1.0, pal.edge),
            );
            if edit.changed() {
                changed = true;
                self.lsp.path_dirty.insert(kind.id.to_string());
            }
            if edit.lost_focus() && self.lsp.path_dirty.remove(kind.id) {
                // Finishing the edit restarts that language's
                // server, so the new path is the one in use. A
                // field visited and left alone is not an edit.
                restart = Some(kind.id.to_string());
            }
            ui.add_space(8.0);
        }
        (changed, lsp_changed, restart)
    }

    /// The Keys section: the same map the help page shows, with a filter.
    fn settings_keys(&mut self, ui: &mut Ui, style: &SettingsStyle) {
        let pal = *style.pal;
        let small = style.small.clone();
        let content = style.content;
        ui.visuals_mut().extreme_bg_color = pal.panel_deep;
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.sheets.keys_query)
                .hint_text("filter keys")
                .font(self.typo.ui_id(12.5 * self.scale))
                .desired_width(content)
                .frame(Frame::NONE),
        );
        ui.painter().hline(
            edit.rect.x_range(),
            edit.rect.bottom() + 2.0,
            Stroke::new(1.0, pal.edge),
        );
        ui.add_space(8.0);
        let query = self.sheets.keys_query.to_lowercase();
        ScrollArea::vertical()
            .id_salt("keys")
            .max_height(280.0)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (group, rows) in HELP_GROUPS {
                    let matching: Vec<&(&str, &str)> = rows
                        .iter()
                        .filter(|(k, v)| {
                            query.is_empty()
                                || k.to_lowercase().contains(&query)
                                || v.to_lowercase().contains(&query)
                        })
                        .collect();
                    if matching.is_empty() {
                        continue;
                    }
                    ui.label(
                        RichText::new(group.to_uppercase())
                            .font(self.typo.ui_id(9.5 * self.scale))
                            .color(pal.ink_soft)
                            .extra_letter_spacing(0.1 * self.scale),
                    );
                    ui.add_space(2.0);
                    egui::Grid::new(format!("keys-{group}"))
                        .num_columns(2)
                        .spacing([16.0, 3.0])
                        .show(ui, |ui| {
                            for &(k, v) in matching {
                                ui.label(
                                    RichText::new(k)
                                        .font(self.typo.code_id(11.5 * self.scale))
                                        .color(pal.accent),
                                );
                                ui.label(RichText::new(v).font(small.clone()).color(pal.ink_soft));
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                }
            });
    }

    /// Settings: a modal screen, a rail of sections, and one section at a time.
    ///
    /// Modal because settings are a place you go, not a card that floats over the page and
    /// lets a scroll wheel move the text underneath you. The page is still there — veiled
    /// in the theme's own ink — and it comes back the way it was.
    pub(super) fn settings_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let font = self.typo.ui_id(12.5 * self.scale);
        let title = self.typo.prose_id(20.0 * self.scale);
        let head = self.typo.prose_id(14.5 * self.scale);
        let small = self.typo.ui_id(11.5 * self.scale);
        let values = self.typo.code_id(11.5 * self.scale);
        // The sheet grows with the type and the window; it never keeps a 560 pt column
        // when the reader has asked for 1.8x type.
        let screen = ctx.content_rect();
        let width = (560.0 * self.scale)
            .clamp(560.0, 820.0)
            .min((screen.width() - 40.0).max(560.0));
        let rail = (118.0 * self.scale).clamp(118.0, 160.0);
        let content = width - rail - 78.0;
        // A two-column page is only honest while the longest toggle label and its box
        // fit a column.
        let label_w = 19.0 * (12.5 * self.scale) * 0.5 + (12.5 * self.scale) * 0.88 + 9.0;
        let single = ((content - 12.0) * 0.5) < label_w;
        let mut open = self.sheets.show_settings;
        let mut changed = false;
        let mut lsp_changed = false;
        let mut tree_changed = false;
        let mut reset_settings = false;
        let mut restart: Option<String> = None;
        let mut new_mono = self.typo.mono_idx;
        let mut new_prose = self.typo.prose_idx;
        let mut theme_choice: Option<usize> = None;
        if self.theme_preview.is_none() {
            self.theme_preview = Some(sample_tokens(&self.hl));
        }
        let sample = self.theme_preview.clone().unwrap_or_default();

        let modal =
            egui::Modal::new(egui::Id::new("zenith-settings"))
                .backdrop_color(theme::alpha(pal.ink, 44))
                .frame(
                    Frame::NONE
                        .fill(pal.panel)
                        .stroke(Stroke::new(1.0, pal.edge))
                        .corner_radius(CornerRadius::same(SHEET_RADIUS))
                        .shadow(sheet_shadow(&pal)),
                )
                .show(ctx, |ui| {
                    ui.set_width(width);
                    // ---- the title ----
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new("Settings").font(title.clone()).color(pal.ink));
                    });
                    ui.add_space(8.0);
                    ui.painter().hline(
                        ui.min_rect().left()..=ui.min_rect().right(),
                        ui.cursor().top(),
                        Stroke::new(1.0, pal.edge),
                    );
                    ui.add_space(14.0);

                    let body_top = ui.cursor().top();
                    let mut rail_edge = 0.0;
                    ui.horizontal_top(|ui| {
                        ui.add_space(20.0);
                        // ---- the rail ----
                        ui.vertical(|ui| {
                            ui.set_width(rail);
                            for (i, name) in SETTINGS_SECTIONS.iter().enumerate() {
                                let selected = self.sheets.settings_tab == i;
                                let text = RichText::new(*name)
                                    .font(head.clone())
                                    .color(if selected { pal.ink } else { pal.ink_soft });
                                let response =
                                    ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                                if selected && self.sheets.settings_rail_focus {
                                    ui.painter().rect_filled(
                                        Rect::from_min_size(
                                            Pos2::new(
                                                response.rect.left() - 8.0,
                                                response.rect.top() + 2.0,
                                            ),
                                            vec2(2.0, (response.rect.height() - 4.0).max(2.0)),
                                        ),
                                        CornerRadius::same(1),
                                        pal.accent,
                                    );
                                }
                                if response.clicked() {
                                    self.sheets.settings_tab = i;
                                    self.sheets.settings_rail_focus = true;
                                    self.sheets.settings_focus = 0;
                                }
                            }
                            rail_edge = ui.min_rect().right();
                        });
                        ui.add_space(18.0);

                        // ---- the section ----
                        ui.vertical(|ui| {
                            ui.set_width(content);
                            // A fixed minimum height keeps the screen from jumping as the
                            // reader moves down the rail.
                            ui.set_min_height(296.0);
                            let focus = if self.sheets.settings_rail_focus {
                                None
                            } else {
                                Some(self.sheets.settings_focus)
                            };
                            let style = SettingsStyle {
                                pal: &pal,
                                font: &font,
                                small: &small,
                                values: &values,
                                content,
                                single,
                                focus,
                            };
                            match self.sheets.settings_tab {
                                0 => {
                                    let (page_changed, tree_now) = self.settings_page(ui, &style);
                                    changed |= page_changed;
                                    tree_changed |= tree_now;
                                }
                                1 => {
                                    let (mono, prose) = self.settings_type(ui, &style);
                                    new_mono = mono;
                                    new_prose = prose;
                                }
                                2 => {
                                    theme_choice = self.settings_theme(ui, &style, &sample);
                                }
                                3 => {
                                    let (lsp_now, lsp_restart, restart_now) =
                                        self.settings_language(ui, &style);
                                    changed |= lsp_now;
                                    lsp_changed |= lsp_restart;
                                    restart = restart_now;
                                }
                                4 => self.settings_keys(ui, &style),
                                _ => {}
                            }
                        });
                    });
                    ui.add_space(16.0);

                    // ---- the footer ----
                    // Reverting is the one thing here that cannot be undone by putting it
                    // back by hand, so it lives alone under its own rule, and asks twice.
                    let footer_y = ui.cursor().top();
                    ui.painter().hline(
                        (ui.min_rect().left() + 20.0)..=(ui.min_rect().right() - 20.0),
                        footer_y,
                        Stroke::new(1.0, pal.edge),
                    );
                    // The rail's rule runs the whole way to the footer, not just past the
                    // last tab: it divides the sheet, not the list.
                    ui.painter().vline(
                        rail_edge + 9.0,
                        egui::Rangef::new(body_top, footer_y),
                        Stroke::new(1.0, pal.edge),
                    );
                    ui.add_space(10.0);
                    let now = ui.input(|i| i.time);
                    let armed = self.reset_asked.is_some_and(|t| now - t < 5.0);
                    let reset_focused = !self.sheets.settings_rail_focus
                        && self.sheets.settings_focus == self.settings_focus_count();
                    let text = if armed {
                        "restore defaults?"
                    } else {
                        "restore defaults"
                    };
                    let colour = if armed {
                        pal.tone(Tone::Invalid)
                    } else {
                        pal.ink_soft
                    };
                    let galley =
                        ui.painter()
                            .layout_no_wrap(text.to_string(), small.clone(), colour);
                    let button_h = (small.size * 2.2).max(26.0);
                    let button_w = galley.size().x + 20.0;
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        let (rect, reset) =
                            ui.allocate_exact_size(vec2(button_w, button_h), egui::Sense::click());
                        let fill = if reset.is_pointer_button_down_on() {
                            theme::alpha(pal.ink, 30)
                        } else if reset.hovered() {
                            theme::alpha(pal.ink, 16)
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(4),
                            Stroke::new(
                                1.0,
                                if armed {
                                    pal.tone(Tone::Invalid)
                                } else if reset_focused {
                                    pal.accent
                                } else {
                                    pal.edge
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().galley(
                            Pos2::new(
                                rect.center().x - galley.size().x * 0.5,
                                rect.center().y - galley.size().y * 0.5,
                            ),
                            galley,
                            colour,
                        );
                        if reset.clicked() {
                            if armed {
                                self.reset_asked = None;
                                reset_settings = true;
                            } else {
                                self.reset_asked = Some(now);
                            }
                        }
                    });
                    ui.add_space(14.0);
                });

        if reset_settings {
            self.reset_defaults(ctx);
        }
        if let Some(id) = restart {
            self.restart_server(&id);
        }
        if lsp_changed {
            self.start_discovery();
        }
        if tree_changed {
            // The tree's filters are questions about what to read, and the answer is a
            // new walk of the folder — which happens in the background, so it is still
            // immediate as far as the reader is concerned.
            let root = self.folder.as_ref().map(|f| f.root.clone());
            if let Some(root) = root {
                let focus = self.open.as_ref().map(|o| o.path.clone());
                self.open_folder(&root, focus);
            }
        }
        if new_mono != self.typo.mono_idx || new_prose != self.typo.prose_idx {
            self.apply_typography(ctx, new_mono, new_prose);
        }
        if let Some(i) = theme_choice {
            self.set_theme(ctx, i);
        }
        if changed {
            self.store.config.font_scale = self.scale;
            self.store.save_config();
            self.rebuild_metrics(ctx);
            // The frame can be handed back to the desktop, or taken away, while running.
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(
                !self.store.config.frameless,
            ));
        }
        if modal.should_close() {
            open = false;
        }
        self.sheets.show_settings = open;
    }

    /// Put every piece of taste back to how it shipped.
    ///
    /// The servers' paths stay: a path is something the reader went and found on their own
    /// machine, not a preference, and throwing it away would cost them the search again.
    pub(super) fn reset_defaults(&mut self, ctx: &egui::Context) {
        let mut fresh = zenith_core::catalog::Config::default();
        fresh.lsp.servers = std::mem::take(&mut self.store.config.lsp.servers);
        self.store.config = fresh;
        self.scale = self.store.config.font_scale;
        self.palette = theme::by_name(&self.store.config.theme);
        self.typo = typography::build(0, 0, 0);
        ctx.set_fonts(self.typo.defs.clone());
        apply_style(ctx, &self.palette, &self.typo, self.scale);
        self.rebuild_metrics(ctx);
        self.store.save_config();
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(
            !self.store.config.frameless,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if self.palette.is_dark() {
            egui::SystemTheme::Dark
        } else {
            egui::SystemTheme::Light
        }));
        // The tree's filters and the servers both follow the defaults.
        match self.folder.as_ref().map(|f| f.root.clone()) {
            Some(root) => {
                let focus = self.open.as_ref().map(|o| o.path.clone());
                self.open_folder(&root, focus);
            }
            None => self.start_discovery(),
        }
        self.say("back to the defaults");
    }

    /// What a language's server is doing, and why, for the settings panel.
    pub(super) fn server_state(&self, id: &str) -> (String, Option<String>) {
        if !self.store.config.lsp.enabled {
            // Not running is not the same as not found: nothing was looked for.
            return ("off".into(), None);
        }
        if let Some(session) = self.lsp.servers.iter().find(|s| s.id() == id) {
            return match session.state() {
                lsp::State::Starting => ("starting".into(), None),
                lsp::State::Ready => ("ready".into(), None),
                lsp::State::Failed(why) => ("failed".into(), Some(why.clone())),
            };
        }
        if self.lsp.available.iter().any(|a| a.kind.id == id) {
            ("found".into(), None)
        } else if self.lsp.discovering.is_some() {
            ("looking…".into(), None)
        } else {
            ("not found".into(), None)
        }
    }

    /// Stop a language's server so the next question starts it again — with the path the
    /// reader has just written down.
    pub(super) fn restart_server(&mut self, id: &str) {
        for session in self.lsp.servers.iter_mut().filter(|s| s.id() == id) {
            session.stop();
        }
        self.lsp.servers.retain(|s| s.id() != id);
        self.lsp.available.retain(|a| a.kind.id != id);
        self.lsp.outline_asked = None;
        let Some(root) = self.folder.as_ref().map(|f| f.root.clone()) else {
            return;
        };
        let kinds: Vec<&'static lsp::servers::Kind> = self
            .lsp
            .kinds
            .iter()
            .filter(|k| k.id == id)
            .copied()
            .collect();
        self.discover(kinds, root);
    }
}
