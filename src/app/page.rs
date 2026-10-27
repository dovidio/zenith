//! The reading page: the page itself, its scroll position, and the handful of pure
//! helpers that turn a caret into a block or a requested movement.

use super::review::{paint_review_lift, paint_review_marks};
use super::widgets::*;
use super::*;

impl Zenith {
    /// The page itself: a title block, then the text, typeset.
    pub(super) fn page(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let ctx = ui.ctx().clone();
        if self.hold_down && self.frame > 3 {
            let lines = self.open.as_ref().map(|o| o.doc.lines.len()).unwrap_or(0);
            if self.caret.line + 1 < lines {
                self.motion(vim::Motion::Down(1));
            }
        }
        let pending = std::mem::replace(&mut self.pending, Pending::None);

        // Typeset for this page width before borrowing the document. The foot is left
        // empty so a notice can sit there without setting itself over a line.
        let page_h = (ui.available_height() - PAGE_FOOT).max(1.0);
        let page_rect = Rect::from_min_size(ui.min_rect().min, vec2(ui.available_width(), page_h));
        self.page_center_x = page_rect.center().x;
        self.ensure_layout(&ctx, page_rect);
        let style = self.style();

        let Some(open) = &self.open else {
            // The reading room, empty. One door, no key legend: silence in an empty
            // room is a dead end, but a list of shortcuts is not an invitation either.
            let rect = ui.max_rect();
            self.page_center_x = rect.center().x;
            let title = self.typo.prose_id(22.0 * self.scale);
            let small = self.typo.ui_id(13.0 * self.scale);
            ui.painter().text(
                rect.center() - vec2(0.0, 42.0),
                Align2::CENTER_CENTER,
                "Zenith",
                title,
                pal.ink,
            );
            ui.painter().text(
                rect.center() - vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "a reading room for source code",
                small.clone(),
                pal.ink_soft,
            );
            let label = "Open a folder…";
            let galley = ui
                .painter()
                .layout_no_wrap(label.to_string(), small.clone(), pal.ink);
            let row = Rect::from_center_size(
                rect.center() + vec2(0.0, 38.0),
                vec2(galley.size().x + 28.0, 28.0),
            );
            let response = ui.interact(row, ui.id().with("open-folder"), Sense::click());
            if response.hovered() {
                ui.painter()
                    .rect_filled(row, CornerRadius::same(ROW_RADIUS), pal.selection);
            }
            ui.painter().galley(
                Pos2::new(
                    row.center().x - galley.size().x * 0.5,
                    row.center().y - galley.size().y * 0.5,
                ),
                galley,
                pal.ink,
            );
            if response.clicked() {
                self.pick_folder();
            }
            return;
        };

        if let Some(err) = &open.error {
            let rect = ui.max_rect();
            let font = self.typo.prose_id(18.0 * self.scale);
            let small = self.typo.ui_id(13.0 * self.scale);
            ui.painter().text(
                rect.center() - vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                open.rel.clone(),
                font,
                pal.ink,
            );
            ui.painter().text(
                rect.center() + vec2(0.0, 20.0),
                Align2::CENTER_CENTER,
                err.clone(),
                small,
                pal.ink_soft,
            );
            return;
        }
        let Some(laid) = &open.laid else { return };

        let title = open
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let parent = open.path.parent().map(display_path).unwrap_or_default();
        let badge = if open.deleted || open.diff.as_ref().is_some_and(|d| d.deleted_file) {
            Some((
                format!(
                    "deleted from {}",
                    self.review
                        .as_ref()
                        .map(|r| r.base.label())
                        .unwrap_or_else(|| "the base".into())
                ),
                pal.accent,
            ))
        } else if open.diff.as_ref().is_some_and(|d| d.added_file) {
            Some(("new file".to_string(), pal.accent))
        } else {
            self.store.is_favorite(&open.path).then_some((
                format!("{} bookmarked", self.typo.marks.pick("✦")),
                pal.accent,
            ))
        };

        let content_w = ui.available_width().max(laid.col_x + laid.measure + 40.0);

        let diff = open.diff.clone();
        let remember = self.store.position_of(&open.path);
        let view_top = self.scroll;
        let view_h = self.view_h;
        let mut cursor_hint: Option<usize> = None;
        // The furthest the page can scroll, which is a fact about the content, not about
        // what some motion would like. Asking for more than this is how the page ended up
        // straining against the end of the file: the request is granted for a frame (the
        // content is drawn shifted) and then corrected, so the bottom flickers. Nothing
        // below may exceed this.
        let max_scroll = (laid.total - view_h).max(0.0);

        let target: Option<f32> = match pending {
            Pending::None => None,
            Pending::Restore => {
                let y = laid.y_for_frac(remember);
                // The page comes back where it was left, and so does the cursor: a
                // reader returns to a *place*, not to a byte offset.
                cursor_hint = Some(laid.line_at(y));
                Some(y)
            }
            Pending::Centre(line) => {
                Some((laid.top_of(line) + laid.height_of(line) * 0.5 - view_h * 0.5).max(0.0))
            }
            Pending::Top(line) => Some(laid.top_of(line).max(0.0)),
            Pending::Bottom(line) => {
                Some((laid.top_of(line) + laid.height_of(line) - view_h).max(0.0))
            }
            // `gg` and `G` mean the ends of the *page*, not the ends of the text: the
            // title block sits above the first line, and stopping short of it leaves a
            // few pixels that cannot be scrolled to on purpose but can by hand.
            Pending::Start => Some(0.0),
            Pending::End => Some(max_scroll),
            Pending::Reveal(line) => {
                let pad = laid.row_h * 2.5;
                let top = laid.top_of(line);
                let bottom = top + laid.height_of(line);
                if top < view_top + pad {
                    Some((top - pad).max(0.0))
                } else if bottom > view_top + view_h - pad {
                    Some(bottom + pad - view_h)
                } else {
                    None // already in view: do not move the page under the reader
                }
            }
        }
        // Clamp, then ignore a request that would move the page by less than a pixel:
        // a sub-pixel nudge per frame reads as a shimmer, and it also keeps the app
        // repainting forever at the end of a file.
        .map(|y| y.clamp(0.0, max_scroll))
        .filter(|y| view_h > 1.0 && (y - self.scroll).abs() >= 0.75);

        let mut review_hover: Option<usize> = None;
        let mut area = ScrollArea::vertical()
            .id_salt("zenith-page")
            .max_height(page_h)
            // The reading pane has its own indicator — the spine at the edge of the paper —
            // so egui's floating scrollbar is a second bar at a second x, and the two never
            // lined up. The spine is draggable instead, which is what the bar was for.
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false]);
        if let Some(y) = target {
            area = area.vertical_scroll_offset(y.max(0.0));
        }
        if debug() {
            eprintln!(
                "zenith[page]: caret={} scroll={:.0} view_h={:.0} total={:.0} target={target:?} row_h={:.1} measure={:.0} jobs={}",
                self.caret.line,
                self.scroll,
                self.view_h,
                laid.total,
                laid.row_h,
                laid.measure,
                laid.jobs.len()
            );
        }

        let focus = if self.store.config.focus_lens {
            Some(focus_block(open, self.caret.line))
        } else {
            None
        };
        let caret = self.caret;

        let mut clicked: Option<vim::Caret> = None;
        let out = area.show_viewport(ui, |ui, viewport| {
            self.view_h = viewport.height();
            let size = vec2(content_w, laid.total);
            let (id, rect) = ui.allocate_space(size);
            // `rect` is already positioned at the content origin, which the scroll
            // area has moved up by `viewport.min.y`. Subtracting the offset again
            // would draw the whole page thousands of pixels off-screen.
            let origin_y = rect.top();
            let painter = ui.painter();

            // Clicking a line puts the reading cursor there. There is no caret: the
            // cursor marks where you are reading, it does not select.
            //
            // Two deliberate details. The interaction rect is only the *visible* area,
            // and it senses hover rather than clicks: a clickable widget spanning the
            // whole document becomes the focused widget, and egui then dutifully scrolls
            // the page to keep it visible — the document would drift on its own.
            let vis = ui.clip_rect();
            let resp = ui.interact(vis, id, Sense::hover());
            if resp.hovered() {
                if let Some(pos) = ui.input(|i| {
                    i.pointer
                        .primary_clicked()
                        .then(|| i.pointer.interact_pos())
                        .flatten()
                }) {
                    // A click picks both the line and the column, so a word or a
                    // bracket can be aimed at.
                    let line = laid.line_at(pos.y - origin_y);
                    let indent = laid.indent_px(open.doc.lines[line].indent);
                    let into = (pos.x - (laid.col_x + indent)).max(0.0);
                    let col = (into / laid.char_w.max(1.0)).round() as usize;
                    clicked = Some(vim::Caret::new(line, col));
                }
            }

            reader::paint_header(
                painter,
                &style,
                &pal,
                laid.col_x,
                origin_y + BLOOM_ROOM,
                &title,
                &parent,
                badge,
            );

            let frame_ctx = ui.ctx().clone();
            let paint = Paint {
                ctx: &frame_ctx,
                laid,
                doc: &open.doc,
                style: &style,
                palette: &pal,
                origin_y,
                visible: (viewport.min.y, viewport.max.y),
                caret,
                focus,
                fade_alpha: 176,
                zen: false,
                show_cursor: true,
            };
            reader::paint(painter, &paint);
            if let Some(diff) = &diff {
                paint_review_marks(
                    painter,
                    laid,
                    diff,
                    origin_y,
                    (viewport.min.y, viewport.max.y),
                    &style,
                    &pal,
                );
            }
            if resp.hovered() {
                if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                    let line = laid.line_at(pos.y - origin_y);
                    if let Some(diff) = &diff {
                        if (diff.changed_at(line) && diff.old_for(line).is_some())
                            || diff.deletion_at(line).is_some()
                        {
                            review_hover = Some(line);
                        }
                    }
                }
            }
            rect
        });

        self.scroll = out.state.offset.y;
        // A development affordance for photographing the lift without a pointer.
        let pinned_hover = std::env::var("ZENITH_REVIEW_HOVER")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map(|n| n.saturating_sub(1));
        self.review_hover = review_hover.or(self.review_peek).or(pinned_hover);
        if let (Some(line), Some(diff)) = (self.review_hover, diff.as_ref()) {
            let detail = if let Some(deletion) = diff.deletion_at(line) {
                Some(("REMOVED", deletion.old.as_slice()))
            } else {
                diff.old_for(line).map(|old| ("PREVIOUSLY", old))
            };
            if let Some((label, old)) = detail {
                paint_review_lift(
                    ui,
                    page_rect,
                    laid,
                    &style,
                    &pal,
                    &self.typo,
                    self.scale,
                    line,
                    label,
                    old,
                    self.scroll,
                );
            }
        }
        if let Some(line) = cursor_hint {
            self.caret = self
                .open
                .as_ref()
                .map(|o| vim::landing(&o.doc, line))
                .unwrap_or_default();
        }
        if let Some(caret) = clicked {
            self.caret = caret;
            // A click on the page is also the way back from a summoned drawer.
            self.show_panels = false;
            self.show_outline = false;
            self.tree_focus = false;
        }

        // The spine stays with the page, not with the text: it is a bookmark, not a
        // scrollbar, so it lives at the edge of the paper. It is also draggable, which is
        // what a scrollbar was for — so there is one indicator here rather than two.
        let spine_x = page_rect.right() - 16.0;
        let spine_top = page_rect.top();
        let spine_h = page_rect.height();
        if let Some(diff) = &diff {
            for &line in &diff.changed {
                let frac = (laid.top_of(line) / laid.total.max(1.0)).clamp(0.0, 1.0);
                let y = spine_top + frac * spine_h;
                ui.painter().rect_filled(
                    Rect::from_min_size(Pos2::new(spine_x - 4.0, y - 1.0), vec2(9.0, 2.0)),
                    CornerRadius::same(1),
                    pal.accent,
                );
            }
            for deletion in &diff.deletions {
                let line = deletion.at.min(laid.tops.len().saturating_sub(1));
                let frac = (laid.top_of(line) / laid.total.max(1.0)).clamp(0.0, 1.0);
                let y = spine_top + frac * spine_h;
                ui.painter().rect_filled(
                    Rect::from_min_size(Pos2::new(spine_x - 2.0, y - 2.0), vec2(5.0, 4.0)),
                    CornerRadius::same(1),
                    pal.ink_soft,
                );
            }
        }
        let grab = Rect::from_min_max(
            Pos2::new(spine_x - 7.0, spine_top),
            Pos2::new(spine_x + 7.0, spine_top + spine_h),
        );
        let handle = ui.interact(grab, ui.id().with("zenith-spine"), Sense::click_and_drag());
        if handle.dragged() || handle.clicked() {
            if let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos()) {
                let frac = ((pos.y - spine_top) / spine_h).clamp(0.0, 1.0);
                let y = self.frac_to_scroll(frac);
                let landing = self
                    .open
                    .as_ref()
                    .and_then(|o| o.laid.as_ref().map(|l| l.line_at(y)));
                if let Some(line) = landing {
                    if let Some(open) = self.open.as_ref() {
                        let caret = vim::landing(&open.doc, line);
                        self.jump_to(caret);
                    }
                }
            }
        }
        let frac = self.read_fraction();
        reader::paint_spine(ui.painter(), spine_x, spine_top, spine_h, &pal, frac);
    }

    /// How far down the file the page can go: the one range both directions use.
    ///
    /// It used to be `total - four rows` for the reading position and `total - view` for
    /// what scrolling actually allows, which are different numbers — so the mark saturated
    /// before the page reached its end and never arrived at the bottom.
    pub(super) fn scroll_range(&self) -> Option<(f32, f32)> {
        let open = self.open.as_ref()?;
        let laid = open.laid.as_ref()?;
        let max = (laid.total - self.view_h).max(0.0);
        Some((self.scroll, max))
    }

    /// A place in the file as a fraction of it, for the spine.
    pub(super) fn read_fraction(&self) -> Option<f32> {
        let (scroll, max) = self.scroll_range()?;
        (max > 0.0).then(|| (scroll / max).clamp(0.0, 1.0))
    }

    /// The inverse: a place on the spine is a place in the file.
    pub(super) fn frac_to_scroll(&self, frac: f32) -> f32 {
        match self.scroll_range() {
            Some((_, max)) => frac.clamp(0.0, 1.0) * max,
            None => 0.0,
        }
    }
}

