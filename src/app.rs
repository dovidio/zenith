//! The window: a page, and four doors summoned at its head. The reading cursor is
//! the only permanent mark on the page.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::text::TextFormat;
use egui::{
    vec2, Align2, Color32, CornerRadius, FontId, Frame, Key, Margin, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, TextStyle, Ui, ViewportCommand,
};

use crate::catalog::{display_path, Store};
use crate::distill;
use crate::folder::{self, Entry, FileEntry, Folder};
use crate::fuzzy;
use crate::lsp;
use crate::outline::{self, SymKind, Symbol};
use crate::reader::{self, Laid, Paint, Style};
use crate::review::{Base, Review};
use crate::syntax::{Document, Highlighter, Readable};
use crate::theme::{self, Palette};
use crate::tone::Tone;
use crate::typography::{self, Typography};
use crate::vim;

mod launch;
mod lsp_ui;
mod page;
mod smoke;
mod widgets;
use self::launch::*;
use self::widgets::*;
mod tree;
use self::page::{focus_block, reveal_pending, Pending};
use self::tree::{TreeKey, TreeRow};
mod review;
mod sheets;

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

pub fn run(
    target: Option<PathBuf>,
    theme_override: Option<String>,
    measure_override: Option<usize>,
    scale_override: Option<f32>,
    distilled: Option<bool>,
) {
    launch(Startup {
        target,
        theme: theme_override,
        measure: measure_override,
        scale: scale_override,
        distilled,
        // The window asks for its own frame; the config can say otherwise, and the
        // settings panel can change it while running.
        frameless: true,
        smoke: None,
    });
}

