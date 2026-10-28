//! The branch review: the bar, the base sheet, and the old code it lifts.
//!
//! A review is not a patch view. The page still holds whole files; this module only
//! decides which lines are marked, which file is current, and what the previous
//! version of a changed line was.

use super::widgets::*;
use super::*;

/// The review's own state: the branch it is reading, the file being read, the base it
/// is compared with, and the place to return to after a definition detour.
///
/// It lives beside the review methods rather than in the window struct, so the module
/// owns the thing it operates on.
/// What a key press on the base sheet decided.
#[derive(Default)]
struct BaseIntent {
    choose: Option<Base>,
    custom: bool,
    close: bool,
    focus_field: bool,
}

#[derive(Default)]
pub(super) struct ReviewState {
    pub(super) changes: Option<Review>,
    pub(super) index: usize,
    pub(super) base: Base,
    pub(super) load: Option<ReviewLoad>,
    pub(super) show_base: bool,
    pub(super) base_sel: usize,
    pub(super) base_opened_frame: usize,
    pub(super) reference: String,
    pub(super) branches: Vec<String>,
    pub(super) error: String,
    pub(super) hover: Option<usize>,
    pub(super) peek: Option<usize>,
    pub(super) return_to: Option<Place>,
    pub(super) strip_scroll: bool,
    pub(super) after_load: bool,
}

impl Zenith {
    /// Open or close the branch review. `g r` is a toggle rather than a one-way door:
    /// the reader can put the change set down without hunting for a command.
    pub(super) fn toggle_review(&mut self) {
        if self.session.changes.is_some() || self.session.load.is_some() {
            self.leave_review();
            self.status = "review closed".into();
        } else {
            self.start_review(self.session.base.clone());
        }
    }

    pub(super) fn leave_review(&mut self) {
        self.session.changes = None;
        self.session.load = None;
        self.session.hover = None;
        self.session.peek = None;
        self.session.return_to = None;
        self.session.show_base = false;
    }

