//! Small drawn things: the row, the check, the slider, the window buttons.
//!
//! None of them knows what it is for. They take a palette and a rectangle and draw;
//! the sheets and panels decide which one belongs where.

use super::*;

/// The theme sample, toned by the grammar zenith already has loaded.
pub(super) fn sample_tokens(hl: &Highlighter) -> Vec<SampleLine> {
    let path = PathBuf::from("sample.rs");
    let syntax = hl.syntax_for(&path, "").clone();
    hl.parse(&syntax, THEME_SAMPLE, THEME_SAMPLE.len())
        .lines
        .iter()
        .map(|line| {
            (
                line.indent,
                line.tokens
                    .iter()
                    .map(|t| (t.text.clone(), t.tone))
                    .collect(),
            )
        })
        .collect()
}

/// One theme, shown by being used: its paper, its inks, its faces, and the sample set in
/// them. `chosen` draws the accent around it rather than a fill, so the preview is never
/// tinted by the fact that it is the one in force.
#[derive(Clone, Copy, Default)]
pub(super) struct TileState {
    pub(super) chosen: bool,
    pub(super) focused: bool,
}

pub(super) fn theme_tile(
    ui: &egui::Ui,
    rect: Rect,
    palette: &Palette,
    sample: &[SampleLine],
    state: TileState,
    typo: &Typography,
    scale: f32,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(TILE_RADIUS), palette.paper);
    painter.rect_stroke(
        rect,
        CornerRadius::same(TILE_RADIUS),
        Stroke::new(
            if state.chosen || state.focused {
                1.5
            } else {
                1.0
            },
            if state.focused {
                palette.ink_soft
            } else if state.chosen {
                palette.accent
            } else {
                palette.edge
            },
        ),
        egui::StrokeKind::Inside,
    );

    let pad = 9.0;
    painter.text(
        Pos2::new(rect.left() + pad, rect.top() + 5.0),
        Align2::LEFT_TOP,
        palette.name,
        typo.prose_id(12.0 * scale),
        palette.ink_soft,
    );

    let code = typo.code_id(10.0 * scale);
    let prose = typo.prose_italic_id(10.0 * scale);
    let char_w = painter
        .layout_no_wrap("M".to_string(), code.clone(), palette.ink)
        .size()
        .x;
    let step = 13.0 * scale;
    let mut y = rect.top() + 25.0;
    for (indent, tokens) in sample {
        if y + step > rect.bottom() - 4.0 {
            break;
        }
        let mut x = rect.left() + pad + *indent as f32 * char_w;
        for (text, tone) in tokens {
            let ink = palette.tone(*tone);
            let font = if matches!(tone, Tone::Comment | Tone::Doc) {
                prose.clone()
            } else {
                code.clone()
            };
            let galley = painter.layout_no_wrap(text.clone(), font, ink);
            // A tile is not a page: a line that does not fit simply stops.
            if x + galley.size().x > rect.right() - 6.0 {
                break;
            }
            painter.galley(Pos2::new(x, y), galley.clone(), ink);
            x += galley.size().x;
        }
        y += step;
    }
}

/// One of the page's numbers: what it is called, where it is stored, and how to write it.
pub(super) struct Slider<'a> {
    pub(super) label: &'a str,
    pub(super) value: &'a mut f32,
    pub(super) range: std::ops::RangeInclusive<f32>,
    pub(super) decimals: usize,
}