pub fn run_smoke(
    target: Option<PathBuf>,
    out: PathBuf,
    theme_override: Option<String>,
    measure_override: Option<usize>,
    scale_override: Option<f32>,
) {
    launch(Startup {
        target: target.or_else(|| Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")))),
        theme: theme_override,
        measure: measure_override,
        scale: scale_override,
        distilled: None,
        frameless: true,
        smoke: Some(Smoke {
            out,
            frame: 0,
            phase: 0,
            requested: false,
            asked: 0,
            asked_at: 0,
        }),
    });
}

/// When the process started, for the timing report.
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn timing() -> bool {
    std::env::var("ZENITH_TIMING").is_ok()
}

fn launch(opts: Startup) {
    let _ = STARTED.set(std::time::Instant::now());
    let title = match &opts.target {
        Some(p) => format!(
            "zenith — {}",
            p.file_name().unwrap_or_default().to_string_lossy()
        ),
        None => "zenith".to_string(),
    };
    // The strip along the top of a window belongs to the desktop, not to the app: on macOS
    // it is the system's material, on Linux whatever the window manager decides. Neither
    // is the page's colour, so the window never looks like one thing.
    //
    // There are two honest ways to fix that, and they are different per platform.
    //
    // On macOS the window stays entirely the system's — its frame, its traffic lights, its
    // dragging, resizing and window snapping — and only the *title bar* is made
    // transparent, with the content filling the window. The system draws no background
    // there, so the theme's bar shows through, and the buttons float above it.
    //
    // On Linux (and Windows) the window manager paints the frame and there is no way to
    // ask it for a colour, so the frame is dropped and the buttons are ours.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("zenith")
        .with_inner_size([1440.0, 920.0])
        .with_min_inner_size([720.0, 480.0]);
    if opts.frameless {
        #[cfg(target_os = "macos")]
        {
            viewport = viewport
                .with_titlebar_shown(false) // transparent, buttons kept
                .with_fullsize_content_view(true);
        }
        #[cfg(not(target_os = "macos"))]
        {
            viewport = viewport.with_decorations(false);
        }
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let t_run = std::time::Instant::now();
    if let Err(e) = eframe::run_native(
        "zenith",
        options,
        Box::new(move |cc| {
            let t_new = std::time::Instant::now();
            let app = Zenith::new(cc, opts);
            if timing() {
                eprintln!("timing: Zenith::new took {:.1?}", t_new.elapsed());
            }
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("zenith: could not open a window: {e}");
        std::process::exit(1);
    }
    if timing() {
        eprintln!("timing: the window loop ran for {:.1?}", t_run.elapsed());
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The settings rail, top to bottom.
const SETTINGS_SECTIONS: [&str; 5] = ["Page", "Type", "Theme", "Language", "Keys"];

// ---------------------------------------------------------------------------
// The shape of the furniture
// ---------------------------------------------------------------------------

/// The only generous corner in the program: a sheet laid on the desk.
const SHEET_RADIUS: u8 = 14;
/// A theme swatch is a card, not a sheet.
const TILE_RADIUS: u8 = 8;
/// A row's wash and a control's corner.
const ROW_RADIUS: u8 = 4;
/// Letterspacing for a label that should read as a printed plate, in ems.
const LABEL_TRACKING: f32 = 0.12;
/// The masthead is the one place that is tracked a little wider.
const MASTHEAD_TRACKING: f32 = 0.14;
/// The air the page keeps above its title block: the bloom lives here.
const BLOOM_ROOM: f32 = 96.0;
/// The page stops short of the window by this much, so a notice has a quiet foot.
const PAGE_FOOT: f32 = 52.0;
/// The four doors the bloom opens.
const BLOOM_LABELS: [&str; 4] = ["INDEX", "CONTENTS", "LAUNCHER", "SETTINGS"];

/// The shadow under a sheet: paper lifted off the desk, not a light source.
fn sheet_shadow(pal: &Palette) -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: [0, 18],
        blur: 48,
        spread: 0,
        color: Color32::from_black_alpha(if pal.is_dark() { 110 } else { 46 }),
    }
}

/// Text that has been letterspaced.
///
/// egui's `Painter::text` cannot track a run, and the two places that need tracking —
/// the masthead and the panel plates — are exactly the places where tracking is the
/// difference between a label and tiny UI. Returns the rectangle it occupied, so a run
/// can be followed by another.
#[allow(clippy::too_many_arguments)]
fn paint_tracked(
    painter: &egui::Painter,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
    tracking: f32,
) -> Rect {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font,
            extra_letter_spacing: tracking,
            color,
            ..Default::default()
        },
    );
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// What a theme is shown by: a few lines of the sort of thing this program is for.
///
/// Short enough to fit in a tile, and written to use every tone the page has — a doc
/// comment in the reading face, a keyword, a type, a call, a number, punctuation.
const THEME_SAMPLE: &str = "//! read the room, slowly
fn read(page: &Page) {
    let t = page.wait(4);
    Ok(t)
}
";

/// One line of the sample: how far it is indented, and its tones.
type SampleLine = (usize, Vec<(String, Tone)>);

/// How long a message holds the status strip before the position readout comes back.
const NOTICE_SECONDS: f64 = 7.0;

/// What the background read of a large file sends back.
struct Loaded {
    doc: crate::syntax::Document,
    outline: Vec<outline::Symbol>,
    prose: bool,
}

/// A file being toned in the background. The first screenful is already on screen; this
/// completes the rest.
struct Loading {
    rx: std::sync::mpsc::Receiver<Loaded>,
    path: PathBuf,
}

/// A folder being walked in the background.
struct FolderLoad {
    rx: std::sync::mpsc::Receiver<std::io::Result<folder::Folder>>,
    root: PathBuf,
    focus: Option<PathBuf>,
}

/// A review being read from git in the background.
struct ReviewLoad {
    rx: std::sync::mpsc::Receiver<Result<Review, String>>,
}

/// A navigation question that has been asked but not yet answered.
/// A place worth coming back to: a file, and where in it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Place {
    path: PathBuf,
    caret: vim::Caret,
}

struct Nav {
    /// The request id, once it has been written.
    id: Option<i64>,
    /// How many times an empty answer has been taken as "not yet" rather than "nothing".
    tries: u32,
    /// Do not ask again before this: a server reading a project needs seconds, not frames.
    again_at: Option<std::time::Instant>,
    /// When the request went out, so that a server which never answers cannot leave the
    /// reader waiting for an answer that is not coming.
    sent_at: Option<std::time::Instant>,
    path: PathBuf,
    line: usize,
    /// The position in protocol coordinates — see [`lsp::text::position`].
    unit: u32,
    root: PathBuf,
}

/// A search for language servers, running in the background.
struct Discovery {
    rx: std::sync::mpsc::Receiver<Vec<lsp::servers::Available>>,
    root: PathBuf,
}

struct Startup {
    target: Option<PathBuf>,
    theme: Option<String>,
    measure: Option<usize>,
    scale: Option<f32>,
    distilled: Option<bool>,
    frameless: bool,
    smoke: Option<Smoke>,
}

struct Smoke {
    out: PathBuf,
    frame: u32,
    phase: u8,
    requested: bool,
    /// How many times a picture has been asked for, and when the last ask was.
    asked: u32,
    asked_at: u32,
}

/// The open document, in every sense: bytes, meaning, and place on the page.
struct Open {
    path: PathBuf,
    rel: String,
    doc: Document,
    /// True for Markdown and friends: bread text is prose, code blocks are not.
    prose: bool,
    outline: Vec<Symbol>,
    laid: Option<Laid>,
    error: Option<String>,
    /// Present when this file is being reviewed: the lines that changed and the code
    /// they replaced. `None` for ordinary reading.
    diff: Option<crate::review::Diff>,
    /// The file exists only in the base revision (a deleted file).
    deleted: bool,
}

pub struct Zenith {
    store: Store,
    hl: Highlighter,
    typo: Typography,
    palette: Palette,
    scale: f32,
    char_w: f32,

    folder: Option<Folder>,
    expanded: BTreeSet<String>,

    open: Option<Open>,
    /// Where the reader is: a line and a column, so word and bracket motions mean
    /// something. See [`crate::vim`].
    caret: vim::Caret,
    /// Two-key sequences (`gg`, `zz`), which live across frames.
    chords: vim::Chords,
    /// Test hook: pretend the reader is holding `j` down (see `ZENITH_HOLD_DOWN`).
    hold_down: bool,
    /// Frames drawn so far, for the test hooks.
    frame: usize,
    /// A background pass over a large file, if one is running.
    loading: Option<Loading>,
    /// A background walk of a folder, if one is running.
    folder_load: Option<FolderLoad>,
    /// A branch under review, once git has answered.
    review: Option<Review>,
    /// The changed file being read.
    review_index: usize,
    /// The base the reader asked for, or the working tree by default.
    review_base: Base,
    /// A review being collected in the background.
    review_load: Option<ReviewLoad>,
    /// The base sheet, and the ref typed into it.
    show_review_base: bool,
    review_base_sel: usize,
    review_base_opened_frame: usize,
    review_ref: String,
    review_branches: Vec<String>,
    review_error: String,
    /// The changed line whose previous version is lifted.
    review_hover: Option<usize>,
    /// A previous line lifted from the keyboard, without a pointer.
    review_peek: Option<usize>,
    /// Where the reader was in the review before a definition pulled them away.
    review_return: Option<Place>,
    /// Whether the changed-file strip should bring the current file back into view.
    review_strip_scroll: bool,
    /// A screenshot hook: start the review once the folder walk lands.
    review_after_load: bool,
    /// Every language server found on this machine for the languages this folder holds.
    available: Vec<lsp::servers::Available>,
    /// The languages this folder holds that zenith has a server for, found once per
    /// folder so the settings panel can list them without walking the tree every frame.
    lsp_kinds: Vec<&'static lsp::servers::Kind>,
    /// Languages whose server path has been typed into but not yet left: the server is
    /// restarted when the reader finishes, not on every keystroke.
    lsp_path_dirty: std::collections::BTreeSet<String>,
    /// The theme tiles' sample, toned once and painted six times.
    theme_preview: Option<Vec<SampleLine>>,
    /// When "restore defaults" was first clicked, so the second click has to be meant.
    reset_asked: Option<f64>,
    /// A search for those servers, running in the background.
    discovering: Option<Discovery>,
    /// Servers that have been started, at most one per language and folder.
    servers: Vec<lsp::Session>,
    /// A navigation request on its way to a server.
    nav: Option<Nav>,
    /// The document whose outline has been asked of a server.
    outline_asked: Option<PathBuf>,
    /// The message the strip is currently showing, and when it started showing it.
    shown: String,
    shown_at: f64,
    /// Bumped whenever a message is said, so that saying the same thing again is seen as
    /// saying it again rather than as saying nothing.
    status_seq: u64,
    shown_seq: u64,
    /// The debug-only `ZENITH_GOTO` hook has fired.
    #[cfg(debug_assertions)]
    nav_tried: bool,
    /// Frames to let pass after a landing, so the page can act on it before the report.
    #[cfg(debug_assertions)]
    nav_settled: u32,
    #[cfg(debug_assertions)]
    then_done: bool,
    /// Every row the folder panel is showing, in order, built once per frame so that the
    /// keyboard and the drawing agree about what is on screen.
    tree_rows: Vec<TreeRow>,
    /// Whether the keyboard is in the tree rather than on the page.
    tree_focus: bool,
    /// The selected row, held by identity rather than by position.
    tree_sel: Option<TreeKey>,
    /// Set when a key just moved the selection, so the panel can scroll it into view.
    tree_moved: bool,
    /// A development affordance has been applied, once.
    tree_hook_done: bool,
    /// The window's title needs telling what is open.
    title_dirty: bool,
    /// Height of the system's title strip on macOS, in points (0 elsewhere). Our own bar
    /// sits below it, so the system's title and traffic lights have the room they expect.
    chrome_strip: f32,
    tree_hook_step: u8,
    /// The places we have been, so `ctrl-o` can take us back — across files, because a
    /// definition is usually in another one.
    jumps: Vec<Place>,
    jump_pos: usize,
    outline_sel: Option<usize>,

    show_panels: bool,
    show_outline: bool,
    show_settings: bool,
    /// Which section of the settings panel is showing.
    settings_tab: usize,
    /// The settings sheet's keyboard focus: the rail, then the section's controls,
    /// then the reset action.
    settings_rail_focus: bool,
    settings_focus: usize,
    /// The filter on the Keys page.
    keys_query: String,
    /// The bloom is the one piece of chrome that offers itself. Its state is frame
    /// timing, not taste; `started` at -1 until the first frame can measure time.
    bloom_open: bool,
    bloom_started: f64,
    bloom_last_touch: f64,
    bloom_hover_since: Option<f64>,
    page_center_x: f32,
    palette_open: bool,
    palette_opened_frame: usize,
    palette_files_only: bool,
    query: String,
    palette_sel: usize,
    search_open: bool,
    search_query: String,
    /// The query as of the last frame, so hits are recomputed only on a keystroke.
    search_seen: String,
    search_hits: Vec<vim::Caret>,
    search_idx: usize,

    /// A pending movement of the page, resolved once the layout is known.
    pending: Pending,
    scroll: f32,
    /// Height of the reading viewport, remembered so the cursor can be revealed.
    view_h: f32,
    /// Line height in points, for turning screen coordinates into lines.
    row_h: f32,
    /// Reading positions remembered for this session, keyed by path.
    status: String,
    /// Set when the theme, scale, or a voice changes, so egui's style is rebuilt once.
    style_dirty: bool,
    /// When the reading position was last written to disk. A reader who force-quits
    /// should still come back to the page they were on.
    last_saved: f64,

    smoke: Option<Smoke>,
}

/// What `q` means while a review is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageClose {
    /// The page is part of the review: put the review down with it.
    LeaveReview,
    /// A definition detour remembered the review page: go back to it.
    BackToReview,
    /// A detour with nowhere to return to: close it, but keep the review.
    CloseDetour,
}

/// `q` leaves the review from a changed file, goes back to it from a detour, and never
/// silently throws it away when the reader is somewhere else.
fn page_close(review_open: bool, on_review_file: bool, has_return: bool) -> PageClose {
    if !review_open || on_review_file {
        PageClose::LeaveReview
    } else if has_return {
        PageClose::BackToReview
    } else {
        PageClose::CloseDetour
    }
}

impl Zenith {
    fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        let mut store = Store::load();
        if let Some(t) = &startup.theme {
            store.config.theme = t.clone();
        }
        if let Some(m) = startup.measure {
            store.config.measure = m.clamp(30, 240);
        }
        if let Some(s) = startup.scale {
            store.config.font_scale = s.clamp(0.6, 2.4);
        }
        if let Some(d) = startup.distilled {
            store.config.distilled = d;
        }
        let config = store.config.clone();
        let palette = theme::by_name(&config.theme);
        let typo = typography::build(0, 0, 0);
        cc.egui_ctx.set_fonts(typo.defs.clone());
        apply_style(&cc.egui_ctx, &palette, &typo, config.font_scale);
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::SetTheme(if palette.is_dark() {
                egui::SystemTheme::Dark
            } else {
                egui::SystemTheme::Light
            }));

        // Built once here rather than inside the literal below, so its cost is visible.
        let hl = Highlighter::new();
        // Ask the window how wide its buttons are, for the macOS bar inset.
        #[cfg(target_os = "macos")]
        let chrome_strip = {
            use raw_window_handle::HasWindowHandle as _;
            let zoom = cc.egui_ctx.zoom_factor();
            cc.window_handle()
                .ok()
                .and_then(|h| eframe::WindowChromeMetrics::from_window_handle(&h.as_raw()))
                // The traffic lights' own height is the height of the strip they live in.
                .map(|m| (m.traffic_lights_size.y / zoom).max(22.0))
                .unwrap_or(28.0)
        };
        #[cfg(not(target_os = "macos"))]
        let chrome_strip = 0.0;

        let mut app = Zenith {
            scale: config.font_scale,
            char_w: 15.0 * config.font_scale * 0.6,
            store,
            hl,
            typo,
            palette,
            folder: None,
            expanded: BTreeSet::new(),
            open: None,
            review: None,
            review_index: 0,
            review_base: Base::Working,
            review_load: None,
            show_review_base: false,
            review_base_sel: 0,
            review_base_opened_frame: 0,
            review_ref: String::new(),
            review_branches: Vec::new(),
            review_error: String::new(),
            review_hover: None,
            review_peek: None,
            review_return: None,
            review_strip_scroll: false,
            review_after_load: false,
            caret: vim::Caret::default(),
            chords: vim::Chords::default(),
            hold_down: std::env::var("ZENITH_HOLD_DOWN").is_ok(),
            frame: 0,
            loading: None,
            folder_load: None,
            available: Vec::new(),
            lsp_kinds: Vec::new(),
            lsp_path_dirty: std::collections::BTreeSet::new(),
            theme_preview: None,
            reset_asked: None,
            discovering: None,
            servers: Vec::new(),
            nav: None,
            outline_asked: None,
            shown: String::new(),
            shown_at: 0.0,
            status_seq: 0,
            shown_seq: 0,
            #[cfg(debug_assertions)]
            nav_tried: false,
            #[cfg(debug_assertions)]
            nav_settled: 0,
            #[cfg(debug_assertions)]
            then_done: false,
            tree_rows: Vec::new(),
            tree_focus: false,
            tree_sel: None,
            tree_moved: false,
            tree_hook_done: false,
            title_dirty: true,
            chrome_strip,
            tree_hook_step: 0,
            jumps: Vec::new(),
            jump_pos: 0,
            outline_sel: None,
            show_panels: false,
            show_outline: false,
            show_settings: false,
            settings_tab: 0,
            settings_rail_focus: true,
            settings_focus: 0,
            keys_query: String::new(),
            bloom_open: true,
            bloom_started: -1.0,
            bloom_last_touch: 0.0,
            bloom_hover_since: None,
            page_center_x: 0.0,
            palette_open: false,
            palette_opened_frame: 0,
            palette_files_only: false,
            query: String::new(),
            palette_sel: 0,
            search_open: false,
            search_query: String::new(),
            search_seen: String::new(),
            search_hits: Vec::new(),
            search_idx: 0,
            pending: Pending::None,
            scroll: 0.0,
            view_h: 0.0,
            row_h: 15.0 * 1.62,
            status: String::new(),
            style_dirty: true,
            last_saved: 0.0,
            smoke: startup.smoke,
        };
        // Fonts are not available until the first frame runs, so the precise
        // character width is measured there (see `style_dirty`).
        app.choose_startup(startup.target);
        // A development affordance: open an overlay without a keyboard, so the
        // screenshot harness can see it.
        if let Ok(n) = std::env::var("ZENITH_START_LINE") {
            if let (Some(open), Ok(n)) = (app.open.as_ref(), n.parse::<usize>()) {
                let caret = vim::landing(&open.doc, n.saturating_sub(1));
                app.caret = caret;
                app.jump_to(caret);
            }
        }
        if let Ok(base) = std::env::var("ZENITH_REVIEW_BASE") {
            app.review_base = Base::Ref(base);
        }
        if std::env::var("ZENITH_FORCE_REVIEW").is_ok() {
            app.review_after_load = true;
        }
        if std::env::var("ZENITH_FORCE_PALETTE").is_ok() {
            app.open_palette();
            if std::env::var("ZENITH_PALETTE_FILES").is_ok() {
                app.palette_files_only = true;
            }
            if let Ok(q) = std::env::var("ZENITH_PALETTE_QUERY") {
                app.query = q;
            }
        }
        if std::env::var("ZENITH_FORCE_SEARCH").is_ok() {
            app.open_search();
            if let Ok(q) = std::env::var("ZENITH_SEARCH_QUERY") {
                app.search_query = q;
            }
        }
        if std::env::var("ZENITH_FORCE_SETTINGS").is_ok() {
            app.show_settings = true;
        }
        if std::env::var("ZENITH_FORCE_TREE").is_ok() {
            app.show_panels = true;
            app.show_outline = false;
            app.tree_focus = true;
        }
        if std::env::var("ZENITH_FORCE_OUTLINE").is_ok() {
            app.show_outline = true;
            app.show_panels = false;
        }
        if let Ok(tab) = std::env::var("ZENITH_SETTINGS_TAB") {
            app.settings_tab = tab.parse().unwrap_or(0);
        }
        if let Ok(focus) = std::env::var("ZENITH_SETTINGS_FOCUS") {
            app.settings_rail_focus = false;
            app.settings_focus = focus.parse().unwrap_or(0);
        }
        app
    }

    /// Where does the reader begin? A file on the command line wins; otherwise the
    /// last place they were reading; otherwise the project they left open.
    fn choose_startup(&mut self, target: Option<PathBuf>) {
        // What to open, in order: what was asked for on the command line, the last folder
        // this reader had open, and — because running `zenith` inside a project should do
        // the obvious thing — the working directory, if that is what it looks like.
        //
        // Made absolute first. `zenith src/app.rs` from a repository root is the most
        // natural way to ask for a file, and a relative path breaks the walk: climbing out
        // of `src` by popping components reaches the empty path, and then the tree is
        // walking nothing at all while the page shows the file.
        let target = target
            .map(|p| p.canonicalize().unwrap_or(p))
            .or_else(|| self.store.session.last_root.clone())
            .or_else(|| self.store.session.last_file.clone())
            .or_else(working_project);

        let Some(target) = target else {
            self.status = keys("press {mod}O to open a folder");
            return;
        };
        if target.is_dir() {
            self.open_folder(&target, None);
        } else if target.is_file() {
            // The file the reader asked for, now; the folder around it can take its time.
            let root = crate::catalog::project_root(&target);
            self.open_file(&target);
            self.open_folder(&root, Some(target));
        }
    }

    // -- shelves -----------------------------------------------------------

    /// Open a folder, and the file in it the reader wants.
    ///
    /// The walk does not happen here. Reading a directory tree means a stat call per file,
    /// and on a large tree that is seconds — long enough that doing it inline held the
    /// window blank until it finished. The file the reader asked for opens at once, and
    /// the folder arrives on its own a moment later.
    fn open_folder(&mut self, root: &Path, focus: Option<PathBuf>) {
        // Absolute from here on: a folder named `.` has no file name to display, and a
        // relative path remembered in the session breaks the next time zenith is launched
        // from somewhere else.
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let root = root.as_path();
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());
        let cfg = self.store.config.clone();

        self.store.session.last_root = Some(root.to_path_buf());
        self.leave_review();
        self.folder = None;
        self.expanded.clear();
        self.expanded.insert(String::new());
        self.store.save_session();

        self.status = format!("reading {name}…");

        let root_for_thread = root.to_path_buf();
        let name_for_thread = name.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let result = folder::open(
                &root_for_thread,
                &name_for_thread,
                cfg.code_only,
                cfg.show_hidden,
            );
            if let Ok(s) = &result {
                eprintln!(
                    "timing: walk of {} took {:.2?} ({} files, {} unreadable)",
                    root_for_thread.display(),
                    t.elapsed(),
                    s.files.len(),
                    s.unreadable
                );
            }
            let _ = tx.send(result);
        });
        self.folder_load = Some(FolderLoad {
            rx,
            root: root.to_path_buf(),
            focus,
        });
    }

    /// The file a reader should be shown first: a README, then a manifest, then an
    /// entry point, then the first file in the folder.
    fn first_doorway(&self) -> Option<PathBuf> {
        let folder = self.folder.as_ref()?;
        let find = |pred: &dyn Fn(&FileEntry) -> bool| {
            folder
                .files
                .iter()
                .find(|f| pred(f))
                .map(|f| f.path.clone())
        };
        find(&|f| f.name.to_ascii_lowercase().starts_with("readme"))
            .or_else(|| find(&|f| f.rel.matches('/').count() == 0 && f.doorway))
            .or_else(|| find(&|f| f.doorway))
            .or_else(|| folder.files.first().map(|f| f.path.clone()))
    }

    fn rebuild_metrics(&mut self, ctx: &egui::Context) {
        let code_size = 15.0 * self.scale;
        let font = self.typo.code_id(code_size);
        self.char_w = ctx.fonts_mut(|f| f.glyph_width(&font, 'M')).max(1.0);
    }

    fn style(&self) -> Style {
        let (prose, lang) = self
            .open
            .as_ref()
            .map(|o| (o.prose, distill::lang_of(&o.path, &o.doc.syntax)))
            .unwrap_or((false, distill::Lang::Other));
        self.style_for(prose, lang)
    }

    fn style_for(&self, prose_document: bool, lang: distill::Lang) -> Style {
        let code_size = 15.0 * self.scale;
        let mut tones = [Color32::GRAY; Tone::COUNT];
        for t in Tone::ALL {
            tones[t.index()] = self.palette.tone(t);
        }
        // Prose is set larger than the code by exactly the ratio of their x-heights, so
        // that the two look like the same size next to each other. Same point size, and
        // the serif reads as fine print.
        let prose_size = code_size * self.typo.prose_scale();
        Style {
            code: self.typo.code_id(code_size),
            code_italic: self.typo.code_italic_id(code_size),
            code_strong: self.typo.code_strong_id(code_size),
            prose: self.typo.prose_id(prose_size),
            prose_italic: self.typo.prose_italic_id(prose_size),
            prose_strong: self.typo.prose_strong_id(prose_size),
            strong: false,
            ui: self.typo.ui_id(code_size * 0.82),
            leading: self.store.config.leading,
            measure: self.store.config.measure as f32 * self.char_w,
            wrap: self.store.config.wrap,
            guides: self.store.config.guides,
            compress_blank: self.store.config.compress_blank,
            line_numbers: self.store.config.line_numbers,
            serif_comments: self.store.config.serif_comments && self.palette.comment_italic,
            prose_document,
            distilled: self.store.config.distilled,
            // A diff is a diff of source lines. Rendering Markdown would move the
            // lines away from the marks and the meaning of the page.
            markdown: self.store.config.markdown
                && self.open.as_ref().is_none_or(|o| o.diff.is_none()),
            lang,
            wrap_mark: self.typo.marks.pick("↳"),
            tones,
            ink_faint: self.palette.ink_faint,
            guide_ink: self.palette.guide,
            paper: self.palette.paper,
            accent: self.palette.accent,
        }
    }

    // -- documents ---------------------------------------------------------

    fn open_file(&mut self, path: &Path) {
        // The review is a mode, not a page you leave by walking. Opening one of its
        // changed files attaches that file's marks again; opening a definition or any
        // other file keeps the review around so the way back is still the same review.
        if let Some(review) = &self.review {
            let target_in_review = review.files.iter().any(|file| file.path == path);
            if target_in_review {
                self.review_return = None;
            } else if self
                .open
                .as_ref()
                .is_some_and(|open| review.files.iter().any(|file| file.path == open.path))
            {
                // Leaving the changed file for a definition: remember the review page
                // so `q` can put the reader back there, not out of the review.
                self.review_return = self.open.as_ref().map(|open| Place {
                    path: open.path.clone(),
                    caret: self.caret,
                });
            }
            if let Some(index) = review.files.iter().position(|file| file.path == path) {
                self.open_review_index(index);
                return;
            }
        }
        self.open_file_plain(path);
    }

    /// Kept for the navigation paths: a jump and a direct open now agree about the
    /// review, but the name says why the path exists.
    fn open_file_nav(&mut self, path: &Path) {
        self.open_file(path);
    }

    fn open_file_plain(&mut self, path: &Path) {
        if self.open.is_none() {
            // The first page earns the words once; after that the dot is the mark and
            // the words are there for a reader who reaches for it.
            self.bloom_open = true;
            self.bloom_last_touch = 0.0;
        }
        self.nav = None;
        self.remember_position();
        if self.open.as_ref().map(|o| o.path.as_path()) != Some(path) {
            self.outline_asked = None;
        }
        let rel = self
            .folder
            .as_ref()
            .and_then(|s| s.file(path).map(|f| f.rel.clone()))
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        let head = read_head(path);
        let syntax = self.hl.syntax_for(path, &head).clone();
        let readable = match crate::syntax::read_readable(path) {
            Ok(r) => r,
            Err(e) => {
                self.open = Some(Open {
                    path: path.to_path_buf(),
                    rel,
                    doc: empty_doc(&syntax),
                    prose: false,
                    outline: Vec::new(),
                    laid: None,
                    error: Some(format!("{e}")),
                    diff: None,
                    deleted: false,
                });
                return;
            }
        };

        match readable {
            Readable::Binary { reason, .. } => {
                self.open = Some(Open {
                    path: path.to_path_buf(),
                    rel,
                    doc: empty_doc(&syntax),
                    prose: false,
                    outline: Vec::new(),
                    laid: None,
                    error: Some(format!("{reason} — zenith only reads text")),
                    diff: None,
                    deleted: false,
                });
            }
            Readable::Text {
                text,
                bytes,
                lossy,
                truncated,
            } => {
                // A reader should be reading, not waiting. Toning a megabyte of source
                // takes seconds of grammar work, so a large file is read in two passes:
                // the first screenful now, the rest in the background, with the text of
                // every line already present so that search, motions and wrapping work
                // from the first frame.
                const FIRST_PASS_LINES: usize = 400;
                let big = text.len() > 256 * 1024 || count_lines(&text) > 1_500;
                let t_open = std::time::Instant::now();
                let (mut doc, syms, _background) = if big {
                    let end = crate::syntax::prefix_end(&text, FIRST_PASS_LINES);
                    let mut doc = self.hl.parse(&syntax, &text[..end], bytes);
                    let t_plain = std::time::Instant::now();
                    for line in text[end..].lines() {
                        doc.lines.push(crate::syntax::Line::plain(line));
                    }
                    let syms = outline::extract(path, &doc.syntax, &doc.lines);
                    if std::env::var("ZENITH_TIMING").is_ok() {
                        eprintln!(
                            "timing: prefix {} lines {:.0?}, plain fill {:.0?}, outline {:.0?}",
                            FIRST_PASS_LINES,
                            t_plain.duration_since(t_open),
                            t_plain.elapsed(),
                            t_open.elapsed()
                        );
                    }
                    let (tx, rx) = std::sync::mpsc::channel();
                    let path_for_thread = path.to_path_buf();
                    let text_for_thread = text.clone();
                    std::thread::spawn(move || {
                        let hl = crate::syntax::Highlighter::new();
                        let head = read_head(&path_for_thread);
                        let syntax = hl.syntax_for(&path_for_thread, &head).clone();
                        let mut doc = hl.parse(&syntax, &text_for_thread, bytes);
                        doc.lossy = lossy;
                        if outline::family(&path_for_thread, &doc.syntax)
                            == outline::Family::Markdown
                        {
                            doc.md = Some(crate::markdown::render(&doc.lines));
                        }
                        let outline = outline::extract(&path_for_thread, &doc.syntax, &doc.lines);
                        let prose = outline::family(&path_for_thread, &doc.syntax)
                            == outline::Family::Markdown;
                        let _ = tx.send(Loaded {
                            doc,
                            outline,
                            prose,
                        });
                    });
                    let loading = Loading {
                        rx,
                        path: path.to_path_buf(),
                    };
                    self.loading = Some(loading);
                    (doc, syms, true)
                } else {
                    let t_small = std::time::Instant::now();
                    let doc = self.hl.parse(&syntax, &text, bytes);
                    let t_o = std::time::Instant::now();
                    let syms = outline::extract(path, &doc.syntax, &doc.lines);
                    if std::env::var("ZENITH_TIMING").is_ok() {
                        eprintln!(
                            "timing:   small file — parse {:.1?}, outline {:.1?}",
                            t_o.duration_since(t_small),
                            t_o.elapsed()
                        );
                    }
                    (doc, syms, false)
                };
                // A Markdown file gets a rendering of itself, kept beside the source.
                if outline::family(path, &doc.syntax) == outline::Family::Markdown {
                    doc.md = Some(crate::markdown::render(&doc.lines));
                }
                doc.truncated |= truncated;
                doc.lossy = lossy;
                let prose = outline::family(path, &doc.syntax) == outline::Family::Markdown;
                let mut note = String::new();
                if doc.truncated {
                    note.push_str("showing the first 2 MB");
                }
                if doc.lossy {
                    if !note.is_empty() {
                        note.push_str(" · ");
                    }
                    note.push_str("decoded lossily (not UTF-8)");
                }
                self.title_dirty = true;
                self.outline_sel = None;
                self.open = Some(Open {
                    path: path.to_path_buf(),
                    rel,
                    prose,
                    doc,
                    outline: syms,
                    laid: None,
                    error: None,
                    diff: None,
                    deleted: false,
                });
                if !note.is_empty() {
                    self.say(note);
                }
                self.caret = vim::Caret::default();
                self.pending = Pending::Restore;
                self.store.touch_recent(path);
                self.expand_to(path);
            }
        }
    }

    // -- bloom ---------------------------------------------------------------

    /// The bloom is one accent dot, with four words that unfurl when asked. At rest
    /// it is a mark; on hover it opens a short line of doors. The words stay while the
    /// pointer is on them, then fold away. The dot and line are the app's only motion.
    fn bloom_hover(&mut self, hovered: bool, now: f64) {
        if hovered {
            self.bloom_last_touch = now;
            if self.bloom_open {
                self.bloom_hover_since = None;
            } else {
                let since = *self.bloom_hover_since.get_or_insert(now);
                if now - since > 0.16 {
                    self.bloom_open = true;
                }
            }
        } else {
            self.bloom_hover_since = None;
            if self.bloom_open && now - self.bloom_last_touch > 2.6 {
                self.bloom_open = false;
            }
        }
    }

    /// Draw the bloom line: the dot travels from the centre to the start of the line as
    /// the four words settle out of it. `alpha` is the top-of-page fade; `open_t` is the
    /// dot/words expansion. Returns whether the dot or a word is under the pointer.
    fn paint_bloom_line(
        &mut self,
        ui: &mut Ui,
        center: Pos2,
        open_t: f32,
        alpha: f32,
        now: f64,
    ) -> bool {
        if alpha <= 0.001 {
            return false;
        }
        let pal = self.palette;
        let font = self.typo.ui_id(9.5 * self.scale.min(1.15));
        let tracking = 0.13 * 9.5 * self.scale.min(1.15);
        let galleys: Vec<_> = BLOOM_LABELS
            .iter()
            .map(|label| {
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    label,
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        extra_letter_spacing: tracking,
                        color: pal.ink_soft,
                        ..Default::default()
                    },
                );
                ui.painter().layout_job(job)
            })
            .collect();
        let dot_w = 10.0 * self.scale;
        let gap = 19.0 * self.scale;
        let total: f32 = dot_w
            + gap
            + galleys.iter().map(|g| g.size().x).sum::<f32>()
            + gap * (galleys.len().saturating_sub(1)) as f32;
        let dot_center = Pos2::new(center.x - (total * 0.5 - dot_w * 0.5) * open_t, center.y);
        let dot_rect = Rect::from_center_size(dot_center, vec2(28.0, 28.0));
        let dot = ui.interact(dot_rect, ui.id().with("bloom-dot"), Sense::click());
        let dot_hover = dot.hovered();
        let mut hovered = dot_hover;
        if dot.clicked() {
            self.bloom_open = !self.bloom_open;
            self.bloom_last_touch = now;
            if self.bloom_open {
                self.bloom_hover_since = None;
            } else {
                // A click that folds the words must not be undone by the pointer that
                // is still resting on the dot; it re-arms when the pointer leaves.
                self.bloom_hover_since = Some(now + 10.0);
            }
        }
        let dot_ink = theme::alpha(pal.accent, (alpha * 255.0) as u8);
        ui.painter()
            .circle_filled(dot_center, 3.5 * self.scale.min(1.2), dot_ink);
        if dot_hover {
            ui.painter().circle_stroke(
                dot_center,
                10.0 * self.scale.min(1.1),
                Stroke::new(1.0, dot_ink),
            );
        }
        if open_t <= 0.01 {
            return hovered;
        }
        let word_alpha = (alpha * open_t * 255.0) as u8;
        if word_alpha == 0 {
            return hovered;
        }
        let mut x = dot_center.x + dot_w * 0.5 + gap;
        for (i, galley) in galleys.into_iter().enumerate() {
            let size = galley.size();
            let hit = Rect::from_min_size(
                Pos2::new(x - 3.0, center.y - size.y * 0.5 - 4.0),
                vec2(size.x + 6.0, size.y + 8.0),
            );
            let response = ui.interact(hit, ui.id().with(("bloom-word", i)), Sense::click());
            let word_hover = response.hovered();
            hovered |= word_hover;
            if response.clicked() {
                self.bloom_open = false;
                self.bloom_last_touch = now;
                self.bloom_hover_since = Some(now + 10.0);
                self.summon(i);
            }
            let color = if word_hover { pal.ink } else { pal.ink_soft };
            let mut job = egui::text::LayoutJob::default();
            job.append(
                BLOOM_LABELS[i],
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    extra_letter_spacing: tracking,
                    color,
                    ..Default::default()
                },
            );
            let painted = ui.painter().layout_job(job);
            ui.painter().galley(
                Pos2::new(x, center.y - painted.size().y * 0.5),
                painted,
                theme::alpha(color, word_alpha),
            );
            x += size.x + gap;
        }
        hovered
    }

    /// A sheet's top: never tucked under the review bars, and never higher than the
    /// top air at large type. The base sheet hides the bloom band, so it only clears the
    /// review bar; the launcher clears both.
    fn sheet_top(&self, screen: Rect, high: bool, include_bloom: bool) -> f32 {
        let bars = self.review_bar_height()
            + if include_bloom {
                self.review_bloom_height()
            } else {
                0.0
            }
            + if self.review_bar_height() > 0.0 {
                16.0
            } else {
                0.0
            };
        let floor = screen.top() + bars;
        if high {
            floor.max(screen.top() + 72.0)
        } else {
            (screen.top() + screen.height() / 3.0).max(floor)
        }
    }

    fn remember_position(&mut self) {
        let Some(open) = &self.open else { return };
        let laid = match &open.laid {
            Some(l) => l,
            None => return,
        };
        let span = (laid.total - laid.row_h * 3.0).max(1.0);
        let frac = (self.scroll / span).clamp(0.0, 1.0);
        let path = open.path.clone();
        self.store.remember_position(&path, frac);
    }

    fn reload(&mut self) {
        if let Some(path) = self.open.as_ref().map(|o| o.path.clone()) {
            let caret = self.caret;
            self.open_file(&path);
            if let Some(open) = &self.open {
                self.caret = vim::clamp(&open.doc, caret);
            }
        }
    }

    fn line_count(&self) -> usize {
        self.open.as_ref().map(|o| o.doc.line_count()).unwrap_or(0)
    }

    /// Expand every ancestor folder of `path` so the open file is visible in the tree.
    fn expand_to(&mut self, path: &Path) {
        let Some(folder) = &self.folder else { return };
        let Some(rel) = folder.file(path).map(|f| f.rel.clone()) else {
            return;
        };
        let mut acc = String::new();
        let parts: Vec<&str> = rel.split('/').collect();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            self.expanded.insert(acc.clone());
        }
    }

    fn step_file(&mut self, forward: bool) {
        let Some(folder) = &self.folder else { return };
        let files: Vec<PathBuf> = folder
            .files
            .iter()
            .filter(|f| f.size > 0)
            .map(|f| f.path.clone())
            .collect();
        if files.is_empty() {
            return;
        }
        let cur = self.open.as_ref().map(|o| o.path.clone());
        let idx = cur
            .and_then(|p| files.iter().position(|f| *f == p))
            .map(|i| {
                if forward {
                    (i + 1) % files.len()
                } else {
                    (i + files.len() - 1) % files.len()
                }
            })
            .unwrap_or(0);
        let next = files[idx].clone();
        self.open_file(&next);
    }

    fn step_symbol(&mut self, forward: bool) {
        let Some(open) = &self.open else { return };
        if open.outline.is_empty() {
            return;
        }
        let cur = self.caret.line;
        let target = if forward {
            open.outline.iter().find(|s| s.line > cur).map(|s| s.line)
        } else {
            open.outline
                .iter()
                .rev()
                .find(|s| s.line < cur)
                .map(|s| s.line)
        };
        if let Some(line) = target {
            let caret = self
                .open
                .as_ref()
                .map(|o| vim::landing(&o.doc, line))
                .unwrap_or_default();
            self.jump_to(caret);
        }
    }

    /// Move the page the smallest amount that brings the cursor into view — except at
    /// the document's ends, where the reader means the end of the *page*, title block
    /// and all. Landing on the first or last line by any motion (k, ctrl-u, j, ctrl-d)
    /// should not leave the page stranded a few rows away from its edge.
    fn ensure_visible(&mut self) {
        let last = self
            .open
            .as_ref()
            .map(|o| o.doc.lines.len().saturating_sub(1))
            .unwrap_or(0);
        self.pending = reveal_pending(self.caret.line, last);
    }

    // -- layout ------------------------------------------------------------

    /// Typeset the open document for a page of this width, if it is not already
    /// typeset. Re-running is guarded by a hash of everything that affects layout.
    fn ensure_layout(&mut self, ctx: &egui::Context, page: Rect) {
        let t_layout = std::time::Instant::now();
        let gutter = self.char_w * 3.4;
        let style = {
            let mut s = self.style();
            s.measure = reader::effective_measure(page, s.measure, gutter);
            s
        };
        let col_x = reader::column_x(page, style.measure, gutter);
        // The bloom lives in the air above the title block; the header and every line
        // below it start after that air, so the space scrolls with the page.
        let header_h = reader::header_height(&style) + BLOOM_ROOM;
        let key = style.key();

        let Some(open) = &mut self.open else { return };
        if open.error.is_some() {
            return;
        }
        match &mut open.laid {
            Some(laid) if laid.key == key => {
                // Only the column moved: no need to re-typeset anything.
                laid.col_x = col_x;
            }
            _ => {
                let strong = open.diff.as_ref().map(|diff| {
                    let mut mask = vec![false; open.doc.lines.len()];
                    for &line in &diff.changed {
                        if let Some(slot) = mask.get_mut(line) {
                            *slot = true;
                        }
                    }
                    mask
                });
                open.laid = Some(reader::layout(
                    ctx,
                    &open.doc,
                    &style,
                    col_x,
                    header_h,
                    strong.as_deref(),
                ));
                if std::env::var("ZENITH_TIMING").is_ok() {
                    eprintln!("timing: layout {:.0?}", t_layout.elapsed());
                }
            }
        }
    }

    // -- keyboard ----------------------------------------------------------

    /// The keyboard layer, as one ordered pass over this frame's key presses.
    ///
    /// Every press is visited once, in the order it happened. A key that does not complete
    /// a two-key sequence is handled normally rather than discarded, so no press is ever
    /// silently lost.
    fn reading_keys(&mut self, ctx: &egui::Context) {
        // Two different kinds of "busy", which want different keys to work. A text box
        // (the file finder, the search field) owns every letter. An open panel (settings,
        // help) owns none of them, but should still keep the page still behind it.
        let text_entry = self.search_open
            || self.palette_open
            || self.show_review_base
            || (self.show_settings && ctx.memory(|m| m.focused().is_some()));
        let modal = self.show_settings || self.show_review_base;

        // Every key press this frame, with the modifiers that came with it.
        let presses: Vec<(Key, bool, bool, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, modifiers.shift, modifiers.ctrl, modifiers.command)),
                    _ => None,
                })
                .collect()
        });

        for (key, shift, ctrl, cmd) in presses {
            // ---- commands ----
            //
            // A text box owns the letter keys: the only commands that reach through one
            // are those that need a modifier, plus Escape. Without that rule a bound
            // letter cannot be typed at all.
            if commands_allowed(key, cmd, ctrl, text_entry) {
                if key == Key::Q && (cmd || ctrl) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    continue;
                }
                if cmd && key == Key::O {
                    self.pick_folder();
                    continue;
                }
                if cmd && key == Key::K {
                    self.open_palette();
                    continue;
                }
                if (cmd || ctrl) && key == Key::P {
                    self.open_palette_files();
                    continue;
                }
                if key == Key::F && (cmd || ctrl) {
                    self.open_search();
                    continue;
                }
                if (cmd && key == Key::Comma) || (!cmd && !ctrl && key == Key::S) {
                    self.show_settings = !self.show_settings;
                    if self.show_settings {
                        self.show_panels = false;
                        self.show_outline = false;
                        self.tree_focus = false;
                        self.settings_rail_focus = true;
                        self.settings_focus = 0;
                    }
                    continue;
                }
                if key == Key::Escape {
                    self.show_settings = false;
                    self.palette_open = false;
                    self.search_open = false;
                    self.show_review_base = false;
                    self.show_panels = false;
                    self.show_outline = false;
                    self.tree_focus = false;
                    self.bloom_open = false;
                    self.chords.clear();
                    continue;
                }
                if !cmd && !ctrl && key == Key::Questionmark {
                    self.show_settings = true;
                    self.show_panels = false;
                    self.show_outline = false;
                    self.tree_focus = false;
                    self.settings_tab = 4;
                    self.settings_rail_focus = true;
                    self.settings_focus = 0;
                    self.keys_query.clear();
                    continue;
                }
                if ctrl && key == Key::O {
                    self.jump(-1);
                    continue;
                }
                if ctrl && key == Key::I {
                    self.jump(1);
                    continue;
                }
            }
            if self.show_settings && self.settings_key(ctx, key, shift, ctrl, cmd) {
                continue;
            }
            if text_entry || modal {
                continue;
            }

            // ---- two-key sequences ----
            if let Some(c) = key_char(key) {
                match self
                    .chords
                    .feed(if shift { c.to_ascii_uppercase() } else { c })
                {
                    vim::Feed::Pending => continue,
                    vim::Feed::Chord(done) => {
                        self.run_chord(done);
                        continue;
                    }
                    vim::Feed::Pass => {}
                }
            }

            // ---- the tree owns the keyboard while it has focus ----
            if self.tree_focus {
                // Tab is also how you get *in*, so it has to be handled here as well as
                // in the command block, where it would be read as a page key.
                if key == Key::Tab {
                    self.tree_focus = false;
                } else {
                    self.tree_key(key, shift, ctrl);
                }
                continue;
            }
            if key == Key::Tab && !self.tree_rows.is_empty() {
                self.show_panels = true;
                self.show_outline = false;
                self.tree_focus = true;
                if self.tree_sel.is_none() {
                    self.tree_sel = self.tree_index().map(|i| self.tree_rows[i].key());
                }
                self.tree_moved = true;
                continue;
            }

            // ---- ordinary keys ----
            if let Some(act) = self.action_for(key, shift, ctrl, ctx) {
                self.run_action(act, ctx);
            }
        }
        if debug() && self.chords.is_pending() {
            eprintln!("zenith[keys]: waiting for the second key of a sequence");
        }
    }

    fn run_chord(&mut self, done: vim::Chord) {
        // Review is global: it belongs to the folder, not to the tree's selection.
        if done == vim::Chord::Review {
            self.toggle_review();
            return;
        }
        if done == vim::Chord::ReviewBase {
            self.open_review_base();
            return;
        }
        // Two-key sequences mean the same thing in the tree as on the page: `gg` is the
        // top of what you are looking at.
        if self.tree_focus {
            if done == vim::Chord::FileTop {
                let first = self.tree_rows.first().map(|r| r.key());
                self.tree_sel = first;
                self.tree_moved = true;
            }
            return;
        }
        match done {
            vim::Chord::FileTop => self.motion(vim::Motion::FileTop),
            vim::Chord::Centre => self.centre_on_caret(),
            vim::Chord::Top => self.align_caret(true),
            vim::Chord::Bottom => self.align_caret(false),
            vim::Chord::Definition => self.goto_definition(),
            vim::Chord::Review | vim::Chord::ReviewBase => {}
        }
    }

    /// What does this key mean? `None` for keys that mean nothing here.
    fn action_for(&self, key: Key, shift: bool, ctrl: bool, ctx: &egui::Context) -> Option<Act> {
        if ctrl {
            return match key {
                Key::D => Some(Act::HalfPageDown),
                Key::U => Some(Act::HalfPageUp),
                Key::F => Some(Act::PageDown),
                Key::B => Some(Act::PageUp),
                // Vim's other line motions; handy on keyboards where the arrow
                // cluster is a stretch away.
                Key::J => Some(Act::Down),
                Key::K => Some(Act::Up),
                _ => None,
            };
        }
        // Navigation first: a reader reaches for these without looking.
        let plain = match key {
            Key::J | Key::ArrowDown => Some(Act::Down),
            Key::K | Key::ArrowUp => Some(Act::Up),
            Key::Space | Key::PageDown => Some(Act::PageDown),
            Key::PageUp => Some(Act::PageUp),
            Key::Enter => Some(Act::Centre),
            Key::W => Some(Act::WordNext),
            Key::E => Some(Act::WordEnd),
            Key::B => Some(Act::WordPrev),
            Key::Num0 | Key::Home => Some(Act::LineStart),
            Key::End => Some(Act::LineEnd),
            Key::CloseCurlyBracket => Some(Act::ParagraphNext),
            Key::OpenCurlyBracket => Some(Act::ParagraphPrev),
            Key::H => Some(Act::ScreenTop),
            Key::M => Some(Act::ScreenMiddle),
            Key::L => Some(Act::ScreenBottom),
            Key::CloseBracket => Some(Act::NextChapter),
            Key::OpenBracket => Some(Act::PrevChapter),
            Key::N => Some(Act::SearchNext),
            Key::O => Some(Act::Outline),
            Key::F => Some(Act::Focus),
            Key::R => Some(Act::Reload),
            Key::Q => Some(Act::CloseFile),
            Key::Y => Some(Act::CopyBlock),
            Key::P => Some(Act::Peek),
            Key::D => Some(Act::Distilled),
            Key::X => Some(Act::Markdown),
            Key::T => Some(Act::NextTheme),
            Key::Slash => Some(Act::Search),
            Key::Minus => Some(Act::Smaller),
            Key::Equals => Some(Act::Larger),
            Key::Num1 => Some(Act::Theme(0)),
            Key::Num2 => Some(Act::Theme(1)),
            Key::Num3 => Some(Act::Theme(2)),
            Key::Num4 => Some(Act::Theme(3)),
            Key::Num5 => Some(Act::Theme(4)),
            Key::Num6 => Some(Act::Theme(5)),
            _ => None,
        };
        let shifted = match key {
            Key::G => Some(Act::FileBottom),
            Key::Y => Some(Act::CopyLine),
            Key::D => Some(Act::Distilled),
            Key::J => Some(Act::NextFile),
            Key::K => Some(Act::PrevFile),
            Key::M => Some(Act::Bookmark),
            Key::N => Some(Act::SearchPrev),
            Key::Num3 => Some(Act::LineNumbers),  // ⇧3 is `#`
            Key::Num4 => Some(Act::LineEnd),      // ⇧4 is `$`
            Key::Num5 => Some(Act::BracketMatch), // ⇧5 is `%`
            Key::Num6 => Some(Act::LineFirst),    // ⇧6 is `^`
            Key::T => Some(Act::PrevTheme),
            Key::Space => Some(Act::PageUp),
            _ => None,
        };
        let _ = ctx;
        if shift {
            shifted.or(plain.filter(|a| matches!(a, Act::PageUp)))
        } else {
            plain
        }
    }

    fn run_action(&mut self, act: Act, ctx: &egui::Context) {
        match act {
            Act::Down => self.motion(vim::Motion::Down(1)),
            Act::Up => self.motion(vim::Motion::Up(1)),
            Act::HalfPageDown => {
                let half = (self.view_h / self.row_h.max(1.0) / 2.0) as usize;
                self.motion(vim::Motion::Down(half.max(1)));
            }
            Act::HalfPageUp => {
                let half = (self.view_h / self.row_h.max(1.0) / 2.0) as usize;
                self.motion(vim::Motion::Up(half.max(1)));
            }
            Act::PageDown => {
                let page = (self.view_h / self.row_h.max(1.0)) as usize;
                self.motion(vim::Motion::Down(page.max(1)));
            }
            Act::PageUp => {
                let page = (self.view_h / self.row_h.max(1.0)) as usize;
                self.motion(vim::Motion::Up(page.max(1)));
            }
            Act::Centre => self.centre_on_caret(),
            Act::WordNext => self.motion(vim::Motion::WordNext),
            Act::WordPrev => self.motion(vim::Motion::WordPrev),
            Act::WordEnd => self.motion(vim::Motion::WordEnd),
            Act::LineStart => self.motion(vim::Motion::LineStart),
            Act::LineFirst => self.motion(vim::Motion::LineFirst),
            Act::LineEnd => self.motion(vim::Motion::LineEnd),
            Act::ParagraphNext => self.motion(vim::Motion::ParagraphNext),
            Act::ParagraphPrev => self.motion(vim::Motion::ParagraphPrev),
            Act::BracketMatch => self.motion(vim::Motion::BracketMatch),
            Act::FileBottom => self.motion(vim::Motion::FileBottom),
            Act::ScreenTop | Act::ScreenMiddle | Act::ScreenBottom => {
                let lines = self.line_count();
                if lines == 0 {
                    return;
                }
                let first = self.line_at_offset(self.scroll);
                let visible = (self.view_h / self.row_h.max(1.0)) as usize;
                let line = match act {
                    Act::ScreenTop => first,
                    Act::ScreenBottom => (first + visible.saturating_sub(1)).min(lines - 1),
                    _ => first + visible / 2,
                };
                let caret = vim::landing(&self.open.as_ref().unwrap().doc, line);
                self.jump_to(caret);
            }
            Act::Theme(idx) => {
                self.set_theme(ctx, idx);
                self.say(format!("theme: {}", self.palette.name));
            }
            Act::NextTheme | Act::PrevTheme => {
                let current = theme::THEMES
                    .iter()
                    .position(|p| p.name == self.palette.name)
                    .unwrap_or(0);
                let count = theme::THEMES.len();
                let next = if matches!(act, Act::NextTheme) {
                    (current + 1) % count
                } else {
                    (current + count - 1) % count
                };
                self.set_theme(ctx, next);
                self.say(format!("theme: {}", self.palette.name));
            }
            Act::LineNumbers => {
                self.store.config.line_numbers = !self.store.config.line_numbers;
                self.store.save_config();
                self.style_dirty = true;
                self.say(if self.store.config.line_numbers {
                    "line numbers on"
                } else {
                    "line numbers off"
                });
            }
            Act::Focus => {
                self.store.config.focus_lens = !self.store.config.focus_lens;
                self.store.save_config();
            }
            Act::Markdown => {
                self.store.config.markdown = !self.store.config.markdown;
                self.store.save_config();
                self.status = if self.store.config.markdown {
                    "markdown: rendered".into()
                } else {
                    "markdown: as written".into()
                };
            }
            Act::Distilled => {
                self.store.config.distilled = !self.store.config.distilled;
                self.store.save_config();
                self.status = if self.store.config.distilled {
                    "distilled: types and punctuation recede".into()
                } else {
                    "distilled off".into()
                };
            }
            Act::Bookmark => self.toggle_bookmark(),
            Act::CopyBlock | Act::CopyLine => {
                let whole = matches!(act, Act::CopyLine);
                self.copy_to_clipboard(ctx, whole);
            }
            Act::NextFile => {
                if self.review.is_some() {
                    self.step_review(true);
                } else {
                    self.step_file(true);
                }
            }
            Act::PrevFile => {
                if self.review.is_some() {
                    self.step_review(false);
                } else {
                    self.step_file(false);
                }
            }
            Act::NextChapter => {
                if self.review.is_some() {
                    self.review_change(true);
                } else {
                    self.step_symbol(true);
                }
            }
            Act::PrevChapter => {
                if self.review.is_some() {
                    self.review_change(false);
                } else {
                    self.step_symbol(false);
                }
            }
            Act::Outline => {
                self.show_outline = !self.show_outline;
                if self.show_outline {
                    self.show_panels = false;
                }
            }
            Act::Search => {
                // With nothing open there is nothing to search *in*, so `/` does the
                // thing the reader obviously wants instead of nothing at all.
                if self.open.is_some() {
                    self.open_search();
                } else {
                    self.open_palette();
                }
            }
            Act::SearchNext => self.search_step(true),
            Act::SearchPrev => self.search_step(false),
            Act::Smaller => self.bump_scale(-0.06, ctx),
            Act::Larger => self.bump_scale(0.06, ctx),
            Act::Reload => self.reload(),
            Act::CloseFile => self.close_page(),
            Act::Peek => {
                let Some(open) = &self.open else { return };
                let Some(diff) = &open.diff else {
                    self.status = "peek is for a review".into();
                    return;
                };
                let line = self.caret.line;
                let has = (diff.changed_at(line) && diff.old_for(line).is_some())
                    || diff.deletion_at(line).is_some();
                if !has {
                    self.status = "no previous code at this line".into();
                    return;
                }
                self.review_peek = if self.review_peek == Some(line) {
                    None
                } else {
                    Some(line)
                };
            }
        }
    }

    /// Put this file down.
    ///
    /// Closing the file you were *taken into* — a definition in a dependency, or in the
    /// standard library — puts you back where you were reading. That is what `:bd` does
    /// in vim, and it needs no command line here: `q` closes the file, and the file
    /// underneath it is the one you came from. With nothing underneath, the page is the
    /// reading room.
    fn close_page(&mut self) {
        let in_review = self.review.as_ref().is_some_and(|review| {
            self.open
                .as_ref()
                .is_some_and(|open| review.files.iter().any(|file| file.path == open.path))
        });
        match page_close(
            self.review.is_some(),
            in_review,
            self.review_return.is_some(),
        ) {
            PageClose::BackToReview => {
                if let Some(place) = self.review_return.take() {
                    self.go_to(&place);
                    self.status = "back to the review".into();
                } else {
                    self.close_file();
                }
            }
            PageClose::CloseDetour => {
                // A definition opened before we remembered the review page: closing it
                // should still leave the review around for the next `q`.
                self.close_file();
            }
            PageClose::LeaveReview => {
                self.leave_review();
                self.close_file();
            }
        }
    }

    fn close_file(&mut self) {
        self.remember_position();
        self.nav = None;
        let here = self.open.as_ref().map(|o| o.path.clone());
        let back = (0..self.jump_pos.min(self.jumps.len()))
            .rev()
            .find(|&i| Some(&self.jumps[i].path) != here.as_ref())
            .map(|i| (i, self.jumps[i].clone()));
        match back {
            Some((index, place)) => {
                self.jump_pos = index + 1;
                self.go_to(&place);
            }
            None => {
                self.open = None;
                self.caret = vim::Caret::default();
            }
        }
        self.store.save_session();
    }

    fn open_palette(&mut self) {
        // One summoned surface at a time: the launcher replaces a drawer or the search.
        self.search_open = false;
        self.show_panels = false;
        self.show_outline = false;
        self.show_settings = false;
        self.tree_focus = false;
        self.palette_open = true;
        self.palette_opened_frame = self.frame;
        self.palette_files_only = false;
        self.query.clear();
        self.palette_sel = 0;
    }

    /// `⌘P` / `ctrl-P` keep the narrow promise: this is the file finder, not the
    /// command sheet. `⌘K` opens the launcher.
    fn open_palette_files(&mut self) {
        self.open_palette();
        self.palette_files_only = true;
    }

    /// Which launcher commands make sense in this state?
    fn launch_available(&self, action: Launch) -> bool {
        match action {
            Launch::Contents
            | Launch::Search
            | Launch::CopyBlock
            | Launch::CloseFile
            | Launch::Bookmark
            | Launch::Reload => self.open.is_some(),
            Launch::Markdown => self.open.as_ref().is_some_and(|o| o.prose),
            Launch::Index => self.folder.is_some(),
            Launch::OpenFolder => true,
            Launch::Review => self.folder.is_some() || self.open.is_some(),
            Launch::ReviewBase => self.folder.is_some() || self.open.is_some(),
            _ => true,
        }
    }

    /// The state a launcher row carries, not the command it runs.
    fn launch_detail(&self, action: Launch) -> Option<String> {
        let on_off = |value: bool| Some(if value { "on" } else { "off" }.to_string());
        match action {
            Launch::FocusLens => on_off(self.store.config.focus_lens),
            Launch::SoftWrap => on_off(self.store.config.wrap),
            Launch::LineNumbers => on_off(self.store.config.line_numbers),
            Launch::Distilled => on_off(self.store.config.distilled),
            Launch::Markdown => on_off(self.store.config.markdown),
            Launch::NextTheme | Launch::PrevTheme => Some(self.palette.name.to_string()),
            Launch::Larger | Launch::Smaller => Some(format!("{:.2}×", self.scale)),
            Launch::Review => Some(self.review_base.label()),
            Launch::ReviewBase => self.review.as_ref().map(|r| r.base.label()),
            _ => None,
        }
    }

    fn launch_row(&self, spec: &'static CommandSpec, score: i32) -> LaunchRow {
        LaunchRow::Command {
            spec,
            detail: self.launch_detail(spec.action),
            score,
        }
    }

    /// Run one command from the launcher. The launcher closes first; the command opens
    /// whatever surface it means to open.
    fn launch(&mut self, action: Launch, ctx: &egui::Context) {
        self.palette_open = false;
        match action {
            Launch::Contents => {
                self.show_panels = false;
                self.show_outline = true;
                self.show_settings = false;
                self.search_open = false;
                self.tree_focus = false;
            }
            Launch::Search => self.open_search(),
            Launch::CopyBlock => self.copy_to_clipboard(ctx, false),
            Launch::CloseFile => self.close_file(),
            Launch::Bookmark => self.toggle_bookmark(),
            Launch::Reload => self.reload(),
            Launch::Index => {
                self.show_panels = true;
                self.show_outline = false;
                self.show_settings = false;
                self.search_open = false;
                self.tree_focus = true;
                if self.tree_sel.is_none() {
                    self.tree_sel = self.tree_index().map(|i| self.tree_rows[i].key());
                }
                self.tree_moved = true;
            }
            Launch::OpenFolder => self.pick_folder(),
            Launch::Review => self.toggle_review(),
            Launch::ReviewBase => self.open_review_base(),
            Launch::FocusLens => self.run_action(Act::Focus, ctx),
            Launch::SoftWrap => self.toggle_soft_wrap(),
            Launch::LineNumbers => self.run_action(Act::LineNumbers, ctx),
            Launch::Distilled => self.run_action(Act::Distilled, ctx),
            Launch::Markdown => self.run_action(Act::Markdown, ctx),
            Launch::NextTheme => self.run_action(Act::NextTheme, ctx),
            Launch::PrevTheme => self.run_action(Act::PrevTheme, ctx),
            Launch::Larger => self.run_action(Act::Larger, ctx),
            Launch::Smaller => self.run_action(Act::Smaller, ctx),
            Launch::Settings => {
                self.show_panels = false;
                self.show_outline = false;
                self.show_settings = true;
                self.settings_tab = 0;
                self.settings_rail_focus = true;
                self.settings_focus = 0;
            }
            Launch::Keys => {
                self.show_panels = false;
                self.show_outline = false;
                self.show_settings = true;
                self.settings_tab = 4;
                self.settings_rail_focus = true;
                self.settings_focus = 0;
                self.keys_query.clear();
            }
        }
    }

    /// Convert a content-space offset into a line index.
    fn line_at_offset(&self, offset: f32) -> usize {
        self.open
            .as_ref()
            .and_then(|o| o.laid.as_ref())
            .map(|l| l.line_at(offset))
            .unwrap_or(0)
    }

    /// `zt` / `zb`: put the caret's line at the top or the bottom of the page.
    fn align_caret(&mut self, to_top: bool) {
        self.pending = if to_top {
            Pending::Top(self.caret.line)
        } else {
            Pending::Bottom(self.caret.line)
        };
    }

    fn centre_on_caret(&mut self) {
        self.pending = Pending::Centre(self.caret.line);
    }

    fn open_search(&mut self) {
        self.palette_open = false;
        self.show_panels = false;
        self.show_outline = false;
        self.show_settings = false;
        self.tree_focus = false;
        self.search_open = true;
        self.search_query.clear();
        self.search_hits.clear();
        self.search_idx = 0;
    }

    /// Apply a motion and make sure the result is on screen.
    fn motion(&mut self, motion: vim::Motion) {
        // Moving is how a reader says they are no longer waiting: a slow answer must not
        // arrive later and carry the page off somewhere.
        self.nav = None;
        let Some(open) = &self.open else { return };
        let next = vim::resolve(&open.doc, self.caret, motion);
        if next != self.caret {
            self.push_jump();
            self.caret = next;
            self.review_peek = None;
        }
        match motion {
            vim::Motion::FileTop => self.pending = Pending::Start,
            vim::Motion::FileBottom => self.pending = Pending::End,
            _ => self.ensure_visible(),
        }
    }

    fn jump_to(&mut self, caret: vim::Caret) {
        if self.caret != caret {
            self.push_jump();
        }
        self.caret = caret;
        self.review_peek = None;
        self.ensure_visible();
    }

    fn push_jump(&mut self) {
        let Some(path) = self.open.as_ref().map(|o| o.path.clone()) else {
            return;
        };
        let place = Place {
            path,
            caret: self.caret,
        };
        // Already where the list ends: nothing to remember.
        if self.jump_pos > 0 && self.jumps.get(self.jump_pos - 1) == Some(&place) {
            return;
        }
        self.jumps.truncate(self.jump_pos);
        self.jumps.push(place);
        if self.jumps.len() > 200 {
            self.jumps.remove(0);
        }
        self.jump_pos = self.jumps.len();
    }

    /// Go to a remembered place, opening its file if that is a different one.
    fn go_to(&mut self, place: &Place) {
        if self.open.as_ref().map(|o| o.path.as_path()) != Some(place.path.as_path()) {
            self.open_file_nav(&place.path);
        }
        let caret = match &self.open {
            Some(open) => vim::clamp(&open.doc, place.caret),
            None => return,
        };
        self.caret = caret;
        self.ensure_visible();
    }

    /// Back and forth through the places the reader has been, files and all.
    fn jump(&mut self, direction: i32) {
        if self.jumps.is_empty() {
            return;
        }
        let next = self.jump_pos as i32 + direction;
        if next < 0 || next as usize >= self.jumps.len() {
            return;
        }
        self.jump_pos = next as usize;
        let place = self.jumps[self.jump_pos].clone();
        self.go_to(&place);
    }

    fn search_step(&mut self, forward: bool) {
        if self.search_query.is_empty() {
            return;
        }
        let found = {
            let Some(open) = &self.open else { return };
            let hit = vim::search(&open.doc, &self.search_query, self.caret, forward);
            let total = vim::matches(&open.doc, &self.search_query).len();
            hit.map(|h| (h, total))
        };
        match found {
            Some((hit, total)) => {
                self.jump_to(hit);
                self.status = format!("{total} match(es)");
            }
            None => self.status = format!("no match for {:?}", self.search_query),
        }
    }

    fn copy_to_clipboard(&mut self, ctx: &egui::Context, whole_line: bool) {
        let text = {
            let Some(open) = &self.open else { return };
            if whole_line {
                open.doc.lines[self.caret.line.min(open.doc.lines.len() - 1)].text()
            } else {
                let (a, b) = focus_block(open, self.caret.line);
                (a..=b.min(open.doc.lines.len().saturating_sub(1)))
                    .map(|i| open.doc.lines[i].text())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        ctx.copy_text(text);
        self.status = if whole_line {
            "copied the line".into()
        } else {
            "copied the block".into()
        };
    }

    fn toggle_soft_wrap(&mut self) {
        self.store.config.wrap = !self.store.config.wrap;
        self.store.save_config();
        self.style_dirty = true;
        self.say(if self.store.config.wrap {
            "soft wrap on"
        } else {
            "soft wrap off"
        });
    }

    fn bump_scale(&mut self, delta: f32, ctx: &egui::Context) {
        self.scale = (self.scale + delta).clamp(0.7, 2.2);
        self.store.config.font_scale = self.scale;
        self.rebuild_metrics(ctx);
        apply_style(ctx, &self.palette, &self.typo, self.scale);
        self.store.save_config();
    }

    fn set_theme(&mut self, ctx: &egui::Context, idx: usize) {
        if let Some(p) = theme::THEMES.get(idx) {
            self.palette = *p;
            self.store.config.theme = p.name.to_string();
            self.store.save_config();
            // The page is not the only thing with ink in it: every widget egui draws takes
            // its colours from the palette too, and a panel still wearing the old one is
            // how you end up with a dark slider on a light page.
            apply_style(ctx, p, &self.typo, self.scale);
            // Whatever the system still draws — menus, the title bar if it is visible, a
            // window manager's own preferences — should agree with the page.
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if p.is_dark() {
                egui::SystemTheme::Dark
            } else {
                egui::SystemTheme::Light
            }));
        }
    }

    fn toggle_bookmark(&mut self) {
        if let Some(path) = self.open.as_ref().map(|o| o.path.clone()) {
            let added = self.store.toggle_favorite(&path);
            self.store.save_session();
            self.status = if added {
                "bookmarked".into()
            } else {
                "bookmark removed".into()
            };
        }
    }

    fn pick_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Open a folder to read")
            .pick_folder()
        {
            self.open_folder(&dir, None);
        }
    }

    fn apply_typography(&mut self, ctx: &egui::Context, mono: usize, prose: usize) {
        self.typo = typography::build(mono, prose, self.typo.ui_idx);
        ctx.set_fonts(self.typo.defs.clone());
        self.rebuild_metrics(ctx);
        apply_style(ctx, &self.palette, &self.typo, self.scale);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    Reload,
    CloseFile,
    Smaller,
    Larger,
    Theme(usize),
    NextTheme,
    PrevTheme,
    LineNumbers,
    // files and chapters
    NextFile,
    PrevFile,
    Outline,
    Focus,
    Distilled,
    Markdown,
    Bookmark,
    // navigation
    Down,
    Up,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    Centre,
    WordNext,
    WordPrev,
    WordEnd,
    LineStart,
    LineFirst,
    LineEnd,
    ParagraphNext,
    ParagraphPrev,
    BracketMatch,
    ScreenTop,
    ScreenMiddle,
    ScreenBottom,
    FileBottom,
    NextChapter,
    PrevChapter,
    // review
    Peek,
    // search and copying
    Search,
    SearchNext,
    SearchPrev,
    CopyBlock,
    CopyLine,
}