    pub(super) fn start_review(&mut self, base: Base) {
        if self.session.load.is_some() {
            return;
        }
        let root = self.folder.as_ref().map(|f| f.root.clone()).or_else(|| {
            self.open
                .as_ref()
                .map(|o| crate::catalog::project_root(&o.path))
        });
        let Some(root) = root else {
            self.status = "open a folder to review its changes".into();
            return;
        };
        let files: Vec<FileEntry> = self
            .folder
            .as_ref()
            .map(|f| f.files.clone())
            .unwrap_or_default();
        self.session.changes = None;
        self.session.index = 0;
        self.session.hover = None;
        self.session.peek = None;
        self.session.return_to = None;
        self.bloom.open = true;
        self.bloom.last_touch = 0.0;
        self.session.base = base.clone();
        self.session.show_base = false;
        self.session.error.clear();
        self.status = format!("reading changes against {}…", base.label());
        let (tx, rx) = std::sync::mpsc::channel();
        let base_for_thread = base.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::review::collect(&root, &base_for_thread, &files));
        });
        self.session.load = Some(ReviewLoad { rx });
    }

    pub(super) fn open_review_base(&mut self) {
        let Some(root) = self.folder.as_ref().map(|f| f.root.clone()).or_else(|| {
            self.open
                .as_ref()
                .map(|o| crate::catalog::project_root(&o.path))
        }) else {
            self.status = "open a folder to review its changes".into();
            return;
        };
        self.session.branches = crate::review::branches(&root);
        self.session.base_sel = match &self.session.base {
            Base::Working => 0,
            Base::Staged => 1,
            Base::LastCommit => 2,
            Base::Ref(reference) => self
                .session
                .branches
                .iter()
                .position(|branch| branch == reference)
                .map(|index| 3 + index)
                .unwrap_or_else(|| {
                    // A commit or a branch the recent list does not hold: leave it in
                    // the field so the current base is still visible in the sheet.
                    self.session.reference = reference.clone();
                    0
                }),
        };
        self.session.base_opened_frame = self.frame;
        self.session.error.clear();
        self.session.show_base = true;
    }

    /// In review the same dot and words live in their own quiet band above the page,
    /// so scrolling the page cannot put them over the code.
    pub(super) fn review_bloom_bar(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let height = (34.0 * self.scale).clamp(34.0, 48.0);
        egui::Panel::top("zenith-review-bloom")
            .resizable(false)
            .exact_size(height)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                let now = ui.ctx().input(|i| i.time);
                if self.bloom.last_touch <= 0.0 {
                    self.bloom.last_touch = now;
                }
                let open_t = ui.ctx().animate_bool_with_time(
                    egui::Id::new("zenith-review-bloom"),
                    self.bloom.open,
                    0.18,
                );
                let rect = ui.max_rect();
                let hovered = self.paint_bloom_line(ui, rect.center(), open_t, 1.0, now);
                self.bloom_hover(hovered, now);
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0, pal.edge),
                );
            });
    }

    pub(super) fn review_bar(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let branch = self
            .session
            .changes
            .as_ref()
            .map(|r| short_branch(&r.branch));
        let base_name = short_branch(&self.session.base.label());
        let base_label = match &branch {
            Some(branch) => format!("{branch} ← {base_name}"),
            None => base_name,
        };
        let loading = self.session.load.is_some();
        let files = self.session.changes.as_ref().map(|r| r.files.clone());
        egui::Panel::top("zenith-review")
            .resizable(false)
            .exact_size((64.0 * self.scale).clamp(64.0, 104.0))
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(16, 7)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    paint_tracked(
                        ui.painter(),
                        Pos2::new(ui.cursor().left(), ui.cursor().top() + 9.0),
                        Align2::LEFT_TOP,
                        "REVIEW",
                        self.typo.ui_id(9.5 * self.scale),
                        pal.ink_soft,
                        LABEL_TRACKING * 9.5 * self.scale,
                    );
                    ui.add_space(74.0 * self.scale / 1.4);
                    let base = ui.add(
                        egui::Button::new(
                            RichText::new(format!("base · {base_label}"))
                                .font(self.typo.ui_id(11.0 * self.scale))
                                .color(pal.ink),
                        )
                        .frame(true)
                        .fill(pal.panel_deep),
                    );
                    if base.clicked() {
                        self.open_review_base();
                    }
                    if let Some(files) = &files {
                        ui.label(
                            RichText::new(format!("{} of {}", self.session.index + 1, files.len()))
                                .font(self.typo.ui_id(10.5 * self.scale))
                                .color(pal.ink_soft),
                        );
                    } else if loading {
                        ui.label(
                            RichText::new("reading changes…")
                                .font(self.typo.ui_id(10.5 * self.scale))
                                .color(pal.ink_soft),
                        );
                    }
                    let hint = if self.session.return_to.is_some() {
                        "q  back to the review   ·   J K  files   [ ]  changes   p  peek"
                    } else {
                        "q  close   ·   J K  files   [ ]  changes   p  peek"
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(hint)
                                .font(self.typo.ui_id(9.5 * self.scale))
                                .color(pal.ink_soft),
                        );
                    });
                });
                if let Some(files) = &files {
                    ui.spacing_mut().scroll.floating = false;
                    ui.spacing_mut().scroll.foreground_color = true;
                    ui.spacing_mut().scroll.bar_width = 4.0;
                    ScrollArea::horizontal()
                        .id_salt("review-files")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0 * self.scale;
                                for (i, file) in files.iter().enumerate() {
                                    let current = i == self.session.index;
                                    let label = format!("{}  {}", file.status.mark(), file.rel);
                                    let color = if current { pal.ink } else { pal.ink_soft };
                                    let response = ui.add(
                                        egui::Button::new(
                                            RichText::new(label)
                                                .font(self.typo.code_id(10.5 * self.scale))
                                                .color(color),
                                        )
                                        .frame(false),
                                    );
                                    if current {
                                        ui.painter().hline(
                                            response.rect.x_range(),
                                            response.rect.bottom(),
                                            Stroke::new(1.5, pal.accent),
                                        );
                                        if self.session.strip_scroll {
                                            response.scroll_to_me(Some(egui::Align::Center));
                                        }
                                    }
                                    response.clone().on_hover_text(format!(
                                        "{} · {}",
                                        file.status.label(),
                                        file.rel
                                    ));
                                    if response.clicked() {
                                        self.open_review_index(i);
                                    }
                                }
                            });
                        });
                    self.session.strip_scroll = false;
                }
            });
    }

    pub(super) fn review_bar_height(&self) -> f32 {
        if self.session.changes.is_some() || self.session.load.is_some() {
            (64.0 * self.scale).clamp(64.0, 104.0)
        } else {
            0.0
        }
    }

    pub(super) fn review_bloom_height(&self) -> f32 {
        if self.session.changes.is_some() || self.session.load.is_some() {
            (34.0 * self.scale).clamp(34.0, 48.0)
        } else {
            0.0
        }
    }

    /// The base sheet's keyboard. Returns what the key press decided, without touching
    /// the review: the sheet's drawing and its outcome stay separate.
    fn base_keys(
        &mut self,
        ctx: &egui::Context,
        field_id: egui::Id,
        field_focused: bool,
        row_count: usize,
        branches: &[String],
    ) -> BaseIntent {
        let (kd, ku, kj, kk, kenter, ktab, kesc) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::J),
                i.key_pressed(Key::K),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Tab),
                i.key_pressed(Key::Escape),
            )
        });

        let mut choose: Option<Base> = None;
        let mut custom = false;
        let mut close = false;
        let mut focus_field = false;

        if field_focused {
            if kenter && !self.session.reference.trim().is_empty() {
                custom = true;
            }
            if ktab {
                ctx.memory_mut(|m| m.surrender_focus(field_id));
            }
            if kesc {
                close = true;
            }
        } else {
            let count = row_count.max(1);
            if kd || kj {
                self.session.base_sel = (self.session.base_sel + 1) % count;
            }
            if ku || kk {
                self.session.base_sel = self
                    .session
                    .base_sel
                    .checked_sub(1)
                    .unwrap_or(count.saturating_sub(1));
            }
            if ktab {
                focus_field = true;
            }
            if kesc {
                close = true;
            }
            if kenter {
                choose = if self.session.base_sel < 3 {
                    Some(
                        [Base::Working, Base::Staged, Base::LastCommit][self.session.base_sel]
                            .clone(),
                    )
                } else {
                    branches
                        .get(self.session.base_sel - 3)
                        .cloned()
                        .map(Base::Ref)
                };
            }
        }
        BaseIntent {
            choose,
            custom,
            close,
            focus_field,
        }
    }

    /// The recent branches under the three fixed bases. Its own scroll, its own
    /// selection, and no knowledge of what happens after a choice.
    fn base_branches(
        &mut self,
        ui: &mut Ui,
        branches: &[String],
        choose: &mut Option<Base>,
        branch_h: f32,
        screen_h: f32,
    ) {
        let pal = self.palette;
        let label_font = self.typo.ui_id(9.5 * self.scale);
        let code_font = self.typo.code_id(11.5 * self.scale);
        if !branches.is_empty() {
            ui.add_space(14.0 * self.scale);
            paint_tracked(
                ui.painter(),
                ui.cursor().left_top(),
                Align2::LEFT_TOP,
                "RECENT BRANCHES",
                label_font.clone(),
                pal.ink_soft,
                LABEL_TRACKING * 9.5 * self.scale,
            );
            ui.add_space(17.0 * self.scale);
            ui.spacing_mut().scroll.floating = false;
            ui.spacing_mut().scroll.foreground_color = true;
            ui.spacing_mut().scroll.bar_width = 4.0;
            ScrollArea::vertical()
                .id_salt("review-bases")
                .max_height((148.0 * self.scale).min(screen_h * 0.28))
                .show(ui, |ui| {
                    for (j, branch) in branches.iter().enumerate() {
                        let i = 3 + j;
                        let selected = self.session.base == Base::Ref(branch.clone());
                        let cursor = self.session.base_sel == i;
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(ui.available_width(), branch_h),
                            Sense::click(),
                        );
                        if selected || cursor || response.hovered() {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(4), pal.selection);
                        }
                        if selected {
                            ui.painter().rect_filled(
                                Rect::from_min_size(
                                    Pos2::new(rect.left(), rect.top() + 7.0),
                                    vec2(2.0, (rect.height() - 14.0).max(1.0)),
                                ),
                                CornerRadius::same(1),
                                pal.accent,
                            );
                        }
                        let ink = if selected || cursor || response.hovered() {
                            pal.ink
                        } else {
                            pal.ink_soft
                        };
                        ui.painter().text(
                            Pos2::new(rect.left() + 13.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            branch,
                            code_font.clone(),
                            ink,
                        );
                        if cursor {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        if response.clicked() {
                            *choose = Some(Base::Ref(branch.clone()));
                        }
                    }
                });
        }
    }

    pub(super) fn review_base_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let screen = ctx.content_rect();
        let width = (560.0 * self.scale)
            .clamp(560.0, 820.0)
            .min((screen.width() - 40.0).max(360.0));
        let high = self.scale > 1.3 || screen.height() <= 700.0;
        let top = self.sheet_top(screen, high, false);
        let pos = Pos2::new(screen.center().x - width * 0.5, top);

        let branches = self.session.branches.clone();
        let row_count = 3 + branches.len();
        self.session.base_sel = self.session.base_sel.min(row_count.saturating_sub(1));

        let field_id = egui::Id::new("review-base-ref");
        if self.frame == self.session.base_opened_frame {
            ctx.memory_mut(|m| m.surrender_focus(field_id));
        }
        let field_focused = ctx.memory(|m| m.has_focus(field_id));

        let intent = self.base_keys(ctx, field_id, field_focused, row_count, &branches);
        let mut choose = intent.choose;
        let mut custom = intent.custom;
        let close = intent.close;
        let focus_field = intent.focus_field;

        let row_h = (36.0 * self.scale).clamp(36.0, 52.0);
        let branch_h = (28.0 * self.scale).clamp(28.0, 42.0);
        let title_font = self.typo.prose_id(20.0 * self.scale);
        let label_font = self.typo.ui_id(9.5 * self.scale);
        let body_font = self.typo.ui_id(10.5 * self.scale);
        let row_font = self.typo.ui_id(12.5 * self.scale);
        let row_small = self.typo.ui_id(10.5 * self.scale);
        let hint_font = self.typo.ui_id(9.5 * self.scale);
        let field_font = self.typo.code_id(13.0 * self.scale);

        let sheet = egui::Window::new("review-base")
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
                    .inner_margin(Margin::symmetric(16, 14))
                    .shadow(sheet_shadow(&pal)),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().extreme_bg_color = pal.panel_deep;
                ui.spacing_mut().item_spacing.y = 0.0;

                ui.painter().text(
                    ui.min_rect().min,
                    Align2::LEFT_TOP,
                    "Compare changes with",
                    title_font,
                    pal.ink,
                );
                ui.add_space(24.0 * self.scale);
                let mut at = ui.cursor().left_top();
                if let Some(review) = &self.session.changes {
                    paint_tracked(
                        ui.painter(),
                        at,
                        Align2::LEFT_TOP,
                        &format!("ON {}", short_branch(&review.branch).to_uppercase()),
                        label_font.clone(),
                        pal.ink_soft,
                        LABEL_TRACKING * 9.5 * self.scale,
                    );
                    at.y += 17.0 * self.scale;
                    ui.add_space(17.0 * self.scale);
                }
                ui.painter().text(
                    Pos2::new(at.x, at.y),
                    Align2::LEFT_TOP,
                    "A branch is compared from its common ancestor with HEAD.",
                    body_font,
                    pal.ink_soft,
                );
                ui.add_space(22.0 * self.scale);

                for (i, base) in [Base::Working, Base::Staged, Base::LastCommit]
                    .into_iter()
                    .enumerate()
                {
                    let selected = self.session.base == base;
                    let cursor = self.session.base_sel == i;
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                    if selected || cursor || response.hovered() {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(4), pal.selection);
                    }
                    if selected {
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                Pos2::new(rect.left(), rect.top() + 8.0),
                                vec2(2.0, (rect.height() - 16.0).max(1.0)),
                            ),
                            CornerRadius::same(1),
                            pal.accent,
                        );
                    }
                    let (title, detail) = match base {
                        Base::Working => ("Working tree", "uncommitted changes"),
                        Base::Staged => ("Staged", "the index against HEAD"),
                        Base::LastCommit => ("Last commit", "HEAD against its parent"),
                        Base::Ref(_) => ("", ""),
                    };
                    let ink = if selected || cursor || response.hovered() {
                        pal.ink
                    } else {
                        pal.ink_soft
                    };
                    ui.painter().text(
                        Pos2::new(rect.left() + 13.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        title,
                        row_font.clone(),
                        ink,
                    );
                    ui.painter().text(
                        Pos2::new(rect.right() - 8.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        detail,
                        row_small.clone(),
                        pal.ink_soft,
                    );
                    if response.clicked() {
                        choose = Some(base);
                    }
                }

                self.base_branches(ui, &branches, &mut choose, branch_h, screen.height());

                ui.add_space(14.0 * self.scale);
                let y = ui.cursor().top();
                let left = ui.min_rect().left();
                let right = ui.min_rect().right();
                ui.painter()
                    .hline(left..=right, y, Stroke::new(1.0, pal.edge));
                ui.add_space(14.0 * self.scale);

                paint_tracked(
                    ui.painter(),
                    ui.cursor().left_top(),
                    Align2::LEFT_TOP,
                    "A BRANCH OR COMMIT",
                    label_font,
                    pal.ink_soft,
                    LABEL_TRACKING * 9.5 * self.scale,
                );
                ui.add_space(15.0 * self.scale);
                let can = !self.session.reference.trim().is_empty();
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.session.reference)
                            .id(field_id)
                            .hint_text("main, origin/main, 3a1f2c")
                            .font(field_font.clone())
                            .desired_width(ui.available_width() - 62.0)
                            .frame(Frame::NONE),
                    );
                    if focus_field {
                        field.request_focus();
                    }
                    let action = ui.add(
                        egui::Label::new(
                            RichText::new("Review")
                                .font(self.typo.ui_id(11.5 * self.scale).clone())
                                .color(if can { pal.accent } else { pal.ink_faint }),
                        )
                        .sense(Sense::click()),
                    );
                    if action.clicked() && can {
                        custom = true;
                    }
                });
                let y = ui.cursor().top();
                ui.painter().hline(
                    left..=right,
                    y,
                    Stroke::new(1.0, if field_focused { pal.accent } else { pal.edge }),
                );

                if !self.session.error.is_empty() {
                    ui.add_space(8.0 * self.scale);
                    ui.painter().text(
                        ui.cursor().left_top(),
                        Align2::LEFT_TOP,
                        &self.session.error,
                        self.typo.ui_id(10.5 * self.scale),
                        pal.accent,
                    );
                    ui.add_space(14.0 * self.scale);
                } else {
                    ui.add_space(10.0 * self.scale);
                }

                let hint = if field_focused {
                    "enter  review    ·    esc  close"
                } else {
                    "J K  choose    ·    enter  review    ·    tab  type    ·    esc  close"
                };
                ui.painter().text(
                    ui.cursor().left_top(),
                    Align2::LEFT_TOP,
                    hint,
                    hint_font,
                    pal.ink_soft,
                );
                ui.add_space(16.0 * self.scale);
            });

        if custom {
            choose = Some(Base::Ref(self.session.reference.trim().to_string()));
        }
        if close {
            self.session.show_base = false;
            return;
        }
        if let Some(base) = choose {
            self.session.show_base = false;
            self.start_review(base);
            return;
        }
        let sheet_rect = sheet.as_ref().map(|s| s.response.rect);
        if self.frame > self.session.base_opened_frame
            && ctx.input(|i| i.pointer.primary_clicked())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| sheet_rect.is_none_or(|rect| !rect.contains(p)))
        {
            self.session.show_base = false;
        }
    }

    pub(super) fn open_review_index(&mut self, index: usize) {
        let Some(review) = &self.session.changes else {
            return;
        };
        let Some(file) = review.files.get(index).cloned() else {
            return;
        };
        let text = match review.file_text(&file) {
            Ok(text) => text,
            Err(e) => {
                let head = String::new();
                let syntax = self.hl.syntax_for(&file.path, &head).clone();
                self.open = Some(Open {
                    path: file.path.clone(),
                    rel: file.rel.clone(),
                    doc: empty_doc(&syntax),
                    prose: false,
                    outline: Vec::new(),
                    laid: None,
                    error: Some(e.clone()),
                    diff: None,
                    deleted: file.status == crate::review::Status::Deleted,
                });
                self.session.index = index;
                self.session.hover = None;
                self.session.peek = None;
                self.session.return_to = None;
                self.title_dirty = true;
                self.status = e;
                return;
            }
        };
        let mut diff = review.diff_for(&file).ok();
        if let Some(diff) = &mut diff {
            let lines = text.lines().count();
            for deletion in &mut diff.deletions {
                deletion.at = deletion.at.min(lines.saturating_sub(1));
            }
        }
        let head = text.lines().next().unwrap_or("").to_string();
        let syntax = self.hl.syntax_for(&file.path, &head).clone();
        let doc = self.hl.parse(&syntax, &text, text.len());
        let syms = outline::extract(&file.path, &doc.syntax, &doc.lines);
        let prose = outline::family(&file.path, &doc.syntax) == outline::Family::Markdown;
        self.open = Some(Open {
            path: file.path,
            rel: file.rel,
            doc,
            prose,
            outline: syms,
            laid: None,
            error: None,
            diff,
            deleted: file.status == crate::review::Status::Deleted,
        });
        self.session.index = index;
        self.session.hover = None;
        self.session.peek = None;
        self.session.return_to = None;
        self.session.strip_scroll = true;
        self.caret = vim::Caret::default();
        self.pending = Pending::Start;
        self.scroll = 0.0;
        self.title_dirty = true;
    }

    pub(super) fn step_review(&mut self, forward: bool) {
        let Some(review) = &self.session.changes else {
            self.step_file(forward);
            return;
        };
        if review.files.is_empty() {
            return;
        }
        let n = review.files.len();
        let next = if forward {
            (self.session.index + 1) % n
        } else {
            (self.session.index + n - 1) % n
        };
        self.open_review_index(next);
    }

    pub(super) fn review_change(&mut self, forward: bool) {
        let Some(open) = &self.open else { return };
        let Some(diff) = &open.diff else {
            self.status = "this file has no marked changes".into();
            return;
        };
        let Some(line) = diff.change_step(self.caret.line, forward) else {
            return;
        };
        let caret = vim::landing(&open.doc, line);
        self.jump_to(caret);
    }
}