/// A slider for a page made of hairlines.
///
/// egui's own is a pill on a rail with the number in a rounded box — the one piece of
/// chrome in this program that looks like a form. This one is the house style: the label
/// and its value on one line, the value set in the code face where it can be read, and a
/// rule under them filled in the accent up to the hand.
pub(super) fn line_slider(
    ui: &mut Ui,
    pal: &Palette,
    label_font: &egui::FontId,
    value_font: &egui::FontId,
    spec: Slider<'_>,
    focused: bool,
) -> bool {
    let Slider {
        label,
        value,
        range,
        decimals,
    } = spec;
    let (low, high) = (*range.start(), *range.end());
    let width = ui.available_width();
    let height = (label_font.size * 2.35).max(30.0);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, height), egui::Sense::click_and_drag());

    let mut changed = false;
    if response.dragged() || response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let t = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
            let wanted = low + t * (high - low);
            if (wanted - *value).abs() > f32::EPSILON {
                *value = wanted;
                changed = true;
            }
        }
    }

    let painter = ui.painter();
    if focused {
        painter.rect_stroke(
            rect.shrink(1.0),
            CornerRadius::same(ROW_RADIUS),
            Stroke::new(1.0, pal.accent),
            egui::StrokeKind::Inside,
        );
    }
    let text = format!("{value:.*}", decimals);
    let galley = painter.layout_no_wrap(text, value_font.clone(), pal.ink);
    let value_w = galley.size().x;
    painter.text(
        Pos2::new(rect.left(), rect.top()),
        Align2::LEFT_TOP,
        label,
        label_font.clone(),
        pal.ink_soft,
    );
    painter.galley(
        Pos2::new(rect.right() - value_w, rect.top()),
        galley,
        pal.ink,
    );

    // The track: a rule across, filled to the hand, with the hand a tick on it.
    let y = rect.bottom() - 8.0;
    let fraction = ((*value - low) / (high - low)).clamp(0.0, 1.0);
    let hand = if response.hovered() || response.dragged() {
        pal.accent
    } else {
        pal.ink_soft
    };
    painter.hline(rect.x_range(), y, Stroke::new(1.0, pal.edge));
    painter.hline(
        rect.left()..=(rect.left() + fraction * rect.width()),
        y,
        Stroke::new(1.5, pal.accent),
    );
    painter.vline(
        rect.left() + fraction * rect.width(),
        egui::Rangef::new(y - 5.0, y + 5.0),
        Stroke::new(1.5, hand),
    );
    changed
}

/// A column of rendered checkboxes, drawn as one grid so the settings panel stays short.
pub(super) fn toggles(
    ui: &mut Ui,
    font: &FontId,
    pal: &Palette,
    rows: &mut [(&str, &mut bool)],
    focus: Option<usize>,
    base: &mut usize,
) -> bool {
    let mut changed = false;
    for (label, value) in rows.iter_mut() {
        let focused = focus == Some(*base);
        *base += 1;
        changed |= check(ui, font, pal, label, value, focused);
    }
    changed
}