// ---------------------------------------------------------------------------
// The frame
// ---------------------------------------------------------------------------

impl eframe::App for Zenith {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        if self.frame == 1 && timing() {
            if let Some(t0) = STARTED.get() {
                eprintln!("timing: first frame at {:.1?}", t0.elapsed());
            }
        }

        // Has the background walk of a folder finished?
        if let Some(load) = &self.folder_load {
            match load.rx.try_recv() {
                Ok(Ok(folder)) => {
                    let focus = load.focus.clone();
                    let root = load.root.clone();
                    self.folder_load = None;
                    self.folder = Some(folder);
                    // A review started before the folder walk finished owns the page.
                    // Opening the walk's first doorway here would clear it by accident.
                    let keep_review = self.review.is_some() || self.review_load.is_some();
                    let wanted = if keep_review {
                        None
                    } else {
                        focus
                            .filter(|p| p.starts_with(&root))
                            .or_else(|| {
                                self.open
                                    .as_ref()
                                    .map(|o| o.path.clone())
                                    .filter(|p| p.starts_with(&root))
                            })
                            .or_else(|| self.first_doorway())
                    };
                    match wanted {
                        Some(path) if self.open.as_ref().map(|o| &o.path) != Some(&path) => {
                            self.open_file(&path)
                        }
                        Some(path) => {
                            // The file was opened before the walk had finished, so it
                            // only knew its bare file name. The folder now knows where it
                            // lives, and the masthead is supposed to say so.
                            if let Some(rel) = self
                                .folder
                                .as_ref()
                                .and_then(|s| s.file(&path).map(|f| f.rel.clone()))
                            {
                                if let Some(open) = self.open.as_mut() {
                                    if open.path == path {
                                        open.rel = rel;
                                        self.title_dirty = true;
                                    }
                                }
                            }
                            self.expand_to(&path);
                        }
                        None => {}
                    }
                    self.start_discovery();
                    if self.review_after_load {
                        self.review_after_load = false;
                        self.start_review(self.review_base.clone());
                    }
                }
                Ok(Err(e)) => {
                    let root = load.root.clone();
                    self.folder_load = None;
                    self.status = format!("cannot read {root:?}: {e}");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.folder_load = None;
                }
            }
        }