/// The change marks that carry a review without a second colour: a tab in the margin
/// for a changed line, a short rule where code was removed.
pub(super) fn paint_review_marks(
    painter: &egui::Painter,
    laid: &Laid,
    diff: &crate::review::Diff,
    origin_y: f32,
    visible: (f32, f32),
    style: &Style,
    pal: &Palette,
) {
    for &line in &diff.changed {
        let top = origin_y + laid.top_of(line);
        let h = laid.height_of(line);
        if top + h < visible.0 || top > visible.1 {
            continue;
        }
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(laid.col_x - 9.0, top + 4.0),
                vec2(2.0, (h - 8.0).max(2.0)),
            ),
            CornerRadius::same(1),
            pal.accent,
        );
    }
    for deletion in &diff.deletions {
        let line = deletion.at.min(laid.tops.len().saturating_sub(1));
        let y = origin_y + laid.top_of(line);
        if y < visible.0 - 2.0 || y > visible.1 + 2.0 {
            continue;
        }
        painter.hline(
            laid.col_x - 16.0..=laid.col_x + 18.0,
            y,
            Stroke::new(1.0, pal.ink_soft),
        );
        painter.text(
            Pos2::new(laid.col_x - 18.0, y),
            Align2::RIGHT_CENTER,
            "–",
            style.ui.clone(),
            pal.ink_soft,
        );
    }
}