/// One statement that is either true or false.
///
/// Not egui's checkbox: that is a filled blob that reads as a pill. These are paper
/// statements — a hairline square when off, the same square with an ink check when on.
/// A circle would mean "choose one of these"; these are independent.
pub(super) fn check(
    ui: &mut Ui,
    font: &FontId,
    pal: &Palette,
    label: &str,
    value: &mut bool,
    focused: bool,
) -> bool {
    let row_h = (font.size * 1.65).max(20.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(ROW_RADIUS),
            theme::alpha(pal.ink, 10),
        );
    }
    if focused {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            CornerRadius::same(ROW_RADIUS),
            Stroke::new(1.0, pal.accent),
            egui::StrokeKind::Inside,
        );
    }

    let side = (font.size * 0.88).max(11.0);
    let box_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 1.0, rect.center().y - side * 0.5),
        vec2(side, side),
    );
    let stroke = if *value {
        theme::alpha(pal.ink, 160)
    } else if hovered {
        pal.ink_soft
    } else {
        theme::alpha(pal.ink, 70)
    };
    ui.painter().rect_stroke(
        box_rect,
        CornerRadius::same(2),
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    if *value {
        let c = box_rect.center();
        let ink = Stroke::new(1.1, pal.ink);
        ui.painter()
            .line_segment([c + vec2(-2.8, -0.2), c + vec2(-0.7, 2.0)], ink);
        ui.painter()
            .line_segment([c + vec2(-0.7, 2.0), c + vec2(2.8, -2.2)], ink);
    }
    ui.painter().text(
        Pos2::new(box_rect.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font.clone(),
        if *value { pal.ink } else { pal.ink_soft },
    );

    if resp.clicked() {
        *value = !*value;
        true
    } else {
        false
    }
}

/// Keep a branch name in the review strip without letting it push the count off the
/// page. The full name is still in the base sheet.
pub(super) fn short_branch(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= 34 {
        return name.to_string();
    }
    let head: String = chars[..18].iter().collect();
    let tail: String = chars[chars.len() - 12..].iter().collect();
    format!("{head}…{tail}")
}

/// How many lines does this text have? Cheap enough to ask once when opening.
pub(super) fn count_lines(text: &str) -> usize {
    text.lines().count()
}

// The two sidebars are one system, so they share a frame and a rhythm: the same inner
// margin, the same drop for the label, the same gap between that label and the list, the
// same row height and the same type size. Otherwise the eye reads them as two unrelated
// columns that happen to be adjacent — which is what "not vertically aligned" means.
pub(super) const PANEL_MARGIN_X: i8 = 8;
pub(super) const PANEL_MARGIN_Y: i8 = 8;
pub(super) const PANEL_LABEL_DROP: f32 = 7.0;
pub(super) const PANEL_LABEL_GAP: f32 = 30.0;
pub(super) const PANEL_ROW_H: f32 = 22.0;
pub(super) const PANEL_ROW_FONT: f32 = 12.0;
pub(super) const PANEL_LABEL_FONT: f32 = 10.0;

/// How wide three window buttons are, so the dragging area can stop before them.
pub(super) fn window_buttons_width() -> f32 {
    3.0 * (12.0 + 10.0) + 8.0
}

/// Does the app have to draw the window's buttons itself?
///
/// On macOS the system keeps them, floating over the transparent title bar; everywhere
/// else a window without a frame would have none at all, and no way to close.
pub(super) fn own_window_buttons() -> bool {
    !cfg!(target_os = "macos")
}

/// Minimise, maximise, close — drawn by us, because on Linux and Windows a window without
/// a system frame has no buttons at all.
pub(super) fn window_buttons(ui: &mut Ui, pal: &Palette, ctx: &egui::Context, bar: Rect) {
    let size = 12.0;
    let gap = 10.0;
    let mut x = bar.right() - 16.0;
    let y = bar.center().y;
    let mut button = |glyph: &str, colour: Color32, action: egui::ViewportCommand| {
        let rect = Rect::from_center_size(Pos2::new(x, y), vec2(size + gap, size + gap));
        let resp = ui.interact(rect, ui.id().with(glyph), Sense::click());
        let ink = if resp.hovered() { pal.ink } else { colour };
        let painter = ui.painter().with_clip_rect(rect);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(11.0),
            ink,
        );
        if resp.clicked() {
            ctx.send_viewport_cmd(action);
        }
        x -= size + gap;
    };
    button("✕", pal.ink_soft, egui::ViewportCommand::Close);
    button("＋", pal.ink_soft, egui::ViewportCommand::Maximized(true));
    button("—", pal.ink_soft, egui::ViewportCommand::Minimized(true));
}

/// The window's edges, for a window with no system frame: eight thin zones that ask the
/// backend to begin a resize when one of them is dragged.
///
/// Drawn last, on top of everything, so a press near the edge resizes the window rather
/// than clicking whatever happened to be underneath.
pub(super) fn resize_handles(ui: &mut Ui) {
    use egui::viewport::ResizeDirection as Dir;
    let edge = 6.0;
    let r = ui.ctx().content_rect();
    let zones: [(Dir, Rect); 8] = [
        (
            Dir::North,
            Rect::from_min_max(r.left_top(), Pos2::new(r.right(), r.top() + edge)),
        ),
        (
            Dir::South,
            Rect::from_min_max(Pos2::new(r.left(), r.bottom() - edge), r.right_bottom()),
        ),
        (
            Dir::West,
            Rect::from_min_max(r.left_top(), Pos2::new(r.left() + edge, r.bottom())),
        ),
        (
            Dir::East,
            Rect::from_min_max(Pos2::new(r.right() - edge, r.top()), r.right_bottom()),
        ),
        (
            Dir::NorthWest,
            Rect::from_min_max(r.left_top(), Pos2::new(r.left() + edge, r.top() + edge)),
        ),
        (
            Dir::NorthEast,
            Rect::from_min_max(
                Pos2::new(r.right() - edge, r.top()),
                Pos2::new(r.right(), r.top() + edge),
            ),
        ),
        (
            Dir::SouthWest,
            Rect::from_min_max(
                Pos2::new(r.left(), r.bottom() - edge),
                Pos2::new(r.left() + edge, r.bottom()),
            ),
        ),
        (
            Dir::SouthEast,
            Rect::from_min_max(
                Pos2::new(r.right() - edge, r.bottom() - edge),
                r.right_bottom(),
            ),
        ),
    ];
    for (dir, rect) in zones {
        let id = egui::Id::new(("zenith-resize", dir as u8));
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        if resp.drag_started() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }
}