        // Has the review's git diff finished?
        if let Some(load) = &self.review_load {
            match load.rx.try_recv() {
                Ok(Ok(review)) => {
                    self.review_load = None;
                    if review.files.is_empty() {
                        self.review = None;
                        self.status = format!("no changes against {}", review.base.label());
                    } else {
                        self.review = Some(review);
                        self.review_index = 0;
                        let start = std::env::var("ZENITH_REVIEW_INDEX")
                            .ok()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        let len = self.review.as_ref().map_or(1, |r| r.files.len());
                        self.open_review_index(start.min(len.saturating_sub(1)));
                        if std::env::var("ZENITH_REVIEW_BASE_SHEET").is_ok() {
                            self.open_review_base();
                        }
                        if let Some(review) = &self.review {
                            let mut note = format!(
                                "review · {} · {} file{}",
                                review.base.label(),
                                review.files.len(),
                                if review.files.len() == 1 { "" } else { "s" }
                            );
                            if review.skipped > 0 {
                                note.push_str(&format!(" · {} not shown", review.skipped));
                            }
                            self.status = note;
                        }
                    }
                }
                Ok(Err(e)) => {
                    self.review_load = None;
                    self.review = None;
                    self.status = format!("cannot review changes: {e}");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.review_load = None;
                }
            }
        }