pub(super) fn focus_block(open: &Open, cursor: usize) -> (usize, usize) {
    let n = open.doc.lines.len();
    if n == 0 {
        return (0, 0);
    }
    let syms = &open.outline;
    if !syms.is_empty() {
        // One backward pass: each symbol ends just before the next one that is not
        // nested inside it.
        let mut ends = vec![n - 1; syms.len()];
        let mut stack: Vec<usize> = Vec::new();
        for (i, s) in syms.iter().enumerate() {
            while let Some(&top) = stack.last() {
                if syms[top].depth >= s.depth {
                    ends[top] = s.line.saturating_sub(1);
                    stack.pop();
                } else {
                    break;
                }
            }
            stack.push(i);
        }
        let mut best: Option<(usize, usize, usize)> = None;
        for (i, s) in syms.iter().enumerate() {
            let end = ends[i];
            if s.line <= cursor && cursor <= end && end > s.line {
                let deeper = best.is_none_or(|(d, _, _)| s.depth >= d);
                if deeper {
                    best = Some((s.depth, s.line, end));
                }
            }
        }
        if let Some((_, a, b)) = best {
            // A one-line block is not worth dimming the world for.
            if b > a + 1 {
                return (a, b);
            }
        }
    }
    paragraph_block(&open.doc, cursor)
}