/// A disclosure triangle, drawn rather than typed.
///
/// The file tree used to put `▸` and `▾` in a text run. Those exist in Apple's fonts and
/// in whichever symbol font a Linux distribution happens to ship — so on some machines
/// the tree had no carets at all. A path is 10 lines and cannot go missing.
pub(super) fn caret(painter: &egui::Painter, centre: Pos2, open: bool, colour: Color32) {
    let r = 4.0;
    let pts = if open {
        vec![
            centre + vec2(-r, -r * 0.5),
            centre + vec2(r, -r * 0.5),
            centre + vec2(0.0, r * 0.8),
        ]
    } else {
        vec![
            centre + vec2(-r * 0.5, -r),
            centre + vec2(-r * 0.5, r),
            centre + vec2(r * 0.8, 0.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, colour, Stroke::NONE));
}

/// One clickable line in a panel. Zenith draws its own rows so the interface can be
/// typeset (faint marks, hairline selection) rather than widgeted.
#[allow(clippy::too_many_arguments)]
pub(super) fn row(
    ui: &mut Ui,
    pal: &Palette,
    text: &str,
    glyph: &str,
    depth: usize,
    selected: bool,
    ink: Color32,
    font: &FontId,
    right: Option<&str>,
    // `Some(open)` draws a disclosure caret as a triangle. Typed `▸`/`▾` are not
    // dependable: they are in Apple's fonts and in whichever symbol font a Linux
    // distribution happens to ship, and on some machines the tree had no carets at all.
    caret_open: Option<bool>,
) -> egui::Response {
    let (_rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW_H), Sense::click());
    let rect = resp.rect;
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), pal.selection);
        // The same mark the page puts beside the line you are on, in miniature: a
        // bookmark tab, so the eye finds the active row before it reads it.
        ui.painter().rect_filled(
            Rect::from_min_max(
                Pos2::new(rect.left(), rect.top() + 2.0),
                Pos2::new(rect.left() + 2.0, rect.bottom() - 2.0),
            ),
            CornerRadius::same(1),
            theme::alpha(pal.accent, 190),
        );
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme::alpha(pal.ink, 10));
    }

    let mut x = rect.left() + 4.0 + depth as f32 * 12.0;
    let y = rect.center().y;
    let clip = rect.shrink(2.0);
    let painter = ui.painter().with_clip_rect(clip);

    if let Some(open) = caret_open {
        caret(&painter, Pos2::new(x + 5.0, y), open, pal.ink_faint);
        x += 16.0;
    } else if !glyph.is_empty() {
        let g = painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            glyph,
            font.clone(),
            pal.ink_faint,
        );
        x = g.right() + 6.0;
    }
    let right_w = right
        .map(|r| r.chars().count() as f32 * font.size * 0.60 + 10.0)
        .unwrap_or(6.0);
    let avail = (rect.right() - right_w - x).max(10.0);
    // Both panels are set in the code face, whose advance is 0.6 em.
    let max_chars = (avail / (font.size * 0.60)) as usize;
    let label = truncate(text, max_chars);
    let cut = label != text;
    painter.text(
        Pos2::new(x, y),
        Align2::LEFT_CENTER,
        &label,
        font.clone(),
        ink,
    );
    if let Some(r) = right {
        painter.text(
            Pos2::new(rect.right() - 4.0, y),
            Align2::RIGHT_CENTER,
            r,
            font.clone(),
            pal.ink_faint,
        );
    }
    // A name the panel could not fit is still a name the reader needs. Hovering is the
    // only way to ask for it, and it costs nothing when the row already fits.
    if cut {
        resp.on_hover_text(RichText::new(text).font(font.clone()))
    } else {
        resp
    }
}

pub(super) fn truncate(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Walk up from a file to the repository (or project) that contains it.
/// The directory zenith was launched from, if it looks like a project.
///
/// With no argument and no memory of a last folder, a reader who typed `zenith` inside a
/// repository means that repository. Anywhere else — a home directory, a desktop icon,
/// `/` — the reading room opens empty instead of walking something enormous.
pub(super) fn working_project() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let markers = [
        ".git",
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "pom.xml",
        "Gemfile",
        "CMakeLists.txt",
    ];
    markers.iter().any(|m| cwd.join(m).exists()).then_some(cwd)
}