        // Has the background pass over a large file finished?
        if let Some(loading) = &self.loading {
            match loading.rx.try_recv() {
                Ok(loaded) => {
                    let path = loading.path.clone();
                    self.loading = None;
                    if let Some(open) = self.open.as_mut() {
                        if open.path == path {
                            if std::env::var("ZENITH_TIMING").is_ok() {
                                eprintln!(
                                    "timing: background tones landed ({} lines)",
                                    loaded.doc.lines.len()
                                );
                            }
                            open.doc = loaded.doc;
                            open.outline = loaded.outline;
                            open.prose = loaded.prose;
                            open.laid = None;
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Keep the frames coming while the tones are still arriving.
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                }
            }
        }

        // The window's title, once the file it names has settled.
        if self.title_dirty {
            let rel = self.open.as_ref().map(|o| o.rel.clone());
            self.set_window_title(&ctx, rel.as_deref());
            self.title_dirty = false;
        }

        // The rows first: both the keyboard and the panel read this list, and they have to
        // agree about what "the row below this one" means.
        self.tree_rows = self.build_tree_rows();
        self.tree_moved = false;

        // A development affordance: move the tree selection without a keyboard, so a
        // screenshot can show the focused tree with something selected.
        if !self.tree_hook_done {
            if let Ok(n) = std::env::var("ZENITH_TREE_DOWN") {
                if self.tree_hook_step == 0 {
                    // First open the folders, so there is somewhere to go.
                    let dirs: Vec<String> = self
                        .tree_rows
                        .iter()
                        .filter_map(|r| match r {
                            TreeRow::Dir { rel, .. } => Some(rel.clone()),
                            _ => None,
                        })
                        .collect();
                    for d in dirs {
                        self.expanded.insert(d);
                    }
                    self.tree_hook_step = 1;
                } else {
                    let n: i32 = n.parse().unwrap_or(0);
                    self.tree_focus = true;
                    if self.tree_sel.is_none() {
                        self.tree_sel = self.tree_index().map(|i| self.tree_rows[i].key());
                    }
                    for _ in 0..n.abs() {
                        self.tree_key(if n < 0 { Key::K } else { Key::J }, false, false);
                    }
                    self.tree_moved = true;
                    self.tree_hook_done = true;
                    if debug() {
                        eprintln!(
                            "zenith[tree]: moved {n} in {} rows, sel={:?}",
                            self.tree_rows.len(),
                            self.tree_sel
                        );
                    }
                }
            } else if std::env::var("ZENITH_FORCE_TREE").is_ok() && !self.tree_rows.is_empty() {
                if self.tree_sel.is_none() {
                    self.tree_sel = self.tree_index().map(|i| self.tree_rows[i].key());
                }
                self.tree_moved = true;
                self.tree_hook_done = true;
            }
        }

        if self.style_dirty {
            apply_style(&ctx, &self.palette, &self.typo, self.scale);
            self.rebuild_metrics(&ctx);
            self.style_dirty = false;
        }

        if let Some(dir) = dropped_folder(&ctx) {
            self.open_folder(&dir, None);
        }

        // Remember where we are every so often, not only on a clean exit: closing a
        // laptop lid is not a save event.
        let now = ctx.input(|i| i.time);
        if self.smoke.is_none() && now - self.last_saved > 10.0 {
            self.last_saved = now;
            if self.open.is_some() {
                self.remember_position();
                self.store.save_session();
            }
        }

        self.poll_discovery();
        self.poll_lsp();
        // A server for the language being read, and its outline, once it is ready to
        // answer. Both are idempotent: they do nothing on most frames.
        self.ensure_servers();
        self.ensure_outline();

        self.reading_keys(&ctx);

        // Keys may have just asked a question, so the send happens after them.
        self.advance_nav();
        #[cfg(debug_assertions)]
        self.dev_nav(&ctx);
        #[cfg(debug_assertions)]
        self.forced_notice();
        if self.discovering.is_some() || self.nav.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }

        let pal = self.palette;
        // The page owns the window. On macOS the desktop draws the title and the traffic
        // lights; on Linux and Windows we still have to draw the window's own buttons,
        // so a bare controls bar remains there and nowhere else.
        if own_window_buttons() {
            self.top_bar(ui);
        }
        if self.review.is_some() || self.review_load.is_some() {
            if !self.show_review_base {
                self.review_bloom_bar(ui);
            }
            self.review_bar(ui);
        }

        if self.show_panels {
            self.folder_panel(ui);
        }
        if self.show_outline {
            self.outline_panel(ui);
        }

        egui::CentralPanel::no_frame()
            .frame(Frame::NONE.fill(pal.paper))
            .show(ui, |ui| {
                self.page(ui);
            });

        self.notice(ui);
        // The bloom is drawn above the page and below the sheets; it hides itself while
        // any summoned surface is open.
        self.bloom(&ctx);

        // Settings already owns an egui Modal with the same backdrop; the other sheets
        // are plain Windows and need the scrim supplied here.
        let sheet_open = self.palette_open || self.search_open || self.show_review_base;
        if sheet_open {
            let screen = ctx.content_rect();
            let _ = ui.interact(screen, ui.id().with("sheet-scrim"), Sense::click());
            ui.painter()
                .rect_filled(screen, CornerRadius::ZERO, theme::alpha(pal.ink, 44));
        }

        if self.palette_open {
            self.palette_overlay(&ctx);
        }
        if self.search_open {
            self.search_overlay(&ctx);
        }
        if self.show_settings {
            self.settings_overlay(&ctx);
        }
        if self.show_review_base {
            self.review_base_overlay(&ctx);
        }

        // A window without a frame has no edges of its own; these are ours. Last, so they
        // sit on top of the content they border.
        if self.store.config.frameless && !cfg!(target_os = "macos") {
            resize_handles(ui);
        }
        self.smoke_tick(&ctx);
    }

