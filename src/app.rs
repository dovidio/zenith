//! The window: a page, and four doors summoned at its head. The reading cursor is
//! the only permanent mark on the page.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::text::TextFormat;
use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Key, Margin, Pos2, Rect, RichText, ScrollArea,
    Sense, Stroke, TextStyle, Ui, ViewportCommand, vec2,
};

use crate::catalog::{Store, display_path};
use crate::distill;
use crate::fuzzy;
use crate::lsp;
use crate::outline::{self, SymKind, Symbol};
use crate::reader::{self, Laid, Paint, Style};
use crate::folder::{self, Entry, FileEntry, Folder};
use crate::syntax::{Document, Highlighter, Readable};
use crate::theme::{self, Palette, Tone};
use crate::typography::{self, Typography};
use crate::vim;

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
        Some(p) => format!("zenith — {}", p.file_name().unwrap_or_default().to_string_lossy()),
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
    bloom_visible: bool,
    bloom_started: f64,
    bloom_last_touch: f64,
    bloom_seen: bool,
    bloom_hover_since: Option<f64>,
    bloom_hover_opened: bool,
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
            bloom_visible: true,
            bloom_started: -1.0,
            bloom_last_touch: 0.0,
            bloom_seen: false,
            bloom_hover_since: None,
            bloom_hover_opened: false,
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
            folder.files.iter().find(|f| pred(f)).map(|f| f.path.clone())
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
            prose: self.typo.prose_id(prose_size),
            prose_italic: self.typo.prose_italic_id(prose_size),
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
            markdown: self.store.config.markdown,
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
                        let _ = tx.send(Loaded { doc, outline, prose });
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
            open.outline.iter().rev().find(|s| s.line < cur).map(|s| s.line)
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
                open.laid = Some(reader::layout(ctx, &open.doc, &style, col_x, header_h));
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
            || (self.show_settings && ctx.memory(|m| m.focused().is_some()));
        let modal = self.show_settings;

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
                    self.show_panels = false;
                    self.show_outline = false;
                    self.tree_focus = false;
                    self.bloom_open = false;
                    self.bloom_visible = false;
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
                match self.chords.feed(if shift { c.to_ascii_uppercase() } else { c }) {
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

    /// The index of the selected row, falling back to the file being read.
    ///
    /// Opening the tree should put you where you already are, not at the top of a list of
    /// twenty folders.
    fn tree_index(&self) -> Option<usize> {
        let rows = &self.tree_rows;
        if rows.is_empty() {
            return None;
        }
        if let Some(key) = &self.tree_sel {
            if let Some(i) = rows.iter().position(|r| &r.key() == key) {
                return Some(i);
            }
        }
        let here = self.open.as_ref().map(|o| o.path.clone());
        here.and_then(|p| {
            rows.iter()
                .position(|r| matches!(r.key(), TreeKey::File(q) if q == p))
        })
    }

    fn tree_move(&mut self, delta: i32) -> bool {
        let len = self.tree_rows.len();
        if len == 0 {
            return false;
        }
        let idx = walk_index(len, self.tree_index(), delta);
        let key = self.tree_rows[idx].key();
        let changed = self.tree_sel.as_ref() != Some(&key);
        self.tree_sel = Some(key);
        changed
    }

    /// `l`, Right or Enter: go in, or open.
    fn tree_open(&mut self) {
        let Some(idx) = self.tree_index() else { return };
        match self.tree_rows[idx].clone() {
            TreeRow::Dir { rel, open, .. } => {
                if open {
                    // Already open: step into it, the way a file manager does.
                    self.tree_move(1);
                } else {
                    self.expanded.insert(rel);
                }
            }
            TreeRow::File { path, .. } => {
                // The page follows the tree, but the keyboard stays here: browsing a
                // folder should not throw you back to the text on every file.
                self.open_file(&path);
            }
        }
    }

    /// `h` or Left: out of a folder, or up to whatever this row belongs to.
    fn tree_back(&mut self) {
        let Some(idx) = self.tree_index() else { return };
        if let TreeRow::Dir {
            rel,
            open: true,
            ..
        } = &self.tree_rows[idx]
        {
            let rel = rel.clone();
            self.expanded.remove(&rel);
            return;
        }
        if let Some(parent) = parent_index(&self.tree_rows, idx) {
            let key = self.tree_rows[parent].key();
            self.tree_sel = Some(key);
            self.tree_moved = true;
        }
    }

    /// The keyboard while the tree has focus. Every key is accounted for, so the page
    /// never moves behind the reader's back.
    fn tree_key(&mut self, key: Key, shift: bool, ctrl: bool) {
        let page = ((self.view_h / 21.0) as i32).max(4);
        let moved = match key {
            Key::J | Key::ArrowDown => self.tree_move(1),
            Key::K | Key::ArrowUp => self.tree_move(-1),
            Key::PageDown | Key::Space if !shift => self.tree_move(page),
            Key::PageUp | Key::Space => self.tree_move(-page),
            Key::D if ctrl => self.tree_move(page / 2),
            Key::U if ctrl => self.tree_move(-page / 2),
            Key::G if shift => {
                let last = self.tree_rows.len().saturating_sub(1);
                let key = self.tree_rows.get(last).map(|r| r.key());
                let changed = key.is_some() && self.tree_sel != key;
                self.tree_sel = key;
                changed
            }
            Key::L | Key::ArrowRight | Key::Enter => {
                self.tree_open();
                false
            }
            Key::H | Key::ArrowLeft => {
                self.tree_back();
                false
            }
            Key::Tab => {
                self.tree_focus = false;
                false
            }
            // `/` is how a reader looks for a file, and this is the file list.
            Key::Slash => {
                self.open_palette_files();
                false
            }
            _ => false,
        };
        if moved {
            self.tree_moved = true;
        }
    }

    fn run_chord(&mut self, done: vim::Chord) {
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
        }
    }

    /// What does this key mean? `None` for keys that mean nothing here.
    fn action_for(
        &self,
        key: Key,
        shift: bool,
        ctrl: bool,
        ctx: &egui::Context,
    ) -> Option<Act> {
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
            Key::Num3 => Some(Act::LineNumbers), // ⇧3 is `#`
            Key::Num4 => Some(Act::LineEnd),   // ⇧4 is `$`
            Key::Num5 => Some(Act::BracketMatch), // ⇧5 is `%`
            Key::Num6 => Some(Act::LineFirst), // ⇧6 is `^`
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
            Act::NextFile => self.step_file(true),
            Act::PrevFile => self.step_file(false),
            Act::NextChapter => self.step_symbol(true),
            Act::PrevChapter => self.step_symbol(false),
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
            Act::CloseFile => self.close_file(),
        }
    }

    /// Put this file down.
    ///
    /// Closing the file you were *taken into* — a definition in a dependency, or in the
    /// standard library — puts you back where you were reading. That is what `:bd` does
    /// in vim, and it needs no command line here: `q` closes the file, and the file
    /// underneath it is the one you came from. With nothing underneath, the page is the
    /// reading room.
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
            self.open_file(&place.path);
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
                    let wanted = focus
                        .filter(|p| p.starts_with(&root))
                        .or_else(|| {
                            self.open
                                .as_ref()
                                .map(|o| o.path.clone())
                                .filter(|p| p.starts_with(&root))
                        })
                        .or_else(|| self.first_doorway());
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

        if self.palette_open {
            self.palette_overlay(&ctx);
        }
        if self.search_open {
            self.search_overlay(&ctx);
        }
        if self.show_settings {
            self.settings_overlay(&ctx);
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
                let drag_rect = Rect::from_min_max(bar.left_top(), Pos2::new(drag_right, bar.bottom()));
                let drag = ui.interact(
                    drag_rect,
                    ui.id().with("titlebar"),
                    Sense::click_and_drag(),
                );
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
                    let f = if last { file_font.clone() } else { dir_font.clone() };
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

    /// The bloom: one dot at the top, four doors when it is asked. This is the only
    /// piece of chrome that offers itself, and the only thing in the program that moves.
    fn bloom(&mut self, ctx: &egui::Context) {
        let surfaces = self.show_panels
            || self.show_outline
            || self.show_settings
            || self.palette_open
            || self.search_open;
        let now = ctx.input(|i| i.time);
        let pal = self.palette;
        if self.bloom_started < 0.0 {
            self.bloom_started = now;
            self.bloom_last_touch = now;
        }
        if surfaces {
            self.bloom_open = false;
            self.bloom_visible = false;
            self.bloom_seen = true;
            return;
        }

        let screen = ctx.content_rect();
        let bar = if own_window_buttons() { 40.0 } else { 0.0 };
        let top = screen.top() + bar + self.title_strip() + 14.0;
        let cx = screen.center().x;
        let hot = Rect::from_center_size(Pos2::new(cx, top + 28.0), vec2(520.0, 80.0));
        let in_hot = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| hot.contains(p));
        if in_hot {
            self.bloom_seen = true;
            self.bloom_visible = true;
            self.bloom_last_touch = now;
        }
        if !self.bloom_seen && now - self.bloom_started > 4.0 {
            self.bloom_open = false;
            self.bloom_visible = false;
        }
        if self.bloom_open && now - self.bloom_last_touch > 3.5 {
            self.bloom_open = false;
            self.bloom_last_touch = now;
        }
        if self.bloom_visible && !self.bloom_open && !in_hot && now - self.bloom_last_touch > 2.5
        {
            self.bloom_visible = false;
        }

        let dot_id = egui::Id::new("zenith-bloom");
        let open_t = ctx.animate_bool_with_time(dot_id.with("open"), self.bloom_open, 0.2);
        let vis_t = ctx.animate_bool_with_time(
            dot_id.with("visible"),
            self.bloom_visible,
            if self.bloom_visible { 0.16 } else { 0.6 },
        );
        if vis_t <= 0.001 && open_t <= 0.001 {
            return;
        }

        let dot_center = Pos2::new(cx, top + 7.0);
        let targets = [
            Pos2::new(cx - 112.0, top + 56.0),
            Pos2::new(cx - 40.0, top + 48.0),
            Pos2::new(cx + 40.0, top + 48.0),
            Pos2::new(cx + 112.0, top + 56.0),
        ];
        let area = egui::Rect::from_min_size(
            Pos2::new(cx - 144.0, top - 18.0),
            vec2(288.0, 146.0),
        );
        let hovers = ctx.input(|i| i.pointer.hover_pos());
        let labels = self.scroll < 4.0;

        egui::Area::new(dot_id)
            .order(egui::Order::Foreground)
            .fixed_pos(area.min)
            .show(ctx, |ui| {
                ui.set_min_size(area.size());
                let painter = ui.painter();
                let base = theme::alpha(pal.ink_soft, (vis_t * 255.0) as u8);

                // The dot, and the ring that appears when the pointer reaches for it.
                let dot_rect = Rect::from_center_size(dot_center, vec2(28.0, 28.0));
                let dot = ui.interact(dot_rect, dot_id.with("dot"), Sense::click());
                let dot_hover = dot.hovered()
                    || hovers.is_some_and(|p| p.distance(dot_center) <= 10.0);
                if dot_hover {
                    self.bloom_seen = true;
                    self.bloom_last_touch = now;
                    let since = *self.bloom_hover_since.get_or_insert(now);
                    if !self.bloom_open && now - since > 0.2 {
                        self.bloom_open = true;
                        self.bloom_hover_opened = true;
                    }
                } else {
                    self.bloom_hover_since = None;
                    self.bloom_hover_opened = false;
                }
                if dot.clicked() {
                    if !self.bloom_hover_opened {
                        self.bloom_open = !self.bloom_open;
                    }
                    self.bloom_hover_since = None;
                    self.bloom_hover_opened = false;
                    self.bloom_last_touch = now;
                    self.bloom_seen = true;
                }
                painter.circle_filled(dot_center, 3.5, base);
                if dot_hover {
                    painter.circle_stroke(
                        dot_center,
                        10.0,
                        Stroke::new(1.0, theme::alpha(pal.accent, (vis_t * 255.0) as u8)),
                    );
                }

                // Stems first, so the bubbles sit on them.
                for target in targets {
                    let at = dot_center + (target - dot_center) * open_t;
                    if open_t > 0.01 {
                        painter.line_segment(
                            [dot_center, at],
                            Stroke::new(
                                1.0,
                                theme::alpha(
                                    pal.ink_faint,
                                    (open_t * vis_t * 255.0) as u8,
                                ),
                            ),
                        );
                    }
                }

                for (i, target) in targets.into_iter().enumerate() {
                    let at = dot_center + (target - dot_center) * open_t;
                    let alpha = (open_t * vis_t * 255.0) as u8;
                    if alpha == 0 {
                        continue;
                    }
                    // While the doors are still on top of the dot, they must not be
                    // hit-testable: a click meant to open the bloom would land on
                    // whichever door happened to be painted last (SETTINGS).
                    let hovered = if open_t > 0.85 {
                        let hit =
                            Rect::from_center_size(Pos2::new(at.x, at.y + 7.0), vec2(56.0, 48.0));
                        let response =
                            ui.interact(hit, dot_id.with(("bubble", i)), Sense::click());
                        let hovered = response.hovered();
                        if hovered {
                            self.bloom_seen = true;
                            self.bloom_last_touch = now;
                        }
                        if response.clicked() {
                            self.bloom_open = false;
                            self.bloom_visible = false;
                            self.bloom_seen = true;
                            self.summon(i);
                        }
                        hovered
                    } else {
                        false
                    };
                    let ink = if hovered {
                        pal.ink
                    } else {
                        pal.ink_soft
                    };
                    let edge = if hovered {
                        pal.accent
                    } else {
                        pal.ink_faint
                    };
                    painter.circle_filled(at, 13.0, theme::alpha(pal.paper, alpha));
                    painter.circle_stroke(
                        at,
                        13.0,
                        Stroke::new(1.0, theme::alpha(edge, alpha)),
                    );
                    draw_bloom_mark(painter, at, i, theme::alpha(ink, alpha));
                    if labels {
                        paint_tracked(
                            painter,
                            Pos2::new(at.x, at.y + 18.0),
                            Align2::CENTER_TOP,
                            BLOOM_LABELS[i],
                            self.typo.ui_id(9.5 * self.scale.min(1.15)),
                            theme::alpha(if hovered { pal.ink } else { pal.ink_soft }, alpha),
                            (0.08 * 9.5 * self.scale.min(1.15)).max(0.0),
                        );
                    }
                }
            });

        if self.bloom_open
            && ctx.input(|i| i.pointer.primary_clicked())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !area.contains(p))
        {
            self.bloom_open = false;
            self.bloom_last_touch = now;
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

    /// The folder panel's rows, in the order they appear on screen.
    ///
    /// Built before the keyboard is read each frame, because both the navigation and the
    /// drawing have to mean the same thing by "the row below this one".
    fn build_tree_rows(&self) -> Vec<TreeRow> {
        let mut out: Vec<TreeRow> = Vec::new();
        if let Some(folder) = &self.folder {
            let mut acc = String::new();
            flatten(&folder.tree, &self.expanded, 0, &mut acc, &mut out);
        }
        out
    }

    fn folder_panel(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let font = self.typo.code_id(PANEL_ROW_FONT * self.scale);
        let label_font = self.typo.ui_id(PANEL_LABEL_FONT * self.scale);

        // Collect what we need before mutating. The rows come from the one list the
        // keyboard walks, so a row and its key action can never disagree about which row
        // is which.
        let rows: Vec<TreeRow> = self.tree_rows.clone();
        let tree_files: Vec<TreeRow> = rows.clone();
        let folder_name = self.folder.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let current = self.open.as_ref().map(|o| o.path.clone());

        let mut clicked_file: Option<PathBuf> = None;
        let mut toggled_dir: Option<String> = None;

        let panel_rect = egui::Panel::left("zenith-folder")
            .resizable(true)
            .default_size(252.0)
            .min_size(200.0)
            .max_size(340.0)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(PANEL_MARGIN_X, PANEL_MARGIN_Y)),
            )
            .show(ui, |ui| {
                // With no folder open the panel stays empty: the page already offers to
                // open one, and being told twice is noise.
                if !folder_name.is_empty() {
                    ui.add_space(6.0);
                    ui.painter().hline(
                        ui.min_rect().left()..=ui.min_rect().right(),
                        ui.cursor().top(),
                        Stroke::new(1.0, pal.edge),
                    );
                    ui.add_space(6.0);
                    paint_tracked(
                        ui.painter(),
                        Pos2::new(ui.cursor().left() + 1.0, ui.cursor().top() + PANEL_LABEL_DROP),
                        Align2::LEFT_CENTER,
                        &format!("FOLDER · {folder_name}"),
                        label_font.clone(),
                        pal.ink_soft,
                        LABEL_TRACKING * PANEL_LABEL_FONT * self.scale,
                    );
                }
                if let Some(s) = &self.folder {
                    // No file count, no folder count, no total size: none of it is what
                    // a reader came for. Only a fault gets a line of its own.
                    let mut warn = String::new();
                    if s.unreadable > 0 {
                        warn.push_str(&format!("{} unreadable", s.unreadable));
                    }
                    if !warn.is_empty() {
                        ui.painter().text(
                            Pos2::new(ui.cursor().left() + 6.0, ui.cursor().top() + 22.0),
                            Align2::LEFT_CENTER,
                            warn,
                            label_font.clone(),
                            pal.accent,
                        );
                        ui.add_space(16.0);
                    }
                    // What a filter left out is worth saying once, on hover, because a
                    // reader who cannot see a file should be able to find out why.
                    if s.filtered > 0 {
                        let note = format!(
                            "{} file{} not shown: not source, hidden, or build output \
                             (settings › page)",
                            s.filtered,
                            if s.filtered == 1 { "" } else { "s" }
                        );
                        ui.interact(
                            Rect::from_min_size(
                                Pos2::new(ui.cursor().left(), ui.cursor().top()),
                                vec2(ui.available_width(), 16.0),
                            ),
                            ui.id().with("folder-filter"),
                            Sense::hover(),
                        )
                        .on_hover_text(RichText::new(note).font(label_font.clone()));
                    }
                }
                ui.add_space(PANEL_LABEL_GAP);

                ScrollArea::vertical()
                    .id_salt("zenith-tree")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in &tree_files {
                            let key = item.key();
                            let picked = self.tree_focus && self.tree_sel.as_ref() == Some(&key);
                            let (name, depth, doorway, empty_file, path, rel, is_open) = match item
                            {
                                TreeRow::Dir { name, depth, rel, open } => {
                                    (name, *depth, false, false, None, Some(rel.clone()), *open)
                                }
                                TreeRow::File { name, path, depth, doorway, empty } => (
                                    name,
                                    *depth,
                                    *doorway,
                                    *empty,
                                    Some(path.clone()),
                                    None,
                                    false,
                                ),
                            };
                            let on_current = path
                                .as_ref()
                                .is_some_and(|p| current.as_ref() == Some(p));
                            let ink = if empty_file {
                                pal.ink_faint
                            } else if picked || on_current || doorway {
                                pal.ink
                            } else {
                                pal.ink_soft
                            };
                            let resp = row(
                                ui,
                                &pal,
                                name,
                                if doorway { self.typo.marks.pick("◦") } else { "" },
                                depth,
                                picked,
                                ink,
                                &font,
                                None,
                                if rel.is_some() { Some(is_open) } else { None },
                            );
                            if self.tree_moved && picked {
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                            if resp.clicked() {
                                self.tree_focus = true;
                                self.tree_sel = Some(key);
                                match item {
                                    TreeRow::Dir { .. } => toggled_dir = rel,
                                    TreeRow::File { path, .. } => clicked_file = Some(path.clone()),
                                }
                            }
                        }
                    });
            })
            .response
            .rect;

        if let Some(rel) = toggled_dir {
            if !self.expanded.remove(&rel) {
                self.expanded.insert(rel);
            }
        }
        if let Some(path) = clicked_file {
            self.open_file(&path);
        }
        // One hairline between the desk and the page, not a gap.
        ui.painter().vline(
            panel_rect.right() - 0.5,
            panel_rect.y_range(),
            Stroke::new(1.0, pal.edge),
        );
    }



    fn outline_panel(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let font = self.typo.code_id(PANEL_ROW_FONT * self.scale);
        let label_font = self.typo.ui_id(PANEL_LABEL_FONT * self.scale);
        let glyph_font = self.typo.code_id(11.0 * self.scale);

        let items: Vec<(String, String, usize, usize, String, SymKind)> = self
            .open
            .as_ref()
            .map(|o| {
                o.outline
                    .iter()
                    .map(|s| {
                        (
                            s.name.clone(),
                            s.detail.clone(),
                            s.line,
                            s.depth,
                            s.kind.glyph(&self.typo.marks).to_string(),
                            s.kind,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let title = self
            .open
            .as_ref()
            .map(|o| {
                o.path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let cursor = self.caret.line;
        let some_open = self.open.is_some();
        let mut jump: Option<usize> = None;

        let panel_rect = egui::Panel::right("zenith-outline")
            .resizable(true)
            .default_size(240.0)
            .min_size(180.0)
            .max_size(340.0)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(PANEL_MARGIN_X, PANEL_MARGIN_Y)),
            )
            .show(ui, |ui| {
                // The same rule and the same rhythm as the folder panel, so the two
                // plates sit on one line across the page.
                ui.add_space(6.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(6.0);
                paint_tracked(
                    ui.painter(),
                    Pos2::new(ui.cursor().left() + 1.0, ui.cursor().top() + PANEL_LABEL_DROP),
                    Align2::LEFT_CENTER,
                    "CONTENTS",
                    label_font.clone(),
                    pal.ink_soft,
                    LABEL_TRACKING * PANEL_LABEL_FONT * self.scale,
                );
                ui.add_space(PANEL_LABEL_GAP);
                if !some_open {
                    return;
                }
                ScrollArea::vertical()
                    .id_salt("outline")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // A file with no outline gets an empty panel, not a note about
                        // being empty: the reader can see there is nothing there.
                        if items.is_empty() {
                            return;
                        }
                        // Which symbol is the cursor inside?
                        let mut active = None;
                        for (i, (_, _, line, ..)) in items.iter().enumerate() {
                            if *line <= cursor {
                                active = Some(i);
                            } else {
                                break;
                            }
                        }
                        for (i, (name, detail, _line, depth, glyph, kind)) in
                            items.iter().enumerate()
                        {
                            let selected = active == Some(i);
                            let ink = if selected { pal.ink } else { pal.ink_soft };
                            let resp = row(
                                ui,
                                &pal,
                                name,
                                glyph,
                                (*depth).min(6),
                                selected,
                                ink,
                                &font,
                                None,
                            None,
                            );
                            if resp.clicked() {
                                jump = Some(*_line);
                            }
                            if resp.hovered() && !detail.is_empty() {
                                resp.on_hover_text(
                                    RichText::new(format!("{} · {detail}", kind.label()))
                                        .font(glyph_font.clone()),
                                );
                            }
                        }
                    });
                let _ = title;
            })
            .response
            .rect;

        // The same hairline as the folder panel's, on its own side.
        ui.painter().vline(
            panel_rect.left() + 0.5,
            panel_rect.y_range(),
            Stroke::new(1.0, pal.edge),
        );

        if let Some(line) = jump {
            let caret = self
                .open
                .as_ref()
                .map(|o| vim::landing(&o.doc, line))
                .unwrap_or_default();
            self.jump_to(caret);
        }
    }

    /// The page itself: a title block, then the text, typeset.
    fn page(&mut self, ui: &mut Ui) {
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
                Pos2::new(row.center().x - galley.size().x * 0.5, row.center().y - galley.size().y * 0.5),
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
        let parent = open
            .path
            .parent()
            .map(display_path)
            .unwrap_or_default();
        let badge = self
            .store
            .is_favorite(&open.path)
            .then_some((format!("{} bookmarked", self.typo.marks.pick("✦")), pal.accent));

        let content_w = ui
            .available_width()
            .max(laid.col_x + laid.measure + 40.0);

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
            Pending::Centre(line) => Some(
                (laid.top_of(line) + laid.height_of(line) * 0.5 - view_h * 0.5).max(0.0),
            ),
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
            rect
        });

        self.scroll = out.state.offset.y;
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
        reader::paint_spine(
            ui.painter(),
            spine_x,
            spine_top,
            spine_h,
            &pal,
            frac,
        );
    }

    /// How far down the file the page can go: the one range both directions use.
    ///
    /// It used to be `total - four rows` for the reading position and `total - view` for
    /// what scrolling actually allows, which are different numbers — so the mark saturated
    /// before the page reached its end and never arrived at the bottom.
    fn scroll_range(&self) -> Option<(f32, f32)> {
        let open = self.open.as_ref()?;
        let laid = open.laid.as_ref()?;
        let max = (laid.total - self.view_h).max(0.0);
        Some((self.scroll, max))
    }

    /// A place in the file as a fraction of it, for the spine.
    fn read_fraction(&self) -> Option<f32> {
        let (scroll, max) = self.scroll_range()?;
        (max > 0.0).then(|| (scroll / max).clamp(0.0, 1.0))
    }

    /// The inverse: a place on the spine is a place in the file.
    fn frac_to_scroll(&self, frac: f32) -> f32 {
        match self.scroll_range() {
            Some((_, max)) => frac.clamp(0.0, 1.0) * max,
            None => 0.0,
        }
    }

    // -- overlays ----------------------------------------------------------

    fn palette_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let screen = ctx.content_rect();
        let width = (560.0 * self.scale)
            .clamp(560.0, 820.0)
            .min((screen.width() - 40.0).max(360.0));
        // The launcher sits a third of the way down, like a spotlight, not against the
        // top edge like a toolbar.
        let top = if self.scale > 1.3 || screen.height() <= 700.0 {
            screen.top() + 72.0
        } else {
            screen.top() + screen.height() / 3.0
        };
        let pos = Pos2::new(screen.center().x - width * 0.5, top);
        let needle = self.query.trim().to_string();

        // Matching files. The list is capped, but the reader is told how many were
        // matched; a silent cap makes a file look absent.
        let (matches, matched_total): (Vec<(i32, usize, Vec<usize>)>, usize) =
            match &self.folder {
                Some(s) => {
                    let mut v: Vec<(i32, usize, Vec<usize>)> = s
                        .files
                        .iter()
                        .enumerate()
                        .filter_map(|(i, f)| {
                            fuzzy::match_path(&f.rel, &needle)
                                .map(|h| (h.score, i, h.positions))
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
        if self.palette_files_only {
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
                if let Some(spec) = LAUNCH_COMMANDS
                    .iter()
                    .find(|s| s.action == Launch::Index)
                {
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
                if let Some(score) = launch_command_score(spec, &needle) {
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

        let row_count = rows.len();
        self.palette_sel = self.palette_sel.min(row_count.saturating_sub(1));
        let hint = if self.palette_files_only {
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
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(hint)
                        .font(input_font.clone())
                        .desired_width(width - 24.0)
                        .frame(Frame::NONE),
                );
                if !resp.has_focus() {
                    resp.request_focus();
                }
                if resp.changed() {
                    self.palette_sel = 0;
                }
                ui.add_space(6.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(6.0);

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
                    self.palette_sel = (self.palette_sel + 1) % row_count;
                }
                if ku && row_count > 0 {
                    self.palette_sel = self
                        .palette_sel
                        .checked_sub(1)
                        .unwrap_or(row_count.saturating_sub(1));
                }
                if kenter && row_count > 0 {
                    chosen = Some(self.palette_sel.min(row_count - 1));
                }

                let row_h = (28.0 * self.scale).clamp(28.0, 46.0);
                let file_row_h = (44.0 * self.scale).clamp(44.0, 64.0);
                let group_count = if self.palette_files_only || !needle.is_empty() {
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
                let content_h = rows_h
                    + group_count as f32 * (28.0 + 14.0 * self.scale)
                    + 44.0 * self.scale;
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
                let command_font = self.typo.ui_id(13.0 * self.scale);
                let detail_font = self.typo.ui_id(11.5 * self.scale);
                let file_font = self.typo.code_id(12.5 * self.scale);
                let file_dir_font = self.typo.code_id(10.5 * self.scale);
                let group_font = self.typo.ui_id(9.5 * self.scale);
                let key_font = self.typo.code_id(10.5 * self.scale);
                let mut last_group: Option<LaunchGroup> = None;

                for (i, row) in rows.iter().enumerate() {
                    if !self.palette_files_only && needle.is_empty() {
                        let group = row.group();
                        if last_group != Some(group) {
                            ui.add_space(if last_group.is_none() { 2.0 } else { 9.0 });
                            paint_tracked(
                                ui.painter(),
                                Pos2::new(ui.min_rect().left() + 10.0, ui.cursor().top() + 2.0),
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

                    let selected = i == self.palette_sel;
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
                                    Pos2::new(
                                        label.right() + 8.0 * self.scale,
                                        rect.center().y,
                                    ),
                                    Align2::LEFT_CENTER,
                                    detail,
                                    detail_font.clone(),
                                    detail_ink,
                                );
                            }
                            if let Some(shortcut) = spec.shortcut {
                                let galley = ui.painter().layout_no_wrap(
                                    shortcut.to_string(),
                                    key_font.clone(),
                                    ink,
                                );
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
                                ui.painter().rect_filled(chip, CornerRadius::same(3), pal.panel_deep);
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
                            let right = if *marked { 24.0 * self.scale } else { 10.0 * self.scale };
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
                            let block_h = name_h
                                + dir_galley.as_ref().map(|g| gap + g.size().y).unwrap_or(0.0);
                            let top = rect.center().y - block_h * 0.5;
                            let x = rect.left() + pad;
                            ui.painter().galley(Pos2::new(x, top), name_galley, ink);
                            if let Some(g) = dir_galley {
                                ui.painter().galley(
                                    Pos2::new(x, top + name_h + gap),
                                    g,
                                    pal.ink_soft,
                                );
                            }
                            if *marked {
                                ui.painter().text(
                                    Pos2::new(
                                        rect.right() - 8.0 * self.scale,
                                        rect.center().y,
                                    ),
                                    Align2::RIGHT_CENTER,
                                    self.typo.marks.pick("✦"),
                                    file_font.clone(),
                                    pal.accent,
                                );
                            }
                            resp.clone().on_hover_text(rel.as_str());
                        }
                    }
                    if resp.clicked() {
                        chosen = Some(i);
                    }
                }

                let files_shown = rows.iter().filter(|r| r.is_file()).count();
                let commands_shown = rows.len() - files_shown;
                ui.add_space(6.0 * self.scale);
                if rows.is_empty() {
                    ui.painter().text(
                        Pos2::new(ui.min_rect().left() + 10.0, ui.cursor().top() + 3.0),
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
                            note = format!("{files_shown} of {matched_total} files");
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
                        Pos2::new(ui.min_rect().left() + 10.0, ui.cursor().top() + 3.0),
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
            && self.frame > self.palette_opened_frame
            && ctx.input(|i| i.pointer.primary_clicked())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| {
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
            self.palette_open = false;
        }
    }

    /// `/`: an incremental search, drawn where vim would draw it — the bottom edge.
    fn search_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let font = self.typo.ui_id(13.5 * self.scale);

        // Recompute on every keystroke, and jump to the first hit as it appears.
        let changed = self.search_seen != self.search_query;
        if changed {
            self.search_seen = self.search_query.clone();
            self.search_hits = self
                .open
                .as_ref()
                .map(|o| vim::matches(&o.doc, &self.search_query))
                .unwrap_or_default();
            if let Some(first) = self.search_hits.first().copied() {
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
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("search this file")
                            .font(font.clone())
                            .desired_width(width - 110.0)
                            .frame(Frame::NONE),
                    );
                    if !resp.has_focus() {
                        resp.request_focus();
                    }
                    let count = if self.search_hits.is_empty() {
                        "no match".to_string()
                    } else {
                        format!("{} match(es)", self.search_hits.len())
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
            self.search_open = false;
            self.chords.clear();
        }
    }

    /// How many controls the current settings section has, excluding the reset action.
    fn settings_focus_count(&self) -> usize {
        match self.settings_tab {
            0 => 14,
            1 => self.typo.mono_voices.len() + self.typo.prose_voices.len(),
            2 => theme::THEMES.len(),
            3 => 1,
            _ => 0,
        }
    }

    /// The settings sheet's keyboard. Tab walks the rail, the controls and the reset
    /// action; arrows move; Space and Enter act. Letters are left to the text fields.
    fn settings_key(
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
        if self.settings_tab >= tabs {
            self.settings_tab = tabs - 1;
        }
        let count = self.settings_focus_count();
        let total = count + 1;
        if self.settings_rail_focus {
            match key {
                Key::ArrowUp | Key::ArrowLeft => {
                    self.settings_tab = (self.settings_tab + tabs - 1) % tabs;
                    self.settings_focus = 0;
                    true
                }
                Key::ArrowDown | Key::ArrowRight => {
                    self.settings_tab = (self.settings_tab + 1) % tabs;
                    self.settings_focus = 0;
                    true
                }
                Key::Tab | Key::Enter | Key::Space => {
                    self.settings_rail_focus = false;
                    self.settings_focus = 0;
                    true
                }
                _ => false,
            }
        } else {
            self.settings_focus = self.settings_focus.min(total.saturating_sub(1));
            match key {
                Key::Tab => {
                    if shift {
                        if self.settings_focus == 0 {
                            self.settings_rail_focus = true;
                        } else {
                            self.settings_focus -= 1;
                        }
                    } else {
                        self.settings_focus += 1;
                        if self.settings_focus >= total {
                            self.settings_rail_focus = true;
                            self.settings_focus = 0;
                        }
                    }
                    true
                }
                Key::ArrowUp | Key::ArrowDown => {
                    if key == Key::ArrowDown {
                        if self.settings_focus < total.saturating_sub(1) {
                            self.settings_focus += 1;
                        }
                    } else if self.settings_focus > 0 {
                        self.settings_focus -= 1;
                    }
                    true
                }
                Key::ArrowLeft | Key::ArrowRight => {
                    let direction = if key == Key::ArrowLeft { -1 } else { 1 };
                    if self.settings_tab == 0
                        && matches!(self.settings_focus, 11..=13)
                    {
                        self.settings_adjust(ctx, self.settings_focus, direction as f32);
                    } else if self.settings_focus < count {
                        self.settings_focus =
                            (self.settings_focus as i32 + direction).clamp(0, count as i32 - 1)
                                as usize;
                    }
                    true
                }
                Key::Space | Key::Enter => {
                    self.settings_activate(ctx, self.settings_focus);
                    true
                }
                _ => false,
            }
        }
    }

    /// Activate the focused settings control. `count` itself is the reset action.
    fn settings_activate(&mut self, ctx: &egui::Context, focus: usize) {
        let count = self.settings_focus_count();
        if focus >= count {
            let now = ctx.input(|i| i.time);
            let armed = self.reset_asked.is_some_and(|t| now - t < 5.0);
            if armed {
                self.reset_asked = None;
                self.reset_defaults(ctx);
                self.settings_rail_focus = true;
                self.settings_focus = 0;
            } else {
                self.reset_asked = Some(now);
            }
            return;
        }
        match self.settings_tab {
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

    fn settings_toggle_page(&mut self, ctx: &egui::Context, focus: usize) {
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
    fn settings_adjust(&mut self, ctx: &egui::Context, focus: usize, direction: f32) {
        match focus {
            11 => {
                let next = (self.store.config.measure as f32 + direction)
                    .clamp(crate::catalog::MEASURE_MIN as f32, crate::catalog::MEASURE_MAX as f32);
                self.store.config.measure = next.round() as usize;
                self.style_dirty = true;
            }
            12 => {
                self.store.config.leading = (self.store.config.leading + direction * 0.02)
                    .clamp(crate::catalog::LEADING_MIN, crate::catalog::LEADING_MAX);
                self.style_dirty = true;
            }
            13 => {
                self.scale = (self.scale + direction * 0.02)
                    .clamp(crate::catalog::SCALE_MIN, crate::catalog::SCALE_MAX);
                self.store.config.font_scale = self.scale;
                self.style_dirty = true;
                self.rebuild_metrics(ctx);
            }
            _ => {}
        }
        self.store.save_config();
    }

    /// Settings: a modal screen, a rail of sections, and one section at a time.
    ///
    /// Modal because settings are a place you go, not a card that floats over the page and
    /// lets a scroll wheel move the text underneath you. The page is still there — veiled
    /// in the theme's own ink — and it comes back the way it was.
    fn settings_overlay(&mut self, ctx: &egui::Context) {
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
        let label_w =
            19.0 * (12.5 * self.scale) * 0.5 + (12.5 * self.scale) * 0.88 + 9.0;
        let single = ((content - 12.0) * 0.5) < label_w;
        let mut open = self.show_settings;
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

        let modal = egui::Modal::new(egui::Id::new("zenith-settings"))
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
                            let selected = self.settings_tab == i;
                            let text = RichText::new(*name).font(head.clone()).color(if selected {
                                pal.ink
                            } else {
                                pal.ink_soft
                            });
                            let response =
                                ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                            if selected && self.settings_rail_focus {
                                ui.painter().rect_filled(
                                    Rect::from_min_size(
                                        Pos2::new(response.rect.left() - 8.0, response.rect.top() + 2.0),
                                        vec2(2.0, (response.rect.height() - 4.0).max(2.0)),
                                    ),
                                    CornerRadius::same(1),
                                    pal.accent,
                                );
                            }
                            if response.clicked() {
                                self.settings_tab = i;
                                self.settings_rail_focus = true;
                                self.settings_focus = 0;
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
                        let focus = if self.settings_rail_focus {
                            None
                        } else {
                            Some(self.settings_focus)
                        };
                        match self.settings_tab {
                            // -------------------------------------------- page
                            0 => {
                                if single {
                                    let cfg = &mut self.store.config;
                                    ui.vertical(|ui| {
                                        ui.set_width(content);
                                        let mut base = 0;
                                        changed |= toggles(ui, &font, &pal, &mut [
                                            ("soft wrap", &mut cfg.wrap),
                                            ("indentation whispers", &mut cfg.guides),
                                            ("comments as prose", &mut cfg.serif_comments),
                                            ("focus lens", &mut cfg.focus_lens),
                                            ("line numbers", &mut cfg.line_numbers),
                                            ("compress blank lines", &mut cfg.compress_blank),
                                            ("distilled types", &mut cfg.distilled),
                                            ("themed window bar", &mut cfg.frameless),
                                            ("render markdown", &mut cfg.markdown),
                                        ], focus, &mut base);
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("the tree")
                                                .font(small.clone())
                                                .color(pal.ink_soft),
                                        );
                                        let tree = toggles(ui, &font, &pal, &mut [
                                            ("source files only", &mut cfg.code_only),
                                            ("include hidden files", &mut cfg.show_hidden),
                                        ], focus, &mut base);
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
                                            changed |= toggles(ui, &font, &pal, &mut [
                                                ("soft wrap", &mut cfg.wrap),
                                                ("indentation whispers", &mut cfg.guides),
                                                ("comments as prose", &mut cfg.serif_comments),
                                                ("focus lens", &mut cfg.focus_lens),
                                                ("line numbers", &mut cfg.line_numbers),
                                            ], focus, &mut left);
                                        });
                                        ui.vertical(|ui| {
                                            ui.set_width(half);
                                            changed |= toggles(ui, &font, &pal, &mut [
                                                ("compress blank lines", &mut cfg.compress_blank),
                                                ("distilled types", &mut cfg.distilled),
                                                ("themed window bar", &mut cfg.frameless),
                                                ("render markdown", &mut cfg.markdown),
                                            ], focus, &mut right);
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
                                            let tree = toggles(ui, &font, &pal, &mut [
                                                ("source files only", &mut cfg.code_only),
                                                ("include hidden files", &mut cfg.show_hidden),
                                            ], focus, &mut tree_base);
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
                                        range: crate::catalog::MEASURE_MIN as f32
                                            ..=crate::catalog::MEASURE_MAX as f32,
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
                                        range: crate::catalog::LEADING_MIN
                                            ..=crate::catalog::LEADING_MAX,
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
                                        range: crate::catalog::SCALE_MIN
                                            ..=crate::catalog::SCALE_MAX,
                                        decimals: 2,
                                    },
                                    focus == Some(13),
                                );
                            }
                            // -------------------------------------------- type
                            1 => {
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
                                            r.on_hover_text(
                                                RichText::new(v.note.clone()).font(small.clone()),
                                            );
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
                                            r.on_hover_text(
                                                RichText::new(v.note.clone()).font(small.clone()),
                                            );
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
                                        RichText::new(
                                            "no system fonts found: egui's bundled faces are in use",
                                        )
                                        .font(small.clone())
                                        .color(pal.ink_soft),
                                    );
                                }
                            }
                            // -------------------------------------------- theme
                            2 => {
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
                                            let (rect, response) = ui.allocate_exact_size(
                                                vec2(tile_w, tile_h),
                                                egui::Sense::click(),
                                            );
                                            theme_tile(
                                                ui,
                                                rect,
                                                p,
                                                &sample,
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
                                            response.on_hover_text(
                                                RichText::new(p.mood).font(small.clone()),
                                            );
                                            ui.add_space(10.0);
                                        }
                                    });
                                    ui.add_space(10.0);
                                }
                            }
                            // -------------------------------------------- language
                            3 => {
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

                                if self.lsp_kinds.is_empty() {
                                    ui.label(
                                        RichText::new(
                                            "nothing in this folder speaks a language zenith \
                                             has a server for",
                                        )
                                        .font(small.clone())
                                        .color(pal.ink_soft),
                                    );
                                }
                                for kind in self.lsp_kinds.clone() {
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
                                        ui.label(
                                            RichText::new(state).font(small.clone()).color(ink),
                                        );
                                    });
                                    if let Some(why) = why {
                                        ui.label(
                                            RichText::new(why)
                                                .font(small.clone())
                                                .color(pal.ink_soft),
                                        );
                                    }
                                    let discovered = self
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
                                                discovered.clone().unwrap_or_else(|| {
                                                    "path to the binary".into()
                                                }),
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
                                        self.lsp_path_dirty.insert(kind.id.to_string());
                                    }
                                    if edit.lost_focus() && self.lsp_path_dirty.remove(kind.id) {
                                        // Finishing the edit restarts that language's
                                        // server, so the new path is the one in use. A
                                        // field visited and left alone is not an edit.
                                        restart = Some(kind.id.to_string());
                                    }
                                    ui.add_space(8.0);
                                }
                            }
                            // -------------------------------------------- keys
                            4 => {
                                ui.visuals_mut().extreme_bg_color = pal.panel_deep;
                                let edit = ui.add(
                                    egui::TextEdit::singleline(&mut self.keys_query)
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
                                let query = self.keys_query.to_lowercase();
                                ScrollArea::vertical()
                                    .id_salt("keys")
                                    .max_height(280.0)
                                    .scroll_bar_visibility(
                                        egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                                    )
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
                                                                .font(self.typo.code_id(
                                                                    11.5 * self.scale,
                                                                ))
                                                                .color(pal.accent),
                                                        );
                                                        ui.label(
                                                            RichText::new(v)
                                                                .font(small.clone())
                                                                .color(pal.ink_soft),
                                                        );
                                                        ui.end_row();
                                                    }
                                                });
                                            ui.add_space(8.0);
                                        }
                                    });
                            }
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
                let reset_focused = !self.settings_rail_focus
                    && self.settings_focus == self.settings_focus_count();
                let text = if armed { "restore defaults?" } else { "restore defaults" };
                let colour = if armed { pal.tone(Tone::Invalid) } else { pal.ink_soft };
                let galley = ui.painter().layout_no_wrap(text.to_string(), small.clone(), colour);
                let button_h = (small.size * 2.2).max(26.0);
                let button_w = galley.size().x + 20.0;
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let (rect, reset) = ui.allocate_exact_size(
                        vec2(button_w, button_h),
                        egui::Sense::click(),
                    );
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
        self.show_settings = open;
    }

    /// Put every piece of taste back to how it shipped.
    ///
    /// The servers' paths stay: a path is something the reader went and found on their own
    /// machine, not a preference, and throwing it away would cost them the search again.
    fn reset_defaults(&mut self, ctx: &egui::Context) {
        let mut fresh = crate::catalog::Config::default();
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
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(
            if self.palette.is_dark() {
                egui::SystemTheme::Dark
            } else {
                egui::SystemTheme::Light
            },
        ));
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
    fn server_state(&self, id: &str) -> (String, Option<String>) {
        if !self.store.config.lsp.enabled {
            // Not running is not the same as not found: nothing was looked for.
            return ("off".into(), None);
        }
        if let Some(session) = self.servers.iter().find(|s| s.id() == id) {
            return match session.state() {
                lsp::State::Starting => ("starting".into(), None),
                lsp::State::Ready => ("ready".into(), None),
                lsp::State::Failed(why) => ("failed".into(), Some(why.clone())),
            };
        }
        if self.available.iter().any(|a| a.kind.id == id) {
            ("found".into(), None)
        } else if self.discovering.is_some() {
            ("looking…".into(), None)
        } else {
            ("not found".into(), None)
        }
    }

    /// Stop a language's server so the next question starts it again — with the path the
    /// reader has just written down.
    fn restart_server(&mut self, id: &str) {
        for session in self.servers.iter_mut().filter(|s| s.id() == id) {
            session.stop();
        }
        self.servers.retain(|s| s.id() != id);
        self.available.retain(|a| a.kind.id != id);
        self.outline_asked = None;
        let Some(root) = self.folder.as_ref().map(|f| f.root.clone()) else {
            return;
        };
        let kinds: Vec<&'static lsp::servers::Kind> =
            self.lsp_kinds.iter().filter(|k| k.id == id).copied().collect();
        self.discover(kinds, root);
    }

    // -- language servers --------------------------------------------------

    /// Find the servers for the languages in this folder, in the background.
    ///
    /// Discovery asks binaries their version, which means spawning processes: never on
    /// the frame thread, and never before there is a folder to search.
    fn start_discovery(&mut self) {
        // The languages are a fact about the folder, so the panel can list them even
        // when the servers are turned off.
        self.lsp_path_dirty.clear();
        self.lsp_kinds = self
            .folder
            .as_ref()
            .map(|f| lsp::servers::kinds_among(f.files.iter().map(|e| e.path.as_path())))
            .unwrap_or_default();
        let root = self.folder.as_ref().map(|f| f.root.clone());
        // A server already reading *this* folder is still reading this folder: re-walking
        // the tree to hide or show a file is not a reason to make it start again. One
        // rooted anywhere else is another folder's server and is let go.
        let enabled = self.store.config.lsp.enabled;
        let mut kept = Vec::new();
        for mut session in std::mem::take(&mut self.servers) {
            if enabled && root.as_ref() == Some(&session.root) {
                kept.push(session);
            } else {
                session.stop();
            }
        }
        self.servers = kept;
        self.nav = None;
        self.available.clear();
        self.discovering = None;
        if !enabled {
            return;
        }
        let Some(root) = root else { return };
        let kinds = self.lsp_kinds.clone();
        if kinds.is_empty() {
            return;
        }
        self.discover(kinds, root);
    }

    /// Look for servers for these languages, off the frame thread: `find` runs the
    /// binaries it finds, which is not something a frame may wait for.
    fn discover(&mut self, kinds: Vec<&'static lsp::servers::Kind>, root: PathBuf) {
        if kinds.is_empty() {
            return;
        }
        let overrides: Vec<Option<lsp::servers::Override>> = kinds
            .iter()
            .map(|kind| {
                self.store
                    .config
                    .lsp
                    .servers
                    .get(kind.id)
                    .map(|s| lsp::servers::Override {
                        command: s.command.clone(),
                        args: s.args.clone(),
                    })
            })
            .collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let look_here = root.clone();
        std::thread::spawn(move || {
            let found = kinds
                .iter()
                .zip(overrides)
                .filter_map(|(kind, over)| lsp::servers::find(kind, &look_here, over.as_ref()))
                .collect();
            let _ = tx.send(found);
        });
        self.discovering = Some(Discovery { rx, root });
    }

    fn poll_discovery(&mut self) {
        let Some(job) = &self.discovering else { return };
        let root = job.root.clone();
        match job.rx.try_recv() {
            Ok(found) => {
                self.discovering = None;
                // The reader may have opened another folder while this ran.
                if self.folder.as_ref().map(|f| f.root.clone()) == Some(root) {
                    // Merged rather than replaced: a restart looks for one language, and
                    // must not forget the servers the others already have.
                    for server in found {
                        self.available.retain(|a| a.kind.id != server.kind.id);
                        self.available.push(server);
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.discovering = None,
        }
    }

    /// Press `gd` at a place, for a test with nobody at the keyboard.
    ///
    /// `ZENITH_GOTO=LINE:COL` asks at that place in whatever file is open, then reports
    /// where it landed and exits. The language-server path is the one thing in the app
    /// that cannot be checked by rendering a frame: the question, the handshake and the
    /// arrival only happen when a key is pressed, and a key cannot be pressed headlessly.
    #[cfg(debug_assertions)]
    fn dev_nav(&mut self, ctx: &egui::Context) {
        // A landing may leave nothing asking for another frame, and this hook can only
        // report what it sees; keep the frames coming while it is watching.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let Ok(spec) = std::env::var("ZENITH_GOTO") else { return };
        // `ZENITH_GOTO_FROM=LINE` puts the reader somewhere first, so that a jump has a
        // direction to have. It is re-applied for a few frames because the page does not
        // know its own height on the first one.
        if let Some(line) = std::env::var("ZENITH_GOTO_FROM")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            if !self.nav_tried && self.frame < 40 {
                self.pending = Pending::Top(line.saturating_sub(1));
                return;
            }
        }
        // Fire once discovery has had its chance — or once it is clear it never will,
        // which is what happens when servers are turned off.
        if !self.nav_tried && (!self.available.is_empty() || self.frame > 90) {
            self.nav_tried = true;
            let mut parts = spec.rsplitn(3, ':');
            let col: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            let line: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            self.caret = vim::Caret::new(line.saturating_sub(1), col.saturating_sub(1));
            eprintln!(
                "nav: asking at {line}:{col} — {} server(s) available, {} running",
                self.available.len(),
                self.servers.len()
            );
            self.goto_definition();
            eprintln!("nav: after gd — status {:?}", self.status);
        }
        if self.nav_tried && self.nav.is_none() {
            // The page only acts on a landing when it draws, so let it draw first.
            self.nav_settled += 1;
            if self.nav_settled < 4 {
                return;
            }
            // `ZENITH_THEN=back|close` presses the key that undoes a jump, so that the
            // way back can be checked without a hand on the keyboard.
            if !self.then_done {
                self.then_done = true;
                match std::env::var("ZENITH_THEN").as_deref() {
                    Ok("back") => {
                        self.jump(-1);
                        eprintln!(
                            "nav: back to {} line {}",
                            self.open
                                .as_ref()
                                .map(|o| crate::catalog::display_path(&o.path))
                                .unwrap_or_default(),
                            self.caret.line + 1
                        );
                    }
                    Ok("close") => {
                        self.close_file();
                        eprintln!(
                            "nav: closed to {} line {}",
                            self.open
                                .as_ref()
                                .map(|o| crate::catalog::display_path(&o.path))
                                .unwrap_or_else(|| "(the reading room)".into()),
                            self.caret.line + 1
                        );
                    }
                    _ => {}
                }
                self.nav_settled = 0;
                return;
            }
            // Exit is not a shutdown here: the servers have to be let go first, or the
            // child processes outlive the test.
            for session in &mut self.servers {
                session.stop();
            }
            let outline: Vec<String> = self
                .open
                .as_ref()
                .map(|o| o.outline.iter().take(4).map(|s| format!("{}:{}", s.name, s.line + 1)).collect())
                .unwrap_or_default();
            eprintln!(
                "nav: landed at line {} col {}, status {:?}",
                self.caret.line + 1,
                self.caret.col + 1,
                self.status
            );
            eprintln!(
                "nav: outline {} entries, first: {}",
                self.open.as_ref().map(|o| o.outline.len()).unwrap_or(0),
                outline.join(", ")
            );
            eprintln!(
                "nav: scroll {:.0} of a {:.0} viewport, page top line {}",
                self.scroll,
                self.view_h,
                self.open
                    .as_ref()
                    .and_then(|o| o.laid.as_ref())
                    .map(|l| l.line_at(self.scroll) + 1)
                    .unwrap_or(0)
            );
            std::process::exit(0);
        }
        if self.frame > 900 {
            eprintln!(
                "nav: gave up — status {:?}, nav {:?}, servers {:?}",
                self.status,
                self.nav.as_ref().map(|n| (n.id, n.tries)),
                self.servers
                    .iter()
                    .map(|s| (s.id(), format!("{:?}", s.state())))
                    .collect::<Vec<_>>()
            );
            std::process::exit(1);
        }
    }

    /// Say something in the strip. Saying the same thing twice still counts as saying it.
    fn say(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_seq += 1;
    }

    /// Where is the thing under the cursor defined?
    fn goto_definition(&mut self) {
        let Some(open) = &self.open else {
            self.status = "gd: no file is open".into();
            return;
        };
        let path = open.path.clone();
        let line = self.caret.line;
        let unit = open
            .doc
            .lines
            .get(line)
            .map(|l| lsp::text::position(l, self.caret.col))
            .unwrap_or(0);
        if !self.store.config.lsp.enabled {
            self.status = "gd: language servers are off, in settings".into();
            return;
        }
        let Some((kind, _)) = lsp::servers::kind_for(&path) else {
            self.status = "gd: no language server serves this kind of file".into();
            return;
        };
        if self.discovering.is_some() && self.available.is_empty() {
            self.status = format!("gd: still looking for a {} language server…", kind.title);
            return;
        }
        if !self.available.iter().any(|a| a.kind.id == kind.id) {
            self.status = format!(
                "gd: no {} language server on this machine — `zenith --lsp` says what it looks for",
                kind.title
            );
            return;
        }
        let root = self
            .folder
            .as_ref()
            .map(|f| f.root.clone())
            .unwrap_or_else(|| crate::catalog::project_root(&path));
        self.begin_nav(root, path, line, unit);
    }

    /// Start a server for the language being read, if one was found and is not running.
    ///
    /// This happens when a file of that language is opened, not when `gd` is pressed: the
    /// point of finding a server is that it starts reading the project while the reader
    /// reads, so that by the time a name needs following the answer is already there.
    fn ensure_servers(&mut self) {
        if !self.store.config.lsp.enabled {
            return;
        }
        let Some(path) = self.open.as_ref().map(|o| o.path.clone()) else {
            return;
        };
        let Some((kind, _)) = lsp::servers::kind_for(&path) else {
            return;
        };
        if self.servers.iter().any(|s| s.id() == kind.id) {
            return;
        }
        let Some(found) = self.available.iter().find(|a| a.kind.id == kind.id).cloned() else {
            return;
        };
        let root = self
            .folder
            .as_ref()
            .map(|f| f.root.clone())
            .unwrap_or_else(|| crate::catalog::project_root(&path));
        match lsp::Session::start(kind, &found, &root) {
            Ok(session) => self.servers.push(session),
            Err(e) => self.status = format!("language server: {e}"),
        }
    }

    /// Start the server if it is not running, then ask it the question.
    fn begin_nav(&mut self, root: PathBuf, path: PathBuf, line: usize, unit: u32) {
        let Some((kind, _)) = lsp::servers::kind_for(&path) else {
            return;
        };
        if self.discovering.is_some() && self.available.is_empty() {
            self.status = format!("gd: still looking for a {} language server…", kind.title);
            return;
        }
        self.ensure_servers();
        if !self.servers.iter().any(|s| s.id() == kind.id) {
            self.status = format!("gd: no {} language server on this machine", kind.title);
            return;
        }
        self.status = format!("gd: asking {}…", kind.title);
        self.nav = Some(Nav {
            id: None,
            tries: 0,
            again_at: None,
            sent_at: None,
            path,
            line,
            unit,
            root,
        });
    }

    /// Send a waiting question once its server has finished its handshake.
    fn advance_nav(&mut self) {
        let Some(nav) = &self.nav else { return };
        let (root, path) = (nav.root.clone(), nav.path.clone());
        let (line, unit) = (nav.line, nav.unit);
        let (id, sent, again) = (nav.id, nav.sent_at, nav.again_at);
        let kind = lsp::servers::kind_for(&path).map(|(k, _)| k.id);
        let Some(index) = self
            .servers
            .iter()
            .position(|s| s.root == root && Some(s.id()) == kind)
        else {
            // The server this question was waiting for is gone — it failed its handshake,
            // or the folder changed underneath it. Stop waiting: an unanswered question
            // must not keep asking for frames.
            self.nav = None;
            return;
        };

        // A question that has gone out is waited for, but not forever, and the waiting
        // is longest while the server is still reading the project — that wait is the
        // honest one, and the strip says so.
        if id.is_some() {
            let waited = sent.map(|t| t.elapsed()).unwrap_or_default();
            let idle = self.servers[index].is_idle();
            let limit = if idle {
                std::time::Duration::from_secs(5)
            } else {
                std::time::Duration::from_secs(25)
            };
            if waited > limit {
                let title = self.servers[index].kind.title;
                self.nav = None;
                if idle {
                    self.say("gd: the server did not answer");
                } else {
                    self.say(format!("gd: {title} is still reading this project"));
                }
            } else if !idle {
                let title = self.servers[index].kind.title;
                self.say(format!("gd: {title} is still reading this project…"));
            }
            return;
        }
        if again.is_some_and(|t| t > std::time::Instant::now()) {
            return;
        }
        if *self.servers[index].state() != lsp::State::Ready {
            return;
        }
        match self.servers[index].definition(&path, line as u32, unit) {
            Ok(id) => {
                if let Some(nav) = &mut self.nav {
                    nav.id = Some(id);
                    nav.sent_at = Some(std::time::Instant::now());
                }
            }
            Err(e) => {
                self.nav = None;
                self.say(format!("gd: {e}"));
            }
        }
    }

    /// Ask the server for the outline of the document being read, once.
    ///
    /// Until it answers — and if it never does — the panel shows what the grammar
    /// guessed, which is why nothing has to wait for a server to have a contents list.
    fn ensure_outline(&mut self) {
        if !self.store.config.lsp.enabled {
            return;
        }
        let Some(path) = self.open.as_ref().map(|o| o.path.clone()) else {
            return;
        };
        if self.outline_asked.as_deref() == Some(path.as_path()) {
            return;
        }
        let kind = lsp::servers::kind_for(&path).map(|(k, _)| k.id);
        let Some(session) = self
            .servers
            .iter_mut()
            .find(|s| *s.state() == lsp::State::Ready && Some(s.id()) == kind)
        else {
            return;
        };
        if session.outline(&path).is_ok() {
            self.outline_asked = Some(path);
        }
    }

    /// Take whatever the servers have said since the last frame.
    fn poll_lsp(&mut self) {
        if self.servers.is_empty() {
            return;
        }
        let asked = self.nav.as_ref().and_then(|n| n.id);
        let mut tries = 0u32;
        let mut land: Option<Vec<lsp::Target>> = None;
        let mut gone: Vec<String> = Vec::new();
        for session in &mut self.servers {
            for answer in session.poll() {
                match answer {
                    lsp::Answer::Ready => {}
                    lsp::Answer::Ended(why) => {
                        let mut line = format!("{} — {why}", session.kind.title);
                        if let Some(fix) = lsp::servers::advice(session.kind, &why) {
                            line.push_str(" · ");
                            line.push_str(&fix);
                        }
                        gone.push(line);
                    }
                    lsp::Answer::Found { id, targets } => {
                        if Some(id) != asked {
                            continue;
                        }
                        if targets.is_empty() && !session.is_idle() && tries < 12 {
                            // Ask again shortly: the server is reading the project, and
                            // "nothing is defined there" would be a lie about a file it
                            // has not opened yet.
                            tries += 1;
                            if let Some(nav) = &mut self.nav {
                                nav.id = None;
                                nav.tries += 1;
                                nav.again_at =
                                    Some(std::time::Instant::now() + std::time::Duration::from_millis(600));
                            }
                            self.status = format!(
                                "gd: {} is still reading this project…",
                                session.kind.title
                            );
                        } else {
                            land = Some(targets);
                        }
                    }
                    lsp::Answer::Refused { id, message } => {
                        if Some(id) == asked {
                            if lsp::transient(&message) && tries < 5 {
                                tries += 1;
                                if let Some(nav) = &mut self.nav {
                                    nav.id = None;
                                    nav.tries += 1;
                                    nav.again_at = Some(
                                        std::time::Instant::now()
                                            + std::time::Duration::from_millis(250),
                                    );
                                }
                                self.status =
                                    format!("gd: {} is still settling…", session.kind.title);
                            } else {
                                self.nav = None;
                                self.status = format!("gd: {message}");
                            }
                        }
                    }
                    lsp::Answer::Outline(symbols) => {
                        // A server's outline may arrive after the reader has moved on.
                        let waiting = self.outline_asked.clone();
                        if waiting.is_none() || !symbols.is_empty() {
                            if let (Some(open), Some(waiting)) = (&mut self.open, waiting) {
                                if open.path == waiting {
                                    open.outline = symbols
                                        .into_iter()
                                        .map(|s| Symbol {
                                            line: s.line,
                                            name: s.name,
                                            kind: s.kind,
                                            depth: s.depth,
                                            detail: s.detail,
                                        })
                                        .collect();
                                }
                            }
                        }
                    }
                }
            }
        }
        if !gone.is_empty() {
            self.status = format!("gd: {}", gone.join("; "));
            self.nav = None;
            self.servers
                .retain(|s| !matches!(s.state(), lsp::State::Failed(_)));
        }
        if let Some(targets) = land {
            self.nav = None;
            self.land(targets);
        }
    }

    /// Go where the server said. One target is a teleport; several are a page opening at
    /// the first of them, and the count says so rather than pretending there was one.
    fn land(&mut self, targets: Vec<lsp::Target>) {
        let Some(first) = targets.first() else {
            self.status = "gd: nothing at that place".into();
            return;
        };
        let count = targets.len();
        let there = first.place();
        let path = first.path.clone();
        let (line, unit) = (first.line, first.unit);
        let here = self.open.as_ref().map(|o| o.path.clone());
        let left = self
            .open
            .as_ref()
            .map(|o| o.rel.clone())
            .unwrap_or_default();
        // Is the definition outside the folder being read? A dependency's source is
        // legitimate reading, but the reader has left the page they were on.
        let outside = self
            .folder
            .as_ref()
            .map(|f| !path.starts_with(&f.root))
            .unwrap_or(true);
        let previous = self
            .open
            .as_ref()
            .map(|o| (o.path.clone(), self.scroll));
        let (previous, _) = (previous, self.scroll);
        if here.as_deref() != Some(path.as_path()) {
            self.push_jump();
            self.open_file(&path);
        }
        let Some(open) = &self.open else { return };
        let line = line.min(open.doc.lines.len().saturating_sub(1));
        let col = open
            .doc
            .lines
            .get(line)
            .map(|l| lsp::text::column(l, unit))
            .unwrap_or(0);
        let caret = vim::Caret::new(line, col);
        if here.as_deref() == Some(path.as_path()) && caret != self.caret {
            self.push_jump();
        }
        // Following a definition is an arrival, not a nudge: the line comes to the top
        // of the page, which is where someone who has just arrived starts reading. Going
        // *backwards* is the exception — a line put at the top when it was above you
        // takes away the context you were reading, so it arrives in the middle instead,
        // with the code that leads into it still on screen.
        let above = match (&self.open, &previous) {
            (Some(open), Some((path, top))) if open.path == *path => {
                let first = open.laid.as_ref().map(|l| l.line_at(*top)).unwrap_or(0);
                caret.line < first
            }
            _ => false,
        };
        self.caret = caret;
        self.ensure_visible();
        self.pending = if above {
            Pending::Centre(caret.line)
        } else {
            Pending::Top(caret.line)
        };
        let mut said = if count > 1 {
            format!("→ {there} (1 of {count})")
        } else {
            format!("→ {there}")
        };
        if outside && !left.is_empty() {
            // The way back is not visible from here: the tree does not hold this file,
            // and nothing on the page says which key undoes the jump.
            said.push_str(&format!(" · ctrl-o back to {left}"));
        }
        self.status = said;
    }

    // -- smoke -------------------------------------------------------------

    /// A message in the strip, for photographing it: `ZENITH_FORCE_NOTICE="…"`.
    ///
    /// The status strip is the one piece of chrome whose job is to be read at a glance,
    /// and a glance is what a screenshot is for.
    #[cfg(debug_assertions)]
    fn forced_notice(&mut self) {
        let Ok(text) = std::env::var("ZENITH_FORCE_NOTICE") else {
            return;
        };
        if self.frame <= 2 {
            self.status = text;
        }
    }

    fn smoke_tick(&mut self, ctx: &egui::Context) {
        if self.smoke.is_none() {
            return;
        }
        // A self-portrait has to be reproducible, so each phase states its own
        // conditions explicitly rather than inheriting whatever was last saved.
        if self.smoke.as_ref().map(|s| s.frame) == Some(0) {
            match self.smoke.as_ref().map(|s| s.phase) {
                Some(0) => {
                    self.store.config.focus_lens = false;
                    self.store.config.distilled = false;
                    self.caret = vim::Caret::default();
                    self.pending = Pending::Restore;
                    // `ZENITH_CARET=LINE:COL` puts the cursor somewhere specific, for
                    // photographing what it looks like there.
                    if let Some(place) = std::env::var("ZENITH_CARET").ok().and_then(|v| {
                        let (l, c) = v.split_once(':')?;
                        Some((l.parse::<usize>().ok()?, c.parse::<usize>().ok()?))
                    }) {
                        self.caret = vim::Caret::new(place.0.saturating_sub(1), place.1.saturating_sub(1));
                        self.ensure_visible();
                    }
                }
                _ => {
                    self.store.config.focus_lens = true;
                    let nested = self
                        .folder
                        .as_ref()
                        .map(|s| s.root.join("src/reader.rs"))
                        .filter(|p| p.is_file());
                    if let Some(path) = nested {
                        self.open_file(&path);
                    }
                    self.caret = vim::Caret::new((self.line_count() / 2).max(1), 0);
                    self.centre_on_caret();
                }
            }
        }
        let Some(smoke) = &mut self.smoke else { return };
        smoke.frame += 1;
        // Ask for the next frame on a timer rather than spinning: the renderer needs an
        // idle moment between frames to service a screenshot request, and a frame loop
        // that never idles drops them.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        // Wait for the layout to settle before asking for a picture.
        // Ask for the picture, and ask again if the answer does not come. The renderer
        // drops the occasional screenshot request (it races with a frame that is already
        // in flight), and one dropped request used to mean a failed run and no images.
        if !smoke.requested || (smoke.frame - smoke.asked_at > 20 && smoke.asked < 12) {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
            smoke.requested = true;
            smoke.asked += 1;
            smoke.asked_at = smoke.frame;
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else {
            if smoke.frame > 600 {
                eprintln!("zenith: smoke — no screenshot arrived from the renderer");
                std::process::exit(3);
            }
            return;
        };

        let [w, h] = image.size;
        let mut rgb = Vec::with_capacity(w * h * 3);
        for px in &image.pixels {
            let [r, g, b, _a] = px.to_array();
            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
        let out = match smoke.phase {
            0 => smoke.out.clone(),
            _ => {
                let mut p = smoke.out.clone();
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "smoke".into());
                p.set_file_name(format!("{stem}-zen.png"));
                p
            }
        };
        if let Err(e) = std::fs::write(&out, crate::png::encode_rgb(w as u32, h as u32, &rgb)) {
            eprintln!("zenith: smoke — could not write {out:?}: {e}");
            std::process::exit(4);
        }
        println!("zenith: smoke — wrote {} ({w}×{h})", out.display());

        match smoke.phase {
            0 => {
                // Phase two: the focus lens, reading zenith's own layout engine — a
                // file with real nesting, so the lens has work to do.
                smoke.phase = 1;
                smoke.requested = false;
                smoke.asked = 0;
                smoke.asked_at = 0;
                smoke.frame = 0;
                self.store.config.wrap = true;
            }
            _ => {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// One visible line of the folder tree.
#[derive(Clone)]
enum TreeRow {
    Dir {
        name: String,
        rel: String,
        depth: usize,
        open: bool,
    },
    File {
        name: String,
        path: PathBuf,
        depth: usize,
        doorway: bool,
        empty: bool,
    },
}

/// What a row *is*, independent of where it currently sits in the list.
///
/// The selection is kept as one of these rather than as an index, because expanding a
/// folder inserts rows above the selection: an index would slide to another file, which
/// is exactly the kind of quiet misbehaviour a keyboard interface cannot afford.
#[derive(Clone, PartialEq, Eq, Debug)]
enum TreeKey {
    Dir(String),
    File(PathBuf),
}

impl TreeRow {
    fn key(&self) -> TreeKey {
        match self {
            TreeRow::Dir { rel, .. } => TreeKey::Dir(rel.clone()),
            TreeRow::File { path, .. } => TreeKey::File(path.clone()),
        }
    }

    /// For `h`: which row is this row's parent, if it has one on screen?
    fn is_child_of(&self, other: &TreeRow) -> bool {
        match (self, other) {
            (
                TreeRow::Dir { depth, .. } | TreeRow::File { depth, .. },
                TreeRow::Dir {
                    depth: pd, open, ..
                },
            ) => *open && *pd + 1 == *depth,
            // A row at the top has no row above it to go back to.
            _ => false,
        }
    }
}

/// Where does the selection go? Clamped, so the ends of the list are the ends of the list.
fn walk_index(len: usize, current: Option<usize>, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    match current {
        None => 0,
        Some(i) => (i as i32 + delta).clamp(0, len as i32 - 1) as usize,
    }
}

/// The row that the row at `idx` belongs to, for `h`.
///
/// Not simply "the row above": a folder's children are its own, and a repository belongs
/// it belongs to. Walking up means finding the nearest row above that contains it.
fn parent_index(rows: &[TreeRow], idx: usize) -> Option<usize> {
    let row = rows.get(idx)?;
    (0..idx).rev().find(|i| row.is_child_of(&rows[*i]))
}

/// Flatten the parts of the tree the reader has opened, and only those.
fn flatten(
    entries: &[Entry],
    expanded: &BTreeSet<String>,
    depth: usize,
    acc: &mut String,
    out: &mut Vec<TreeRow>,
) {
    for e in entries {
        match e {
            Entry::Dir(d) => {
                let mark = acc.len();
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(&d.name);
                let rel = acc.clone();
                let open = expanded.contains(&rel);
                out.push(TreeRow::Dir {
                    name: d.name.clone(),
                    rel,
                    depth,
                    open,
                });
                if open {
                    flatten(&d.children, expanded, depth + 1, acc, out);
                }
                acc.truncate(mark);
            }
            Entry::File(f) => out.push(TreeRow::File {
                name: f.name.clone(),
                path: f.path.clone(),
                depth,
                doorway: f.doorway,
                empty: f.size == 0,
            }),
        }
    }
}

/// The theme sample, toned by the grammar zenith already has loaded.
fn sample_tokens(hl: &Highlighter) -> Vec<SampleLine> {
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
struct TileState {
    chosen: bool,
    focused: bool,
}

fn theme_tile(
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
            if state.chosen || state.focused { 1.5 } else { 1.0 },
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
struct Slider<'a> {
    label: &'a str,
    value: &'a mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
}

/// A slider for a page made of hairlines.
///
/// egui's own is a pill on a rail with the number in a rounded box — the one piece of
/// chrome in this program that looks like a form. This one is the house style: the label
/// and its value on one line, the value set in the code face where it can be read, and a
/// rule under them filled in the accent up to the hand.
fn line_slider(
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
fn toggles(
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
fn check(
    ui: &mut Ui,
    font: &FontId,
    pal: &Palette,
    label: &str,
    value: &mut bool,
    focused: bool,
) -> bool {
    let row_h = (font.size * 1.65).max(20.0);
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(ROW_RADIUS), theme::alpha(pal.ink, 10));
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


/// How many lines does this text have? Cheap enough to ask once when opening.
fn count_lines(text: &str) -> usize {
    text.lines().count()
}


// The two sidebars are one system, so they share a frame and a rhythm: the same inner
// margin, the same drop for the label, the same gap between that label and the list, the
// same row height and the same type size. Otherwise the eye reads them as two unrelated
// columns that happen to be adjacent — which is what "not vertically aligned" means.
const PANEL_MARGIN_X: i8 = 8;
const PANEL_MARGIN_Y: i8 = 8;
const PANEL_LABEL_DROP: f32 = 7.0;
const PANEL_LABEL_GAP: f32 = 30.0;
const PANEL_ROW_H: f32 = 22.0;
const PANEL_ROW_FONT: f32 = 12.0;
const PANEL_LABEL_FONT: f32 = 10.0;

/// How wide three window buttons are, so the dragging area can stop before them.
fn window_buttons_width() -> f32 {
    3.0 * (12.0 + 10.0) + 8.0
}

/// Does the app have to draw the window's buttons itself?
///
/// On macOS the system keeps them, floating over the transparent title bar; everywhere
/// else a window without a frame would have none at all, and no way to close.
fn own_window_buttons() -> bool {
    !cfg!(target_os = "macos")
}

/// Minimise, maximise, close — drawn by us, because on Linux and Windows a window without
/// a system frame has no buttons at all.
fn window_buttons(ui: &mut Ui, pal: &Palette, ctx: &egui::Context, bar: Rect) {
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
fn resize_handles(ui: &mut Ui) {
    use egui::viewport::ResizeDirection as Dir;
    let edge = 6.0;
    let r = ui.ctx().content_rect();
    let zones: [(Dir, Rect); 8] = [
        (Dir::North, Rect::from_min_max(r.left_top(), Pos2::new(r.right(), r.top() + edge))),
        (Dir::South, Rect::from_min_max(Pos2::new(r.left(), r.bottom() - edge), r.right_bottom())),
        (Dir::West, Rect::from_min_max(r.left_top(), Pos2::new(r.left() + edge, r.bottom()))),
        (Dir::East, Rect::from_min_max(Pos2::new(r.right() - edge, r.top()), r.right_bottom())),
        (Dir::NorthWest, Rect::from_min_max(r.left_top(), Pos2::new(r.left() + edge, r.top() + edge))),
        (Dir::NorthEast, Rect::from_min_max(Pos2::new(r.right() - edge, r.top()), Pos2::new(r.right(), r.top() + edge))),
        (Dir::SouthWest, Rect::from_min_max(Pos2::new(r.left(), r.bottom() - edge), Pos2::new(r.left() + edge, r.bottom()))),
        (Dir::SouthEast, Rect::from_min_max(Pos2::new(r.right() - edge, r.bottom() - edge), r.right_bottom())),
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

/// The four marks inside the bloom's bubbles: a tree, a list, a looking glass, a pair of
/// sliders. Drawn, not typed, so they are the same weight on every machine.
fn draw_bloom_mark(painter: &egui::Painter, c: Pos2, which: usize, ink: Color32) {
    let stroke = Stroke::new(1.2, ink);
    match which {
        0 => {
            painter.line_segment([c + vec2(0.0, 4.0), c + vec2(0.0, -1.0)], stroke);
            painter.line_segment([c + vec2(0.0, -1.0), c + vec2(-4.0, -4.0)], stroke);
            painter.line_segment([c + vec2(0.0, -1.0), c + vec2(4.0, -4.0)], stroke);
        }
        1 => {
            for (dy, half) in [(-4.0, 5.0), (0.0, 3.0), (4.0, 5.0)] {
                painter.line_segment(
                    [c + vec2(-half, dy), c + vec2(half, dy)],
                    stroke,
                );
            }
        }
        2 => {
            painter.circle_stroke(c + vec2(-1.0, -1.0), 4.0, stroke);
            painter.line_segment([c + vec2(2.0, 2.0), c + vec2(5.0, 5.0)], stroke);
        }
        _ => {
            painter.line_segment([c + vec2(-5.0, -3.0), c + vec2(5.0, -3.0)], stroke);
            painter.line_segment([c + vec2(-5.0, 3.0), c + vec2(5.0, 3.0)], stroke);
            painter.circle_filled(c + vec2(-2.0, -3.0), 1.4, ink);
            painter.circle_filled(c + vec2(2.0, 3.0), 1.4, ink);
        }
    }
}

/// A disclosure triangle, drawn rather than typed.
///
/// The file tree used to put `▸` and `▾` in a text run. Those exist in Apple's fonts and
/// in whichever symbol font a Linux distribution happens to ship — so on some machines
/// the tree had no carets at all. A path is 10 lines and cannot go missing.
fn caret(painter: &egui::Painter, centre: Pos2, open: bool, colour: Color32) {
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
fn row(
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

fn truncate(s: &str, max_chars: usize) -> String {
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
fn working_project() -> Option<PathBuf> {
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
    markers
        .iter()
        .any(|m| cwd.join(m).exists())
        .then_some(cwd)
}

fn read_head(path: &Path) -> String {
    std::fs::read(path)
        .ok()
        .map(|b| {
            let n = b.len().min(4096);
            String::from_utf8_lossy(&b[..n]).lines().next().unwrap_or("").to_string()
        })
        .unwrap_or_default()
}

fn empty_doc(syntax: &syntect::parsing::SyntaxReference) -> Document {
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
fn survives_text_entry(key: Key, cmd: bool, ctrl: bool) -> bool {
    key == Key::Escape
        || (cmd && matches!(key, Key::O | Key::K | Key::P | Key::F | Key::Comma))
        || (ctrl && matches!(key, Key::P | Key::F | Key::O | Key::I))
}

/// Should this key be considered a command at all?
///
/// With a text box focused, only the modified ones (and Escape). With a *panel* open —
/// settings, help — every command still works, so `s` closes the settings panel it
/// opened, and `?` closes the help.
fn commands_allowed(key: Key, cmd: bool, ctrl: bool, text_entry: bool) -> bool {
    !text_entry || survives_text_entry(key, cmd, ctrl)
}

/// Fill in the modifier keys for this platform.
///
/// `⌘` and `⇧` live in Apple's fonts and in almost no others, and a reader on Linux has
/// a Ctrl key anyway — so the labels are written once, here, and translated per platform.
fn keys(text: &str) -> String {
    text.replace("{mod}", typography::mod_label())
        .replace("{shift}", typography::shift_label())
}

/// The character a key stands for, for two-key sequences. Only the keys that can
/// follow `g` or `z` matter, but mapping the alphabet costs nothing and avoids a
/// surprise when another chord is added.
fn key_char(key: Key) -> Option<char> {
    use Key::*;
    Some(match key {
        A => 'a', B => 'b', C => 'c', D => 'd', E => 'e', F => 'f', G => 'g', H => 'h',
        I => 'i', J => 'j', K => 'k', L => 'l', M => 'm', N => 'n', O => 'o', P => 'p',
        Q => 'q', R => 'r', S => 's', T => 't', U => 'u', V => 'v', W => 'w', X => 'x',
        Y => 'y', Z => 'z',
        _ => return None,
    })
}

/// `ZENITH_DEBUG=1` prints what the page and the key reader decide each frame. Reading
/// is a quiet activity; being able to see the machinery is occasionally essential.
fn debug() -> bool {
    use std::sync::OnceLock;
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var("ZENITH_DEBUG").is_ok())
}

fn dropped_folder(ctx: &egui::Context) -> Option<PathBuf> {
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
fn focus_block(open: &Open, cursor: usize) -> (usize, usize) {
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
fn paragraph_block(doc: &Document, line: usize) -> (usize, usize) {
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
fn reveal_pending(line: usize, last: usize) -> Pending {
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
enum Pending {
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

/// Turn a palette into egui's own visual language: same restraint, no chrome.
fn apply_style(ctx: &egui::Context, pal: &Palette, typo: &Typography, scale: f32) {
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
        s.text_styles.insert(TextStyle::Body, typo.ui_id(13.5 * scale));
        s.text_styles.insert(TextStyle::Button, typo.ui_id(13.0 * scale));
        s.text_styles.insert(TextStyle::Small, typo.ui_id(11.5 * scale));
        s.text_styles.insert(TextStyle::Monospace, typo.code_id(code));
        s.text_styles.insert(TextStyle::Heading, typo.prose_id(code * 1.5));
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(7.0, 3.0);
        s.spacing.window_margin = Margin::same(10);
    });
}


// ---------------------------------------------------------------------------
// The launcher
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Launch {
    Contents,
    Search,
    CopyBlock,
    CloseFile,
    Bookmark,
    Reload,
    Index,
    OpenFolder,
    FocusLens,
    SoftWrap,
    LineNumbers,
    Distilled,
    Markdown,
    NextTheme,
    PrevTheme,
    Larger,
    Smaller,
    Settings,
    Keys,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LaunchGroup {
    Here,
    Folder,
    Zenith,
}

impl LaunchGroup {
    fn rank(self) -> u8 {
        match self {
            LaunchGroup::Here => 0,
            LaunchGroup::Folder => 1,
            LaunchGroup::Zenith => 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LaunchGroup::Here => "HERE",
            LaunchGroup::Folder => "FOLDER",
            LaunchGroup::Zenith => "ZENITH",
        }
    }
}

struct CommandSpec {
    action: Launch,
    label: &'static str,
    group: LaunchGroup,
    shortcut: Option<&'static str>,
    keywords: &'static str,
}

const LAUNCH_COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        action: Launch::Contents,
        label: "Contents",
        group: LaunchGroup::Here,
        shortcut: Some("o"),
        keywords: "outline symbols headings document",
    },
    CommandSpec {
        action: Launch::Search,
        label: "Search this file",
        group: LaunchGroup::Here,
        shortcut: Some("/"),
        keywords: "find in file grep search",
    },
    CommandSpec {
        action: Launch::CopyBlock,
        label: "Copy block",
        group: LaunchGroup::Here,
        shortcut: Some("y"),
        keywords: "clipboard yank copy",
    },
    CommandSpec {
        action: Launch::CloseFile,
        label: "Close file",
        group: LaunchGroup::Here,
        shortcut: Some("q"),
        keywords: "close put down file",
    },
    CommandSpec {
        action: Launch::Bookmark,
        label: "Bookmark",
        group: LaunchGroup::Here,
        shortcut: Some("M"),
        keywords: "favourite favorite star mark",
    },
    CommandSpec {
        action: Launch::Reload,
        label: "Reload file",
        group: LaunchGroup::Here,
        shortcut: Some("r"),
        keywords: "reload refresh disk reread",
    },
    CommandSpec {
        action: Launch::Index,
        label: "Index",
        group: LaunchGroup::Folder,
        shortcut: Some("tab"),
        keywords: "tree files folder index sidebar",
    },
    CommandSpec {
        action: Launch::OpenFolder,
        label: "Open folder…",
        group: LaunchGroup::Folder,
        shortcut: Some("⌘O"),
        keywords: "open project directory folder",
    },
    CommandSpec {
        action: Launch::FocusLens,
        label: "Focus lens",
        group: LaunchGroup::Zenith,
        shortcut: Some("f"),
        keywords: "dim attention focus zen lens",
    },
    CommandSpec {
        action: Launch::SoftWrap,
        label: "Soft wrap",
        group: LaunchGroup::Zenith,
        shortcut: None,
        keywords: "wrap lines soft",
    },
    CommandSpec {
        action: Launch::LineNumbers,
        label: "Line numbers",
        group: LaunchGroup::Zenith,
        shortcut: Some("#"),
        keywords: "numbers gutter lines",
    },
    CommandSpec {
        action: Launch::Distilled,
        label: "Distilled",
        group: LaunchGroup::Zenith,
        shortcut: Some("d"),
        keywords: "types punctuation simplify distill",
    },
    CommandSpec {
        action: Launch::Markdown,
        label: "Render markdown",
        group: LaunchGroup::Zenith,
        shortcut: Some("x"),
        keywords: "markdown render preview source",
    },
    CommandSpec {
        action: Launch::NextTheme,
        label: "Next theme",
        group: LaunchGroup::Zenith,
        shortcut: Some("t"),
        keywords: "theme palette colour color atmosphere next",
    },
    CommandSpec {
        action: Launch::PrevTheme,
        label: "Previous theme",
        group: LaunchGroup::Zenith,
        shortcut: Some("T"),
        keywords: "theme palette colour color atmosphere previous",
    },
    CommandSpec {
        action: Launch::Larger,
        label: "Larger type",
        group: LaunchGroup::Zenith,
        shortcut: Some("="),
        keywords: "type font size larger zoom",
    },
    CommandSpec {
        action: Launch::Smaller,
        label: "Smaller type",
        group: LaunchGroup::Zenith,
        shortcut: Some("-"),
        keywords: "type font size smaller zoom",
    },
    CommandSpec {
        action: Launch::Settings,
        label: "Settings",
        group: LaunchGroup::Zenith,
        shortcut: Some("s"),
        keywords: "preferences options settings",
    },
    CommandSpec {
        action: Launch::Keys,
        label: "Keys",
        group: LaunchGroup::Zenith,
        shortcut: Some("?"),
        keywords: "shortcuts keymap keyboard help keys",
    },
];

fn launch_command_score(spec: &CommandSpec, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let label = spec.label.to_lowercase();
    if label == q {
        return Some(300);
    }
    if label.starts_with(&q) {
        return Some(240 - (label.len() as i32 - q.len() as i32).min(40));
    }
    if label.contains(&q) {
        return Some(180);
    }
    let mut score = 0;
    for word in spec.keywords.split_whitespace() {
        if word == q {
            score = score.max(160);
        } else if word.starts_with(&q) {
            score = score.max(140);
        }
    }
    if score > 0 {
        return Some(score);
    }
    fuzzy::match_path(spec.label, &q)
        .map(|hit| (hit.score - 20).max(1))
}

#[derive(Clone)]
enum LaunchRow {
    Command {
        spec: &'static CommandSpec,
        detail: Option<String>,
        score: i32,
    },
    File {
        rel: String,
        path: PathBuf,
        marked: bool,
        positions: Vec<usize>,
        score: i32,
    },
}

impl LaunchRow {
    fn group(&self) -> LaunchGroup {
        match self {
            LaunchRow::Command { spec, .. } => spec.group,
            LaunchRow::File { .. } => LaunchGroup::Folder,
        }
    }

    fn score(&self) -> i32 {
        match self {
            LaunchRow::Command { score, .. } | LaunchRow::File { score, .. } => *score,
        }
    }

    fn is_file(&self) -> bool {
        matches!(self, LaunchRow::File { .. })
    }
}

const HELP_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "reading",
        &[
            ("j k, ctrl-j ctrl-k, ↑ ↓", "down and up a line"),
            ("w b e", "next word, previous word, end of word"),
            ("0 ^ $", "start of line, first non-blank, end of line"),
            ("{ }", "previous and next paragraph"),
            ("%", "jump to the matching bracket"),
            ("ctrl-d ctrl-u", "half a page down and up"),
            ("space ctrl-f, ⇧space ctrl-b", "a whole page"),
            ("gg / G", "top / bottom of the file"),
            ("H M L", "top, middle, bottom of the screen"),
            ("zz zt zb", "centre, or put the line at the top / bottom"),
            ("n N", "next and previous search match"),
            ("[ ]", "previous / next chapter (the contents)"),
            ("ctrl-o ctrl-i", "back and forward through everywhere you have been"),
            ("gd", "go to the definition of what is under the cursor"),
        ],
    ),
    (
        "files and the index",
        &[
            ("J K", "next / previous file in the folder"),
            ("tab", "open the index and move the keyboard into it"),
            ("j k l h", "in the index: move, open, back out"),
            ("enter, G, gg", "in the index: open, last row, first row"),
            ("⌘K", "the launcher: files, views, actions"),
            ("⌘P or ctrl-p", "find a file in the folder"),
            ("/ or ⌘F", "find inside this file"),
            ("q", "close the file, and go back to the one you came from"),
            ("⌘O", "open a folder to read"),
        ],
    ),
    (
        "views",
        &[
            ("o", "contents"),
            ("f", "focus lens: dim everything but this block"),
            ("d", "distilled: fade types and punctuation away"),
            ("y Y", "copy the block / the line"),
            ("M", "bookmark this file"),
            ("t T", "next / previous theme"),
            ("1-6", "pick a theme"),
            ("- =", "smaller / larger type"),
            ("#", "toggle line numbers"),
            ("x", "render markdown, or its source"),
            ("r", "re-read from disk"),
        ],
    ),
    (
        "settings and the window",
        &[
            ("s or ⌘,", "settings"),
            ("?", "this page"),
            ("escape", "close whatever is open"),
            ("{mod}Q", "quit zenith"),
        ],
    ),
];



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
        assert!(survives_text_entry(Key::P, true, false), "⌘P switches finder");
        assert!(survives_text_entry(Key::F, true, false), "⌘F switches finder");
        assert!(survives_text_entry(Key::Comma, true, false));
        assert!(survives_text_entry(Key::P, false, true));
        assert!(survives_text_entry(Key::Escape, false, false));
        // …but the same letters without their modifier do not.
        assert!(!survives_text_entry(Key::O, false, false));
        assert!(!survives_text_entry(Key::K, false, false));
        assert!(!survives_text_entry(Key::F, false, false));
    }
}

#[cfg(test)]
mod tree_tests {
    use super::{parent_index, walk_index, TreeRow};

    fn dir(rel: &str, depth: usize, open: bool) -> TreeRow {
        TreeRow::Dir {
            name: rel.rsplit('/').next().unwrap_or(rel).to_string(),
            rel: rel.to_string(),
            depth,
            open,
        }
    }

    fn file(name: &str, depth: usize) -> TreeRow {
        TreeRow::File {
            name: name.to_string(),
            path: std::path::PathBuf::from(name),
            depth,
            doorway: false,
            empty: false,
        }
    }

    #[test]
    fn the_page_ends_are_reachable_by_ordinary_motions() {
        use super::{reveal_pending, Pending};
        // A caret that lands on the first line asks for the top of the page, not just
        // the first line in view. The same at the other end.
        assert!(matches!(reveal_pending(0, 10), Pending::Start));
        assert!(matches!(reveal_pending(10, 10), Pending::End));
        assert!(matches!(reveal_pending(5, 10), Pending::Reveal(5)));
        // A one-line document is both ends; the top wins.
        assert!(matches!(reveal_pending(0, 0), Pending::Start));
    }

    #[test]
    fn movement_stops_at_both_ends() {
        // The reader should be able to lean on `j` without the selection escaping.
        assert_eq!(walk_index(5, None, 1), 0, "nothing selected starts at the top");
        assert_eq!(walk_index(5, Some(0), -1), 0);
        assert_eq!(walk_index(5, Some(4), 1), 4);
        assert_eq!(walk_index(5, Some(2), 1), 3);
        assert_eq!(walk_index(5, Some(2), -1), 1);
        assert_eq!(walk_index(5, Some(3), 10), 4);
        assert_eq!(walk_index(0, None, 1), 0);
    }

    #[test]
    fn going_back_finds_the_folder_you_came_from() {
        let rows = vec![
            dir("src", 0, true),
            file("src/main.rs", 1),
            dir("src/app", 1, true),
            file("src/app/ui.rs", 2),
            file("src/lib.rs", 1),
            dir("docs", 0, false),
        ];
        // A nested file belongs to its folder, not to the row above it.
        assert_eq!(parent_index(&rows, 3), Some(2));
        // A row at the top of the tree has nothing above it to go back to.
        assert_eq!(parent_index(&rows, 0), None);
    }

    #[test]
    fn a_folder_inside_an_open_folder_belongs_to_it() {
        let rows = vec![dir("src", 0, true), dir("src/app", 1, true)];
        assert_eq!(parent_index(&rows, 1), Some(0));
    }
}

#[cfg(test)]
mod launcher_tests {
    use super::{
        CommandSpec, LAUNCH_COMMANDS, Launch, launch_command_score,
    };

    fn spec(action: Launch) -> &'static CommandSpec {
        LAUNCH_COMMANDS
            .iter()
            .find(|s| s.action == action)
            .expect("a command for every action")
    }

    #[test]
    fn line_finds_line_numbers_not_contents() {
        // `Contents` has the keyword "outline"; a keyword may not be found by its
        // middle. Typing "line" means the gutter, not the table of contents.
        assert!(launch_command_score(spec(Launch::Contents), "line").is_none());
        assert!(launch_command_score(spec(Launch::LineNumbers), "line").is_some());
    }

    #[test]
    fn theme_and_focus_find_their_actions() {
        assert!(launch_command_score(spec(Launch::NextTheme), "theme").is_some());
        assert!(launch_command_score(spec(Launch::PrevTheme), "theme").is_some());
        assert!(launch_command_score(spec(Launch::FocusLens), "focus").is_some());
        assert!(launch_command_score(spec(Launch::Keys), "shortcuts").is_some());
    }

    #[test]
    fn the_keycaps_are_the_keys_the_page_uses() {
        assert_eq!(spec(Launch::Bookmark).shortcut, Some("M"));
        assert_eq!(spec(Launch::LineNumbers).shortcut, Some("#"));
        assert_eq!(spec(Launch::NextTheme).shortcut, Some("t"));
        assert_eq!(spec(Launch::PrevTheme).shortcut, Some("T"));
        assert_eq!(spec(Launch::Settings).shortcut, Some("s"));
        assert_eq!(spec(Launch::Keys).shortcut, Some("?"));
        assert_eq!(spec(Launch::SoftWrap).shortcut, None, "soft wrap has no key");
    }
}