pub(super) fn read_head(path: &Path) -> String {
    std::fs::read(path)
        .ok()
        .map(|b| {
            let n = b.len().min(4096);
            String::from_utf8_lossy(&b[..n])
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default()
}

pub(super) fn empty_doc(syntax: &syntect::parsing::SyntaxReference) -> Document {
    Document {
        syntax: syntax.name.clone(),
        md: None,
        lines: Vec::new(),
        truncated: false,
        lossy: false,
        bytes: 0,
    }
}

/// Can this key be a command while the reader is typing into a text box?
///
/// Only if it carries a modifier, or is Escape. A plain letter binding and a text box want
/// the same key, and the text box has to win: otherwise the letter is unusable.
pub(super) fn survives_text_entry(key: Key, cmd: bool, ctrl: bool) -> bool {
    key == Key::Escape
        || (cmd && matches!(key, Key::O | Key::K | Key::P | Key::F | Key::Comma))
        || (ctrl && matches!(key, Key::P | Key::F | Key::O | Key::I))
}

/// Should this key be considered a command at all?
///
/// With a text box focused, only the modified ones (and Escape). With a *panel* open —
/// settings, help — every command still works, so `s` closes the settings panel it
/// opened, and `?` closes the help.
pub(super) fn commands_allowed(key: Key, cmd: bool, ctrl: bool, text_entry: bool) -> bool {
    !text_entry || survives_text_entry(key, cmd, ctrl)
}

/// Fill in the modifier keys for this platform.
///
/// `⌘` and `⇧` live in Apple's fonts and in almost no others, and a reader on Linux has
/// a Ctrl key anyway — so the labels are written once, here, and translated per platform.
pub(super) fn keys(text: &str) -> String {
    text.replace("{mod}", typography::mod_label())
        .replace("{shift}", typography::shift_label())
}

/// The character a key stands for, for two-key sequences. Only the keys that can
/// follow `g` or `z` matter, but mapping the alphabet costs nothing and avoids a
/// surprise when another chord is added.
pub(super) fn key_char(key: Key) -> Option<char> {
    use Key::*;
    Some(match key {
        A => 'a',
        B => 'b',
        C => 'c',
        D => 'd',
        E => 'e',
        F => 'f',
        G => 'g',
        H => 'h',
        I => 'i',
        J => 'j',
        K => 'k',
        L => 'l',
        M => 'm',
        N => 'n',
        O => 'o',
        P => 'p',
        Q => 'q',
        R => 'r',
        S => 's',
        T => 't',
        U => 'u',
        V => 'v',
        W => 'w',
        X => 'x',
        Y => 'y',
        Z => 'z',
        _ => return None,
    })
}

/// `ZENITH_DEBUG=1` prints what the page and the key reader decide each frame. Reading
/// is a quiet activity; being able to see the machinery is occasionally essential.
pub(super) fn debug() -> bool {
    use std::sync::OnceLock;
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var("ZENITH_DEBUG").is_ok())
}

pub(super) fn dropped_folder(ctx: &egui::Context) -> Option<PathBuf> {
    ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .find_map(|f| f.path.clone())
            .filter(|p| p.is_dir())
    })
}