    fn on_exit(&mut self) {
        self.remember_position();
        self.store.save_config();
        self.store.save_session();
    }
}

impl Zenith {
    /// Tell the window what it is showing.
    ///
    /// On macOS the system draws this in the strip above our bar, and a window manager
    /// shows it in its frame elsewhere, so it should say where the reader is rather than
    /// only where they started.
    fn set_window_title(&self, ctx: &egui::Context, file: Option<&str>) {
        let title = match file {
            Some(rel) => format!("zenith — {rel}"),
            None => "zenith".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    /// How much room the system's title strip needs above our own bar, measured from the
    /// window itself.
    ///
    /// Guessing this number puts our text under the title on one display scale and leaves
    /// a gap on another; the window knows, so it is asked.
    fn title_strip(&self) -> f32 {
        if cfg!(target_os = "macos") && self.store.config.frameless {
            self.chrome_strip
        } else {
            0.0
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let dir_font = self.typo.ui_id(12.0 * self.scale);
        let file_font = self.typo.code_id(12.5 * self.scale);
        let ctx = ui.ctx().clone();
        // On macOS the system still draws its title and its traffic lights over the top of
        // the window, and a transparent title bar does not stop it — the title is drawn
        // over our content, twice over. So the strip belongs to the system and our own bar
        // begins underneath it, which is also what a system toolbar looks like.
        let strip = self.title_strip();
        let crumbs: Vec<String> = self
            .open
            .as_ref()
            .map(|o| o.rel.split('/').map(|s| s.to_string()).collect())
            .unwrap_or_default();
        let panel_rect = egui::Panel::top("zenith-top")
            .resizable(false)
            .exact_size(40.0 + strip)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(14, 6)),
            )
            .show(ui, |ui| {
                // The whole bar moves the window, the way a title bar does. Dragging is
                // started by the backend, so it must be asked for on the frame the drag
                // begins, not after it.
                let bar = ui.max_rect();
                // The buttons sit inside the bar, so the dragging area stops short of
                // them: a press on Close must close, not start moving the window.
                let drag_right = if own_window_buttons() {
                    bar.right() - window_buttons_width()
                } else {
                    bar.right()
                };
                let drag_rect =
                    Rect::from_min_max(bar.left_top(), Pos2::new(drag_right, bar.bottom()));
                let drag =
                    ui.interact(drag_rect, ui.id().with("titlebar"), Sense::click_and_drag());
                // Only where the desktop's own title bar has been taken away does the app
                // have to move the window itself. On macOS the system still does it.
                if self.store.config.frameless && !cfg!(target_os = "macos") {
                    if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    if drag.double_clicked() {
                        let maximised = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximised));
                    }
                }
                if own_window_buttons() {
                    window_buttons(ui, &pal, &ctx, bar);
                }
                let bar_rect = ui.max_rect();
                let y = bar_rect.top() + strip + (bar_rect.height() - strip) * 0.5;
                let painter = ui.painter();
                // An imprint, not a logo: spaced small caps, the quietest ink.
                let brand_size = 10.5 * self.scale;
                let mark = paint_tracked(
                    painter,
                    Pos2::new(bar_rect.left() + 1.0, y),
                    Align2::LEFT_CENTER,
                    "ZENITH",
                    self.typo.ui_id(brand_size),
                    pal.ink_soft,
                    MASTHEAD_TRACKING * brand_size,
                );
                let mark_sep = |x: f32, painter: &egui::Painter| -> f32 {
                    painter
                        .text(
                            Pos2::new(x, y),
                            Align2::LEFT_CENTER,
                            self.typo.marks.pick("›"),
                            self.typo.ui_id(11.5 * self.scale),
                            pal.ink_soft,
                        )
                        .right()
                };
                let mut x = mark.right() + 12.0;
                if !crumbs.is_empty() {
                    x = mark_sep(x, painter) + 10.0;
                }
                for (i, c) in crumbs.iter().enumerate() {
                    let last = i + 1 == crumbs.len();
                    // A path is made of names from the codebase, so the file itself is
                    // set in the code face; the folders are chrome and step back.
                    let f = if last {
                        file_font.clone()
                    } else {
                        dir_font.clone()
                    };
                    let color = if last { pal.ink } else { pal.ink_soft };
                    let r = painter.text(Pos2::new(x, y), Align2::LEFT_CENTER, c, f, color);
                    x = r.right() + 6.0;
                    if !last {
                        x = mark_sep(x, painter) + 6.0;
                    }
                }
            })
            .response
            .rect;
        // The frame is drawn, not implied: one hairline under the masthead.
        ui.painter().hline(
            panel_rect.x_range(),
            panel_rect.bottom() - 0.5,
            Stroke::new(1.0, pal.edge),
        );
    }

    /// A notice is one line at the foot of the page, not a bar. The rest of the time the
    /// foot is empty.
    fn notice(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        if self.status != self.shown || self.status_seq != self.shown_seq {
            self.shown = self.status.clone();
            self.shown_seq = self.status_seq;
            self.shown_at = now;
        }
        if now - self.shown_at > NOTICE_SECONDS {
            self.shown.clear();
            self.status.clear();
        }
        if self.shown.is_empty() {
            return;
        }
        let screen = ui.ctx().content_rect();
        let x = if self.page_center_x > 0.0 {
            self.page_center_x
        } else {
            screen.center().x
        };
        ui.painter().text(
            Pos2::new(x, screen.bottom() - 14.0),
            Align2::CENTER_BOTTOM,
            &self.shown,
            self.typo.ui_id(11.0 * self.scale),
            self.palette.ink_soft,
        );
    }

    /// The reading page's bloom: one accent dot at the top, words on intent. It lives
    /// in the page's top air and nowhere else; the first scroll folds it and fades it,
    /// so it can never sit over a line of code.
    fn bloom(&mut self, ctx: &egui::Context) {
        if self.review.is_some() || self.review_load.is_some() {
            return;
        }
        let surfaces = self.show_panels
            || self.show_outline
            || self.show_settings
            || self.palette_open
            || self.search_open
            || self.show_review_base;
        if surfaces {
            self.bloom_open = false;
            return;
        }
        let now = ctx.input(|i| i.time);
        if self.bloom_started < 0.0 {
            self.bloom_started = now;
            self.bloom_last_touch = now;
        }
        if self.bloom_last_touch <= 0.0 {
            self.bloom_last_touch = now;
        }
        let at_top = self.scroll < 4.0;
        if !at_top {
            self.bloom_open = false;
            self.bloom_hover_since = None;
        }

        let screen = ctx.content_rect();
        let bar = if own_window_buttons() { 40.0 } else { 0.0 };
        let top = screen.top() + bar + self.title_strip() + 14.0;
        let cx = screen.center().x;
        let dot_id = egui::Id::new("zenith-bloom");
        let vis_t = ctx.animate_bool_with_time(
            dot_id.with("visible"),
            at_top,
            if at_top { 0.16 } else { 0.35 },
        );
        if vis_t <= 0.001 {
            return;
        }
        let open_t = ctx.animate_bool_with_time(dot_id.with("open"), self.bloom_open, 0.18);
        let area = Rect::from_center_size(
            Pos2::new(cx, top + 7.0),
            vec2(
                (430.0 * self.scale).clamp(430.0, 620.0),
                (34.0 * self.scale).clamp(34.0, 48.0),
            ),
        );
        let mut hovered = false;
        egui::Area::new(dot_id)
            .order(egui::Order::Foreground)
            .fixed_pos(area.min)
            .show(ctx, |ui| {
                ui.set_min_size(area.size());
                hovered = self.paint_bloom_line(ui, area.center(), open_t, vis_t, now);
            });
        if at_top {
            self.bloom_hover(hovered, now);
        }
        if self.bloom_open
            && ctx.input(|i| i.pointer.primary_clicked())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !area.contains(p))
        {
            self.bloom_open = false;
            self.bloom_last_touch = now;
            self.bloom_hover_since = Some(now + 10.0);
        }
    }

    /// Open one of the bloom's four doors.
    fn summon(&mut self, which: usize) {
        self.show_settings = false;
        self.palette_open = false;
        self.search_open = false;
        self.tree_focus = false;
        match which {
            0 => {
                self.show_panels = true;
                self.show_outline = false;
                self.tree_focus = true;
                if self.tree_sel.is_none() {
                    self.tree_sel = self.tree_index().map(|i| self.tree_rows[i].key());
                }
                self.tree_moved = true;
            }
            1 => {
                self.show_panels = false;
                self.show_outline = true;
            }
            2 => {
                self.show_panels = false;
                self.show_outline = false;
                self.open_palette();
            }
            3 => {
                self.show_panels = false;
                self.show_outline = false;
                self.show_settings = true;
                self.settings_tab = 0;
                self.settings_rail_focus = true;
                self.settings_focus = 0;
            }
            _ => {}
        }
    }

    /// Say something in the strip. Saying the same thing twice still counts as saying it.
    fn say(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_seq += 1;
    }
}

#[cfg(test)]
mod page_close_tests {
    use super::{page_close, PageClose};

    #[test]
    fn q_leaves_the_review_only_from_the_review_itself() {
        assert_eq!(page_close(true, true, false), PageClose::LeaveReview);
        assert_eq!(page_close(false, false, false), PageClose::LeaveReview);
    }

    #[test]
    fn q_returns_from_a_detour_and_keeps_the_review() {
        assert_eq!(page_close(true, false, true), PageClose::BackToReview);
        assert_eq!(page_close(true, false, false), PageClose::CloseDetour);
    }
}