/// Lift the previous code directly under the changed line. It is painted above the page
/// rather than laid into it: the reader's place does not move while they look.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_review_lift(
    ui: &Ui,
    page: Rect,
    laid: &Laid,
    style: &Style,
    pal: &Palette,
    typo: &Typography,
    scale: f32,
    line: usize,
    label: &str,
    old: &[String],
    scroll: f32,
) {
    let line_top = page.top() + laid.top_of(line) - scroll;
    let line_bottom = line_top + laid.height_of(line);
    let width = laid.measure.max(280.0);
    let row = style.code.size * 1.5;
    let shown = old.len().min(7);
    let more = old.len().saturating_sub(shown);
    let height = 30.0 * scale + row * shown as f32 + if more > 0 { row } else { 0.0 };
    let below = line_bottom + 4.0;
    let top = if below + height > page.bottom() - 8.0 && line_top - height - 4.0 > page.top() + 8.0
    {
        line_top - height - 4.0
    } else {
        below
    };
    let rect = Rect::from_min_size(Pos2::new(laid.col_x, top), vec2(width, height));
    if !page.intersects(rect) {
        return;
    }
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("zenith-review-lift"),
        ))
        .with_clip_rect(page);
    painter.rect_filled(rect, 4.0, pal.panel);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, pal.edge),
        egui::StrokeKind::Inside,
    );
    painter.hline(
        rect.x_range(),
        rect.top() + 1.0,
        Stroke::new(2.0, pal.accent),
    );
    painter.text(
        Pos2::new(rect.left() + 12.0, rect.top() + 14.0 * scale),
        Align2::LEFT_TOP,
        label,
        typo.ui_id(9.0 * scale),
        pal.ink_soft,
    );
    let code = typo.code_id(style.code.size * 0.9);
    for (i, line) in old.iter().take(shown).enumerate() {
        painter.text(
            Pos2::new(
                rect.left() + 12.0,
                rect.top() + 28.0 * scale + row * i as f32,
            ),
            Align2::LEFT_TOP,
            line,
            code.clone(),
            pal.ink_soft,
        );
    }
    if more > 0 {
        painter.text(
            Pos2::new(
                rect.left() + 12.0,
                rect.top() + 28.0 * scale + row * shown as f32,
            ),
            Align2::LEFT_TOP,
            format!("… {more} more"),
            typo.ui_id(10.0 * scale),
            pal.ink_soft,
        );
    }
}