/// A paragraph of code: the run of lines at or deeper than the cursor's indentation,
/// bounded by a blank line or a dedent.
pub(super) fn paragraph_block(doc: &Document, line: usize) -> (usize, usize) {
    let n = doc.lines.len();
    if n == 0 {
        return (0, 0);
    }
    let mut i = line.min(n - 1);
    while i > 0 && doc.lines[i].is_blank() {
        i -= 1;
    }
    let base = doc.lines[i].indent;

    let mut start = i;
    let mut j = i;
    while j > 0 {
        let p = j - 1;
        if doc.lines[p].is_blank() || doc.lines[p].indent < base {
            break;
        }
        start = p;
        j = p;
    }

    let mut end = i;
    let mut k = i;
    while k + 1 < n {
        let nx = k + 1;
        if doc.lines[nx].is_blank() || doc.lines[nx].indent < base {
            break;
        }
        end = nx;
        k = nx;
    }
    (start, end)
}

/// What a caret at `line` needs from the page. The ends of the document ask for the
/// ends of the page; everything between asks only to be revealed.
pub(super) fn reveal_pending(line: usize, last: usize) -> Pending {
    if line == 0 {
        Pending::Start
    } else if last > 0 && line >= last {
        Pending::End
    } else {
        Pending::Reveal(line)
    }
}

/// A pending movement of the page.
#[derive(Clone, Copy)]
pub(super) enum Pending {
    None,
    /// Return to the position this file was last left at.
    Restore,
    /// Bring this line into view with the least movement.
    Reveal(usize),
    /// Put this line in the middle of the page.
    Centre(usize),
    /// Put this line at the top of the page.
    Top(usize),
    /// Put this line at the bottom of the page.
    Bottom(usize),
    /// The very top of the page, title block and all.
    Start,
    /// The very bottom.
    End,
}