/// The "movement" the cursor is inside — what the focus lens keeps at full strength.
///
/// Preference order matters. A file with an outline gives us real blocks: a symbol
/// runs from its own line to the line before the next symbol at the same or shallower
/// depth. Files without structure fall back to paragraphs, delimited by blank lines
/// and dedents. The one thing this must never do is return the whole file, or the lens
/// does nothing at all.
/// Turn a palette into egui's own visual language: same restraint, no chrome.
pub(super) fn apply_style(ctx: &egui::Context, pal: &Palette, typo: &Typography, scale: f32) {
    let mut visuals = if pal.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = pal.panel;
    visuals.window_fill = pal.panel;
    visuals.extreme_bg_color = pal.panel_deep;
    visuals.faint_bg_color = theme::alpha(pal.ink, 8);
    visuals.window_stroke = Stroke::new(1.0, pal.edge);
    visuals.window_corner_radius = CornerRadius::same(SHEET_RADIUS);
    visuals.selection.bg_fill = theme::alpha(pal.accent, 60);
    visuals.selection.stroke = Stroke::new(1.0, pal.accent);
    visuals.hyperlink_color = pal.accent;
    // Hint text is text: `ink_faint` is for marks, not for instructions.
    visuals.weak_text_color = Some(pal.ink_soft);
    visuals.override_text_color = None;
    visuals.widgets.noninteractive.bg_fill = pal.panel;
    visuals.widgets.noninteractive.weak_bg_fill = pal.panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.edge);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.ink_soft);
    visuals.widgets.inactive.bg_fill = pal.panel_deep;
    visuals.widgets.inactive.weak_bg_fill = theme::alpha(pal.ink, 10);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, theme::alpha(pal.ink, 26));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.ink_soft);
    visuals.widgets.hovered.bg_fill = theme::alpha(pal.accent, 26);
    visuals.widgets.hovered.weak_bg_fill = theme::alpha(pal.accent, 34);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.accent_soft);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.ink);
    visuals.widgets.active.bg_fill = theme::alpha(pal.accent, 48);
    visuals.widgets.active.weak_bg_fill = theme::alpha(pal.accent, 48);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, pal.ink);
    visuals.widgets.open.weak_bg_fill = theme::alpha(pal.accent, 20);
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(4);
    }
    ctx.set_visuals(visuals);

    let code = 15.0 * scale;
    ctx.all_styles_mut(|s| {
        s.text_styles
            .insert(TextStyle::Body, typo.ui_id(13.5 * scale));
        s.text_styles
            .insert(TextStyle::Button, typo.ui_id(13.0 * scale));
        s.text_styles
            .insert(TextStyle::Small, typo.ui_id(11.5 * scale));
        s.text_styles
            .insert(TextStyle::Monospace, typo.code_id(code));
        s.text_styles
            .insert(TextStyle::Heading, typo.prose_id(code * 1.5));
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(7.0, 3.0);
        s.spacing.window_margin = Margin::same(10);
    });
}

#[cfg(test)]
mod key_tests {
    use super::{commands_allowed, survives_text_entry};
    use egui::Key;

    #[test]
    fn letters_are_typeable_in_a_text_box() {
        // The whole point: every plain letter must reach the search box, so no
        // unmodified letter may be treated as a command while typing.
        // Every plain letter that is bound to something, whether its binding is a
        // command (checked by this predicate) or a motion (skipped by the `text_entry`
        // guard a few lines below it in the key pass).
        for key in [
            Key::S,
            Key::N,
            Key::O,
            Key::W,
            Key::B,
            Key::E,
            Key::J,
            Key::K,
            Key::H,
            Key::L,
            Key::M,
            Key::F,
            Key::Q,
            Key::R,
            Key::D,
            Key::Y,
            Key::Num1,
            Key::Minus,
            Key::Equals,
            Key::Slash,
            Key::OpenBracket,
            Key::CloseBracket,
            Key::Questionmark,
        ] {
            assert!(
                !survives_text_entry(key, false, false),
                "{key:?} would be swallowed by a command instead of typed"
            );
        }
    }

    #[test]
    fn a_panel_does_not_block_its_own_toggle_key() {
        // Settings and help have no text box, so `s` and `?` must still reach them —
        // otherwise the panel that `s` opened could only be closed with the mouse.
        assert!(commands_allowed(Key::S, false, false, false));
        assert!(commands_allowed(Key::Questionmark, false, false, false));
        // But with the search field focused, they are letters first.
        assert!(!commands_allowed(Key::S, false, false, true));
        assert!(!commands_allowed(Key::Questionmark, false, false, true));
        // …while the modified commands still get through either way.
        assert!(commands_allowed(Key::K, true, false, true));
    }

    #[test]
    fn modified_commands_and_escape_still_reach_through() {
        assert!(survives_text_entry(Key::O, true, false));
        assert!(survives_text_entry(Key::K, true, false));
        assert!(
            survives_text_entry(Key::P, true, false),
            "⌘P switches finder"
        );
        assert!(
            survives_text_entry(Key::F, true, false),
            "⌘F switches finder"
        );
        assert!(survives_text_entry(Key::Comma, true, false));
        assert!(survives_text_entry(Key::P, false, true));
        assert!(survives_text_entry(Key::Escape, false, false));
        // …but the same letters without their modifier do not.
        assert!(!survives_text_entry(Key::O, false, false));
        assert!(!survives_text_entry(Key::K, false, false));
        assert!(!survives_text_entry(Key::F, false, false));
    }
}
