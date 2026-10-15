//! The window: a shelf on the left, a page in the middle, a table of contents on
//! the right, and a reading cursor instead of a caret.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::text::TextFormat;
use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Key, Margin, Pos2, Rect, RichText, ScrollArea,
    Sense, Stroke, TextStyle, Ui, ViewportCommand, vec2,
};

use crate::catalog::{Catalog, Store, display_path};
use crate::distill;
use crate::fuzzy;
use crate::outline::{self, SymKind, Symbol};
use crate::reader::{self, Laid, Paint, Style};
use crate::shelf::{self, Entry, FileEntry, Shelf};
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
struct ShelfLoad {
    rx: std::sync::mpsc::Receiver<std::io::Result<shelf::Shelf>>,
    root: PathBuf,
    focus: Option<PathBuf>,
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
    note: Option<String>,
}

pub struct Zenith {
    store: Store,
    catalog: Catalog,
    hl: Highlighter,
    typo: Typography,
    palette: Palette,
    scale: f32,
    char_w: f32,

    shelf: Option<Shelf>,
    expanded: BTreeSet<String>,
    team_open: bool,

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
    shelf_load: Option<ShelfLoad>,
    /// Every row the shelf panel is showing, in order, built once per frame so that the
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
    /// The places we have been, so `ctrl-o` can take us back.
    jumps: Vec<vim::Caret>,
    jump_pos: usize,
    outline_sel: Option<usize>,

    show_shelf: bool,
    show_outline: bool,
    zen: bool,
    show_help: bool,
    show_settings: bool,
    /// Which section of the settings panel is showing.
    settings_tab: usize,
    palette_open: bool,
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
        let t_walk = std::time::Instant::now();
        let catalog = Catalog::build(&config);
        if std::env::var("ZENITH_TIMING").is_ok() {
            eprintln!("timing: catalogue walk {:.0?}", t_walk.elapsed());
        }
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
            catalog,
            hl,
            typo,
            palette,
            shelf: None,
            expanded: BTreeSet::new(),
            team_open: false,
            open: None,
            caret: vim::Caret::default(),
            chords: vim::Chords::default(),
            hold_down: std::env::var("ZENITH_HOLD_DOWN").is_ok(),
            frame: 0,
            loading: None,
            shelf_load: None,
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
            show_shelf: true,
            show_outline: !config.zen,
            zen: config.zen,
            show_help: false,
            show_settings: false,
            settings_tab: 0,
            palette_open: false,
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
            app.tree_focus = true;
        }
        if let Ok(tab) = std::env::var("ZENITH_SETTINGS_TAB") {
            app.settings_tab = tab.parse().unwrap_or(0);
        }
        app
    }

    /// Where does the reader begin? A file on the command line wins; otherwise the
    /// last place they were reading; otherwise the project they left open.
    fn choose_startup(&mut self, target: Option<PathBuf>) {
        let target = target
            .or_else(|| self.store.session.last_root.clone())
            .or_else(|| self.store.session.last_file.clone());

        let Some(target) = target else {
            self.status = keys("press {mod}O to open a folder");
            return;
        };
        if target.is_dir() {
            self.open_shelf(&target, None);
        } else if target.is_file() {
            // The file the reader asked for, now; the shelf around it can take its time.
            let root = project_root(&target);
            self.open_file(&target);
            self.open_shelf(&root, Some(target));
        }
    }

    // -- shelves -----------------------------------------------------------

    /// Open a folder, and the file in it the reader wants.
    ///
    /// The walk does not happen here. Reading a directory tree means a stat call per file,
    /// and on a large tree that is seconds — long enough that doing it inline held the
    /// window blank until it finished. The file the reader asked for opens at once, and
    /// the shelf arrives on its own a moment later.
    fn open_shelf(&mut self, root: &Path, focus: Option<PathBuf>) {
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());
        let cfg = self.store.config.clone();

        self.store.session.last_root = Some(root.to_path_buf());
        self.shelf = None;
        self.expanded.clear();
        self.expanded.insert(String::new());
        self.store.save_session();

        self.status = format!("reading {name}…");

        let root_for_thread = root.to_path_buf();
        let name_for_thread = name.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let result = shelf::open(
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
        self.shelf_load = Some(ShelfLoad {
            rx,
            root: root.to_path_buf(),
            focus,
        });
    }

    /// The file a reader should be shown first: a README, then a manifest, then an
    /// entry point, then the first file in the shelf.
    fn first_doorway(&self) -> Option<PathBuf> {
        let shelf = self.shelf.as_ref()?;
        let find = |pred: &dyn Fn(&FileEntry) -> bool| {
            shelf.files.iter().find(|f| pred(f)).map(|f| f.path.clone())
        };
        find(&|f| f.name.to_ascii_lowercase().starts_with("readme"))
            .or_else(|| find(&|f| f.rel.matches('/').count() == 0 && f.doorway))
            .or_else(|| find(&|f| f.doorway))
            .or_else(|| shelf.files.first().map(|f| f.path.clone()))
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
        Style {
            code: self.typo.code_id(code_size),
            code_italic: self.typo.code_italic_id(code_size),
            prose: self.typo.prose_id(code_size),
            prose_italic: self.typo.prose_italic_id(code_size),
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
        self.remember_position();
        let rel = self
            .shelf
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
                    note: None,
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
                    note: None,
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
                    note: (!note.is_empty()).then_some(note),
                });
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
        self.open.as_ref().map(|o| o.doc.lines.len()).unwrap_or(0)
    }

    /// Expand every ancestor folder of `path` so the open file is visible in the tree.
    fn expand_to(&mut self, path: &Path) {
        let Some(shelf) = &self.shelf else { return };
        let Some(rel) = shelf.file(path).map(|f| f.rel.clone()) else {
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
        let Some(shelf) = &self.shelf else { return };
        let files: Vec<PathBuf> = shelf
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

    /// Move the page the smallest amount that brings the cursor into view. The
    /// request is resolved in `page()`, where the layout and viewport are known.
    fn ensure_visible(&mut self) {
        self.pending = Pending::Reveal(self.caret.line);
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
        let header_h = reader::header_height(&style);
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
        let text_entry = self.search_open || self.palette_open;
        let modal = self.show_settings || self.show_help;

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
                if (cmd && matches!(key, Key::P | Key::K)) || (ctrl && key == Key::P) {
                    self.open_palette();
                    continue;
                }
                if key == Key::F && (cmd || ctrl) {
                    self.open_search();
                    continue;
                }
                if (cmd && key == Key::Comma) || (!cmd && !ctrl && key == Key::S) {
                    self.show_settings = !self.show_settings;
                    continue;
                }
                if key == Key::Escape {
                    self.show_help = false;
                    self.show_settings = false;
                    self.palette_open = false;
                    self.search_open = false;
                    self.chords.clear();
                    self.tree_focus = false;
                    // Escape is the way back from everything, zen mode included: it is
                    // the one key a reader should never have to think about.
                    if self.zen {
                        self.zen = false;
                        self.store.config.zen = false;
                        self.store.save_config();
                    }
                    continue;
                }
                if !cmd && !ctrl && key == Key::Questionmark {
                    self.show_help = !self.show_help;
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
            TreeRow::Team => self.team_open = !self.team_open,
            TreeRow::Repo { path, .. } => self.open_shelf(&path, None),
            TreeRow::OpenFolder => self.pick_folder(),
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
                self.open_palette();
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
            vim::Chord::Definition => {
                self.status =
                    "gd: no language server configured yet".into();
            }
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
            Key::Z => Some(Act::Zen),
            Key::Y => Some(Act::CopyLine),
            Key::D => Some(Act::Distilled),
            Key::J => Some(Act::NextFile),
            Key::K => Some(Act::PrevFile),
            Key::M => Some(Act::Bookmark),
            Key::N => Some(Act::SearchPrev),
            Key::Num4 => Some(Act::LineEnd),   // ⇧4 is `$`
            Key::Num5 => Some(Act::BracketMatch), // ⇧5 is `%`
            Key::Num6 => Some(Act::LineFirst), // ⇧6 is `^`
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
            Act::Zen => {
                self.zen = !self.zen;
                self.store.config.zen = self.zen;
                self.store.save_config();
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
            Act::Outline => self.show_outline = !self.show_outline,
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
            Act::CloseFile => {
                self.remember_position();
                self.open = None;
                self.store.save_session();
            }
            _ => {}
        }
    }

    fn open_palette(&mut self) {
        // The two finders are alternatives, not layers: switching to one closes the
        // other, so `⌘P` while searching means "find a file", not "two boxes".
        self.search_open = false;
        self.palette_open = true;
        self.query.clear();
        self.palette_sel = 0;
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
        self.search_open = true;
        self.search_query.clear();
        self.search_hits.clear();
        self.search_idx = 0;
    }

    /// Apply a motion and make sure the result is on screen.
    fn motion(&mut self, motion: vim::Motion) {
        let Some(open) = &self.open else { return };
        let next = vim::resolve(&open.doc, self.caret, motion);
        if next != self.caret {
            self.push_jump(self.caret);
            self.caret = next;
        }
        self.ensure_visible();
    }

    fn jump_to(&mut self, caret: vim::Caret) {
        if self.caret != caret {
            self.push_jump(self.caret);
        }
        self.caret = caret;
        self.ensure_visible();
    }

    fn push_jump(&mut self, caret: vim::Caret) {
        if self.jumps.last() == Some(&caret) {
            return;
        }
        self.jumps.truncate(self.jump_pos);
        self.jumps.push(caret);
        if self.jumps.len() > 200 {
            self.jumps.remove(0);
        }
        self.jump_pos = self.jumps.len();
    }

    /// Back and forth through the places the reader has been.
    fn jump(&mut self, direction: i32) {
        if self.jumps.is_empty() {
            return;
        }
        let next = self.jump_pos as i32 + direction;
        if next < 0 || next as usize >= self.jumps.len() {
            return;
        }
        self.jump_pos = next as usize;
        self.caret = self.jumps[self.jump_pos];
        self.ensure_visible();
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
            self.open_shelf(&dir, None);
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
    // files and chapters
    NextFile,
    PrevFile,
    Outline,
    Zen,
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
        if let Some(load) = &self.shelf_load {
            match load.rx.try_recv() {
                Ok(Ok(shelf)) => {
                    let name = load
                        .root
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let focus = load.focus.clone();
                    let root = load.root.clone();
                    let files = shelf.files.len();
                    self.shelf_load = None;
                    self.shelf = Some(shelf);
                    self.status = format!("{name} — {files} files");
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
                        Some(path) => self.expand_to(&path),
                        None => {}
                    }
                }
                Ok(Err(e)) => {
                    let root = load.root.clone();
                    self.shelf_load = None;
                    self.status = format!("cannot read {root:?}: {e}");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.shelf_load = None;
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
            self.open_shelf(&dir, None);
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

        self.reading_keys(&ctx);

        let pal = self.palette;
        let zen = self.zen;

        if !zen {
            self.top_bar(ui);
            self.status_bar(ui);
            if self.show_shelf {
                self.shelf_panel(ui);
            }
            if self.show_outline {
                self.outline_panel(ui);
            }
        }

        egui::CentralPanel::no_frame()
            .frame(Frame::NONE.fill(pal.paper))
            .show(ui, |ui| {
                self.page(ui);
            });

        if self.palette_open {
            self.palette_overlay(&ctx);
        }
        if self.search_open {
            self.search_overlay(&ctx);
        }
        if self.show_help {
            self.help_overlay(&ctx);
        }
        if self.show_settings {
            self.settings_overlay(&ctx);
        }

        // A window without a frame has no edges of its own; these are ours. Last, so they
        // sit on top of the content they border.
        if self.store.config.frameless && !cfg!(target_os = "macos") {
            resize_handles(ui);
        }
        if zen {
            self.zen_hint(&ctx);
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
        let font = self.typo.ui_id(12.5 * self.scale);
        let strong = self.typo.prose_id(15.0 * self.scale);
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
        egui::Panel::top("zenith-top")
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
                let mark = painter.text(
                    Pos2::new(bar_rect.left() + 1.0, y),
                    Align2::LEFT_CENTER,
                    "ZENITH",
                    self.typo.prose_id(13.0 * self.scale),
                    pal.ink_faint,
                );
                let mut x = mark.right() + 18.0;
                for (i, c) in crumbs.iter().enumerate() {
                    let last = i + 1 == crumbs.len();
                    let f = if last { strong.clone() } else { font.clone() };
                    let color = if last { pal.ink } else { pal.ink_soft };
                    let r = painter.text(Pos2::new(x, y), Align2::LEFT_CENTER, c, f, color);
                    x = r.right() + 4.0;
                    if !last {
                        let s = painter.text(
                            Pos2::new(x, y),
                            Align2::LEFT_CENTER,
                            self.typo.marks.pick("›"),
                            font.clone(),
                            pal.ink_faint,
                        );
                        x = s.right() + 4.0;
                    }
                }
            });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let font = self.typo.ui_id(11.5 * self.scale);
        let left = match &self.open {
            Some(o) => {
                let lines = o.doc.line_count();
                let line = self.caret.line + 1;
                let col = self.caret.col + 1;
                let lang = o.doc.syntax.clone();
                let mut s = format!("{lang} · line {line}:{col} of {lines}");
                if let Some(note) = &o.note {
                    s.push_str(" · ");
                    s.push_str(note);
                }
                s
            }
            None => self.status.clone(),
        };
        let hint = if self.tree_focus {
            keys("tree: j k move · l open · h back · G end · / find · esc to the page").to_string()
        } else {
            keys("{mod}P find file · / search in file · Z zen · f focus · tab tree · ? keys")
        };
        egui::Panel::bottom("zenith-status")
            .resizable(false)
            .exact_size(26.0)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(14, 4)),
            )
            .show(ui, |ui| {
                let y = ui.min_rect().center().y;
                ui.painter().text(
                    Pos2::new(ui.min_rect().left(), y),
                    Align2::LEFT_CENTER,
                    left,
                    font.clone(),
                    pal.ink_soft,
                );
                ui.painter().text(
                    Pos2::new(ui.min_rect().center().x + 40.0, y),
                    Align2::CENTER_CENTER,
                    hint,
                    font.clone(),
                    pal.ink_faint,
                );
            });
    }

    /// The shelf panel's rows, in the order they appear on screen.
    ///
    /// Built before the keyboard is read each frame, because both the navigation and the
    /// drawing have to mean the same thing by "the row below this one".
    fn build_tree_rows(&self) -> Vec<TreeRow> {
        let mut out: Vec<TreeRow> = vec![TreeRow::Team];
        if self.team_open {
            for r in &self.catalog.team {
                out.push(TreeRow::Repo {
                    name: r.name.clone(),
                    kind: r.kind.clone(),
                    path: r.path.clone(),
                    named: false,
                });
            }
        }
        for s in &self.catalog.shelves {
            out.push(TreeRow::Repo {
                name: s.name.clone(),
                kind: "shelf".to_string(),
                path: s.path.clone(),
                named: true,
            });
        }
        out.push(TreeRow::OpenFolder);
        if let Some(shelf) = &self.shelf {
            let mut acc = String::new();
            flatten(&shelf.tree, &self.expanded, 0, &mut acc, &mut out);
        }
        out
    }

    fn shelf_panel(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let font = self.typo.ui_id(PANEL_ROW_FONT * self.scale);
        let label_font = self.typo.ui_id(PANEL_LABEL_FONT * self.scale);

        // Collect what we need before mutating. The rows come from the one list the
        // keyboard walks, so a row and its key action can never disagree about which row
        // is which.
        let rows: Vec<TreeRow> = self.tree_rows.clone();
        let top_items: Vec<TreeRow> = rows
            .iter()
            .take_while(|r| !matches!(r, TreeRow::Dir { .. } | TreeRow::File { .. }))
            .cloned()
            .collect();
        let shelf_files: Vec<TreeRow> = rows
            .iter()
            .filter(|r| matches!(r, TreeRow::Dir { .. } | TreeRow::File { .. }))
            .cloned()
            .collect();
        let workspace = display_path(&self.catalog.workspace);
        let shelf_name = self.shelf.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let current = self.open.as_ref().map(|o| o.path.clone());
        let team_open = self.team_open;

        let mut clicked_file: Option<PathBuf> = None;
        let mut clicked_repo: Option<PathBuf> = None;
        let mut toggled_dir: Option<String> = None;
        let mut toggle_team = false;
        let mut open_folder = false;

        egui::Panel::left("zenith-shelf")
            .resizable(true)
            .default_size(266.0)
            .min_size(180.0)
            .max_size(460.0)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(PANEL_MARGIN_X, PANEL_MARGIN_Y)),
            )
            .show(ui, |ui| {
                // -- every row above the shelf, drawn from the same list the keyboard
                // walks, so the two can never disagree about what is on screen.
                for item in &top_items {
                    let key = item.key();
                    let picked = self.tree_focus && self.tree_sel.as_ref() == Some(&key);
                    let on_shelf = match item {
                        TreeRow::Repo { path, .. } => {
                            self.shelf.as_ref().is_some_and(|sh| sh.root == *path)
                        }
                        _ => false,
                    };
                    let (text, glyph, right, ink) = match item {
                        TreeRow::Team => (
                            "TEAM".to_string(),
                            "",
                            Some(workspace.clone()),
                            pal.ink_faint,
                        ),
                        TreeRow::Repo { name, kind, named, .. } => (
                            name.clone(),
                            if *named {
                                self.typo.marks.pick("§")
                            } else {
                                self.typo.marks.pick("❖")
                            },
                            Some(kind.clone()),
                            if picked || on_shelf { pal.ink } else { pal.ink_soft },
                        ),
                        TreeRow::OpenFolder => (
                            "Open a folder to read…".to_string(),
                            typography::mod_label(),
                            Some(keys("{mod}O")),
                            pal.accent,
                        ),
                        _ => continue,
                    };
                    let is_team = matches!(item, TreeRow::Team);
                    let resp = row(
                        ui,
                        &pal,
                        &text,
                        glyph,
                        item.depth(),
                        picked,
                        ink,
                        if is_team { &label_font } else { &font },
                        right.as_deref(),
                        if is_team { Some(team_open) } else { None },
                    );
                    if self.tree_moved && picked {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                    if resp.clicked() {
                        // A click means what the key would mean: one selection, moved by
                        // a different hand.
                        self.tree_focus = true;
                        self.tree_sel = Some(key);
                        match item {
                            TreeRow::Team => toggle_team = true,
                            TreeRow::Repo { path, .. } => clicked_repo = Some(path.clone()),
                            TreeRow::OpenFolder => open_folder = true,
                            _ => {}
                        }
                    }
                }
                ui.add_space(6.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(6.0);

                // -- the shelf
                ui.painter().text(
                    Pos2::new(ui.cursor().left() + 1.0, ui.cursor().top() + PANEL_LABEL_DROP),
                    Align2::LEFT_CENTER,
                    format!("SHELF · {shelf_name}"),
                    label_font.clone(),
                    pal.ink_faint,
                );
                if let Some(s) = &self.shelf {
                    // No file count, no folder count, no total size: none of it is what
                    // a reader came for. Only a fault gets a line of its own.
                    let mut warn = String::new();
                    if s.truncated {
                        warn.push_str("stopped at 40,000 files");
                    }
                    if s.unreadable > 0 {
                        if !warn.is_empty() {
                            warn.push_str(" · ");
                        }
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
                             (settings › shelf)",
                            s.filtered,
                            if s.filtered == 1 { "" } else { "s" }
                        );
                        ui.interact(
                            Rect::from_min_size(
                                Pos2::new(ui.cursor().left(), ui.cursor().top()),
                                vec2(ui.available_width(), 16.0),
                            ),
                            ui.id().with("shelf-filter"),
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
                        if shelf_files.is_empty() {
                            ui.painter().text(
                                Pos2::new(ui.min_rect().left() + 6.0, ui.min_rect().top() + 8.0),
                                Align2::LEFT_TOP,
                                "no folder open",
                                font.clone(),
                                pal.ink_faint,
                            );
                            return;
                        }
                        for item in &shelf_files {
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
                                _ => continue,
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
                                    _ => {}
                                }
                            }
                        }
                    });
            });

        if toggle_team {
            self.team_open = !self.team_open;
        }
        if let Some(rel) = toggled_dir {
            if !self.expanded.remove(&rel) {
                self.expanded.insert(rel);
            }
        }
        if open_folder {
            self.pick_folder();
        }
        if let Some(path) = clicked_repo {
            self.open_shelf(&path, None);
        }
        if let Some(path) = clicked_file {
            self.open_file(&path);
        }
    }



    fn outline_panel(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let font = self.typo.ui_id(PANEL_ROW_FONT * self.scale);
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

        egui::Panel::right("zenith-outline")
            .resizable(true)
            .default_size(280.0)
            .min_size(180.0)
            .max_size(460.0)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(PANEL_MARGIN_X, PANEL_MARGIN_Y)),
            )
            .show(ui, |ui| {
                ui.painter().text(
                    Pos2::new(ui.cursor().left() + 1.0, ui.cursor().top() + PANEL_LABEL_DROP),
                    Align2::LEFT_CENTER,
                    "CONTENTS",
                    label_font.clone(),
                    pal.ink_faint,
                );
                ui.add_space(PANEL_LABEL_GAP);
                if !some_open {
                    return;
                }
                ScrollArea::vertical()
                    .id_salt("outline")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if items.is_empty() {
                            ui.painter().text(
                                Pos2::new(ui.min_rect().left() + 4.0, ui.min_rect().top() + 6.0),
                                Align2::LEFT_TOP,
                                "no outline for this file",
                                font.clone(),
                                pal.ink_faint,
                            );
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
            });

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

        // Typeset for this page width before borrowing the document.
        let page_rect = Rect::from_min_size(ui.min_rect().min, ui.available_size());
        self.ensure_layout(&ctx, page_rect);
        let style = self.style();

        let Some(open) = &self.open else {
            // The reading room, empty.
            let rect = ui.max_rect();
            let font = self.typo.prose_id(22.0 * self.scale);
            let small = self.typo.ui_id(13.0 * self.scale);
            ui.painter().text(
                rect.center() - vec2(0.0, 26.0),
                Align2::CENTER_CENTER,
                "Zenith",
                font,
                pal.ink_soft,
            );
            ui.painter().text(
                rect.center() + vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "a reading room for source code",
                small.clone(),
                pal.ink_faint,
            );
            ui.painter().text(
                rect.center() + vec2(0.0, 40.0),
                Align2::CENTER_CENTER,
                keys("{mod}O  open a folder      /  find a file"),
                small,
                pal.ink_faint,
            );
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
                pal.ink_faint,
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
        let zen = self.zen;

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
                origin_y,
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
                zen,
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
        }

        // The spine stays with the page, not with the text: it is a bookmark, not a
        // scrollbar, so it lives at the edge of the paper. It is also draggable, which is
        // what a scrollbar was for — so there is one indicator here rather than two.
        let spine_x = page_rect.right() - 16.0;
        let spine_top = page_rect.top() + 10.0;
        let spine_h = page_rect.height() - 20.0;
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

    /// The inverse of [`Self::read_fraction`]: a place on the spine is a place in the file.
    fn frac_to_scroll(&self, frac: f32) -> f32 {
        let Some(open) = &self.open else { return 0.0 };
        let Some(laid) = &open.laid else { return 0.0 };
        let span = (laid.total - laid.row_h * 4.0).max(1.0);
        (frac.clamp(0.0, 1.0) * span).clamp(0.0, (laid.total - self.view_h).max(0.0))
    }

    fn read_fraction(&self) -> f32 {
        let Some(open) = &self.open else { return 0.0 };
        let Some(laid) = &open.laid else { return 0.0 };
        let span = (laid.total - laid.row_h * 4.0).max(1.0);
        (self.scroll / span).clamp(0.0, 1.0)
    }

    // -- overlays ----------------------------------------------------------

    fn palette_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let screen = ctx.content_rect();
        let width = (screen.width() * 0.46).clamp(360.0, 720.0);
        let pos = Pos2::new(screen.center().x - width * 0.5, screen.top() + 96.0);

        // Matching files.
        let matches: Vec<(i32, usize, Vec<usize>)> = match &self.shelf {
            Some(s) => {
                let mut v: Vec<(i32, usize, Vec<usize>)> = s
                    .files
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| {
                        fuzzy::match_path(&f.rel, &self.query).map(|h| (h.score, i, h.positions))
                    })
                    .collect();
                v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                v.truncate(11);
                v
            }
            None => Vec::new(),
        };
        let file_rows: Vec<(String, bool)> = match &self.shelf {
            Some(s) => matches
                .iter()
                .map(|(_, i, _)| {
                    let f = &s.files[*i];
                    (f.rel.clone(), self.store.is_favorite(&f.path))
                })
                .collect(),
            None => Vec::new(),
        };
        let positions: Vec<Vec<usize>> = matches.iter().map(|(_, _, p)| p.clone()).collect();

        let mut chosen: Option<usize> = None;
        let mut close = false;

        let font = self.typo.ui_id(13.0 * self.scale);
        let input_font = self.typo.ui_id(15.0 * self.scale);

        egui::Window::new("find")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .fixed_pos(pos)
            .fixed_size(vec2(width, 0.0))
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.edge))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(12, 10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(60),
                    }),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().extreme_bg_color = pal.panel_deep;
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("find a file in this shelf")
                        .font(input_font.clone())
                        .desired_width(width - 24.0)
                        .frame(Frame::NONE),
                );
                if !resp.has_focus() {
                    resp.request_focus();
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
                if kd {
                    self.palette_sel = (self.palette_sel + 1) % file_rows.len().max(1);
                }
                if ku {
                    self.palette_sel = self
                        .palette_sel
                        .checked_sub(1)
                        .unwrap_or_else(|| file_rows.len().saturating_sub(1));
                }
                if kenter {
                    chosen = Some(self.palette_sel);
                }

                for (i, (rel, marked)) in file_rows.iter().enumerate() {
                    let selected = i == self.palette_sel;
                    let (rect, resp) = ui
                        .allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                    if selected {
                        ui.painter().rect_filled(
                            resp.rect,
                            CornerRadius::same(4),
                            theme::alpha(pal.accent, 26),
                        );
                    }
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap.max_width = ui.available_width() - 18.0;
                    let dims = positions.get(i).cloned().unwrap_or_default();
                    for (ci, ch) in rel.chars().enumerate() {
                        let hit = dims.binary_search(&ci).is_ok();
                        let mut f = TextFormat {
                            font_id: font.clone(),
                            color: if hit {
                                if selected { pal.ink } else { pal.ink_soft }
                            } else if selected {
                                pal.ink_soft
                            } else {
                                pal.ink_faint
                            },
                            ..Default::default()
                        };
                        if hit {
                            f.underline = Stroke::new(1.0, pal.accent_soft);
                        }
                        job.append(&ch.to_string(), 0.0, f);
                    }
                    let g = ui.painter().layout_job(job);
                    ui.painter()
                        .galley(Pos2::new(rect.left() + 2.0, rect.center().y - g.size().y * 0.5), g, pal.ink);
                    if *marked {
                        ui.painter().text(
                            Pos2::new(rect.right() - 2.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            self.typo.marks.pick("✦"),
                            font.clone(),
                            pal.accent,
                        );
                    }
                    if resp.clicked() {
                        chosen = Some(i);
                    }
                }
                if file_rows.is_empty() {
                    ui.painter().text(
                        Pos2::new(ui.min_rect().left() + 2.0, ui.cursor().top() + 6.0),
                        Align2::LEFT_TOP,
                        "nothing matches",
                        font.clone(),
                        pal.ink_faint,
                    );
                    ui.add_space(18.0);
                }
            });

        if let Some(i) = chosen {
            if let Some((_, idx, _)) = matches.get(i) {
                if let Some(s) = &self.shelf {
                    let path = s.files[*idx].path.clone();
                    self.open_file(&path);
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
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(10, 6)),
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
                    ui.label(RichText::new(count).font(font.clone()).color(pal.ink_faint));
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

    fn help_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let font = self.typo.ui_id(12.5 * self.scale);
        let key_font = self.typo.code_id(12.0 * self.scale);
        let mut open = self.show_help;
        let screen = ctx.content_rect();
        egui::Window::new("keys")
            .open(&mut open)
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .default_pos(Pos2::new(screen.center().x - 240.0, screen.center().y - 200.0))
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.edge))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(18, 16)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Reading keys")
                        .font(self.typo.prose_id(17.0 * self.scale))
                        .color(pal.ink),
                );
                ui.add_space(8.0);
                for (k, v) in HELP_ROWS {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(*k).font(key_font.clone()).color(pal.accent));
                        ui.add_space(90.0);
                        ui.label(RichText::new(*v).font(font.clone()).color(pal.ink_soft));
                    });
                }

            });
        self.show_help = open;
    }

    /// Settings: four short sections, one at a time.
    ///
    /// Sections, two-column rows and paired lists keep the whole panel within a third of
    /// the screen, so nothing has to be scrolled for or hidden.
    fn settings_overlay(&mut self, ctx: &egui::Context) {
        let pal = self.palette;
        let font = self.typo.ui_id(12.5 * self.scale);
        let head = self.typo.prose_id(15.0 * self.scale);
        let small = self.typo.ui_id(11.5 * self.scale);
        let screen = ctx.content_rect();
        let width = 380.0;
        let mut open = self.show_settings;
        let mut changed = false;
        let mut new_mono = self.typo.mono_idx;
        let mut new_prose = self.typo.prose_idx;
        let mut theme_choice: Option<usize> = None;
        let mut open_folder = false;

        egui::Window::new("settings")
            .open(&mut open)
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .fixed_pos(Pos2::new(screen.center().x - width * 0.5, screen.top() + 56.0))
            .fixed_size(vec2(width, 0.0))
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0, pal.edge))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 12)),
            )
            .show(ctx, |ui| {
                // ---- section tabs ----
                ui.horizontal(|ui| {
                    for (i, name) in ["Page", "Type", "Theme", "Shelf"].iter().enumerate() {
                        let selected = self.settings_tab == i;
                        let text = RichText::new(*name)
                            .font(head.clone())
                            .color(if selected { pal.ink } else { pal.ink_faint });
                        if ui.selectable_label(selected, text).clicked() {
                            self.settings_tab = i;
                        }
                    }
                });
                ui.add_space(2.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(8.0);

                // A fixed minimum height keeps the panel from jumping as the reader
                // moves between sections. Nothing here needs scrolling: each section is
                // at most four rows.
                ui.set_min_height(232.0);
                {
                    match self.settings_tab {
                        // ------------------------------------------------ page
                        0 => {
                            let cfg = &mut self.store.config;
                            ui.horizontal(|ui| {
                                let half = width * 0.5 - 18.0;
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    changed |= toggles(ui, &font, &pal, &mut [
                                        ("soft wrap", &mut cfg.wrap),
                                        ("indentation whispers", &mut cfg.guides),
                                        ("comments as prose", &mut cfg.serif_comments),
                                        ("focus lens", &mut cfg.focus_lens),
                                    ]);
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    changed |= toggles(ui, &font, &pal, &mut [
                                        ("compress blank lines", &mut cfg.compress_blank),
                                        ("line numbers", &mut cfg.line_numbers),
                                        ("distilled types", &mut cfg.distilled),
                                        ("themed window bar", &mut cfg.frameless),
                                        ("render markdown", &mut cfg.markdown),
                                    ]);
                                });
                            });
                            ui.add_space(6.0);
                            ui.add(
                                egui::Slider::new(&mut cfg.measure, 40..=160)
                                    .text(RichText::new("measure (characters)").font(font.clone())),
                            );
                            ui.add(
                                egui::Slider::new(&mut cfg.leading, 1.2..=2.2)
                                    .text(RichText::new("leading").font(font.clone())),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.scale, 0.7..=2.0)
                                    .text(RichText::new("type size").font(font.clone())),
                            );
                        }
                        // ------------------------------------------------ type
                        1 => {
                            let code = self
                                .typo
                                .mono_voices
                                .get(self.typo.mono_idx)
                                .map(|v| v.label.clone())
                                .unwrap_or_else(|| self.typo.mono_label.clone());
                            let prose = self
                                .typo
                                .prose_voices
                                .get(self.typo.prose_idx)
                                .map(|v| v.label.clone())
                                .unwrap_or_else(|| self.typo.prose_label.clone());
                            let column = width * 0.5 - 20.0;
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(column);
                                    ui.label(
                                        RichText::new("code")
                                            .font(small.clone())
                                            .color(pal.ink_faint),
                                    );
                                    for (i, v) in self.typo.mono_voices.iter().enumerate() {
                                        let selected = i == self.typo.mono_idx;
                                        let ok = typography::loads(v);
                                        let text = RichText::new(v.label.clone())
                                            .font(font.clone())
                                            .color(if ok { pal.ink_soft } else { pal.ink_faint });
                                        let r = ui.selectable_label(selected, text);
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
                                            .color(pal.ink_faint),
                                    );
                                    for (i, v) in self.typo.prose_voices.iter().enumerate() {
                                        let selected = i == self.typo.prose_idx;
                                        let text = RichText::new(v.label.clone())
                                            .font(font.clone())
                                            .color(pal.ink_soft);
                                        let r = ui.selectable_label(selected, text);
                                        if r.clicked() {
                                            new_prose = i;
                                        }
                                        r.on_hover_text(
                                            RichText::new(v.note.clone()).font(small.clone()),
                                        );
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            let _ = (code, prose);
                            ui.add_space(6.0);
                            let chrome = self
                                .typo
                                .ui_voices
                                .first()
                                .map(|v| v.label.clone())
                                .unwrap_or_else(|| self.typo.ui_label.clone());
                            ui.label(
                                RichText::new(format!("interface: {chrome}"))
                                    .font(small.clone())
                                    .color(pal.ink_faint),
                            );
                            if self.typo.bundled_fallback {
                                ui.label(
                                    RichText::new(
                                        "no system fonts found: egui's bundled faces are in use",
                                    )
                                    .font(small.clone())
                                    .color(pal.ink_faint),
                                );
                            }
                        }
                        // ------------------------------------------------ theme
                        2 => {
                            // Two columns: six full-width rows was half the panel.
                            for pair in theme::THEMES.chunks(2) {
                                ui.horizontal(|ui| {
                                    for p in pair {
                                        let selected = p.name == self.palette.name;
                                        let text = RichText::new(p.name)
                                            .font(font.clone())
                                            .color(p.tone(Tone::Keyword));
                                        let r = ui.selectable_label(selected, text);
                                        if r.clicked() {
                                            theme_choice = Some(
                                                theme::THEMES
                                                    .iter()
                                                    .position(|t| t.name == p.name)
                                                    .unwrap_or(0),
                                            );
                                        }
                                        r.on_hover_text(
                                            RichText::new(p.mood).font(small.clone()),
                                        );
                                        ui.add_space(4.0);
                                    }
                                });
                            }
                        }
                        // ------------------------------------------------ shelf
                        _ => {
                            let cfg = &mut self.store.config;
                            changed |= toggles(
                                ui,
                                &font,
                                &pal,
                                &mut [
                                    ("source files only", &mut cfg.code_only),
                                    ("include hidden files", &mut cfg.show_hidden),
                                ],
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(display_path(&self.catalog.workspace))
                                    .font(small.clone())
                                    .color(pal.ink_faint),
                            );
                            if ui
                                .button(RichText::new("Open a folder…").font(font.clone()))
                                .clicked()
                            {
                                open_folder = true;
                            }
                        }
                    }
                }
            });

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
        if open_folder {
            self.pick_folder();
        }
        self.show_settings = open;
    }

    /// In zen mode, a whisper of chrome that fades when you move the mouse.
    fn zen_hint(&mut self, ctx: &egui::Context) {
        let idle = ctx.input(|i| i.pointer.has_pointer());
        if idle {
            return;
        }
        let pal = self.palette;
        let rect = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("zen-hint"),
        ))
        .text(
            Pos2::new(rect.center().x, rect.bottom() - 22.0),
            Align2::CENTER_BOTTOM,
            "y — return to the reading room",
            self.typo.ui_id(12.0 * self.scale),
            theme::alpha(pal.ink_faint, 150),
        );
    }

    // -- smoke -------------------------------------------------------------

    fn smoke_tick(&mut self, ctx: &egui::Context) {
        if self.smoke.is_none() {
            return;
        }
        // A self-portrait has to be reproducible, so each phase states its own
        // conditions explicitly rather than inheriting whatever was last saved.
        if self.smoke.as_ref().map(|s| s.frame) == Some(0) {
            match self.smoke.as_ref().map(|s| s.phase) {
                Some(0) => {
                    self.zen = false;
                    self.store.config.focus_lens = false;
                    self.store.config.distilled = false;
                    self.caret = vim::Caret::default();
                    self.pending = Pending::Restore;
                }
                _ => {
                    self.zen = true;
                    self.store.config.focus_lens = true;
                    let nested = self
                        .shelf
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
                // Phase two: zen mode with the focus lens, reading zenith's own layout
                // engine — a file with real nesting, so the lens has work to do.
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

/// One visible line of the shelf tree.
#[derive(Clone)]
enum TreeRow {
    Team,
    Repo {
        name: String,
        kind: String,
        path: PathBuf,
        /// A folder named by hand in the config, rather than found in the workspace.
        named: bool,
    },
    OpenFolder,
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
    Team,
    Repo(PathBuf),
    OpenFolder,
    Dir(String),
    File(PathBuf),
}

impl TreeRow {
    fn key(&self) -> TreeKey {
        match self {
            TreeRow::Team => TreeKey::Team,
            TreeRow::Repo { path, .. } => TreeKey::Repo(path.clone()),
            TreeRow::OpenFolder => TreeKey::OpenFolder,
            TreeRow::Dir { rel, .. } => TreeKey::Dir(rel.clone()),
            TreeRow::File { path, .. } => TreeKey::File(path.clone()),
        }
    }

    /// How far to indent, in steps.
    fn depth(&self) -> usize {
        match self {
            TreeRow::Team | TreeRow::OpenFolder => 0,
            TreeRow::Repo { .. } => 1,
            TreeRow::Dir { depth, .. } | TreeRow::File { depth, .. } => *depth,
        }
    }

    /// For `h`: which row is this row's parent, if it has one on screen?
    fn is_child_of(&self, other: &TreeRow) -> bool {
        match (self, other) {
            (TreeRow::Repo { .. }, TreeRow::Team) => true,
            (TreeRow::Dir { depth, .. } | TreeRow::File { depth, .. }, parent) => match parent {
                TreeRow::Dir {
                    depth: pd, open, ..
                } => *open && *pd + 1 == *depth,
                TreeRow::Repo { .. } => *depth == 0,
                _ => false,
            },
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
/// to the team. Walking up means finding the nearest row above that this one is inside.
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

/// A column of rendered checkboxes, drawn as one grid so the settings panel stays short.
fn toggles(
    ui: &mut Ui,
    font: &FontId,
    pal: &Palette,
    rows: &mut [(&str, &mut bool)],
) -> bool {
    let mut changed = false;
    for (label, value) in rows.iter_mut() {
        changed |= ui
            .checkbox(value, RichText::new(*label).font(font.clone()))
            .changed();
    }
    let _ = pal;
    changed
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
const PANEL_ROW_H: f32 = 21.0;
const PANEL_ROW_FONT: f32 = 12.5;
const PANEL_LABEL_FONT: f32 = 10.5;

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
        .map(|r| r.chars().count() as f32 * font.size * 0.58 + 10.0)
        .unwrap_or(6.0);
    let avail = (rect.right() - right_w - x).max(10.0);
    let max_chars = (avail / (font.size * 0.52)) as usize;
    let label = truncate(text, max_chars);
    painter.text(Pos2::new(x, y), Align2::LEFT_CENTER, label, font.clone(), ink);
    if let Some(r) = right {
        painter.text(
            Pos2::new(rect.right() - 4.0, y),
            Align2::RIGHT_CENTER,
            r,
            font.clone(),
            pal.ink_faint,
        );
    }
    resp
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
fn project_root(file: &Path) -> PathBuf {
    let parent = file.parent().map(|p| p.to_path_buf());
    let markers = [
        ".git",
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "pom.xml",
        "Gemfile",
    ];
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dir = parent.clone();
    let mut hops = 0;
    while let Some(d) = dir {
        if hops > 8 {
            break;
        }
        if markers.iter().any(|m| d.join(m).exists()) {
            return d;
        }
        // Stop at the home directory or the root of the filesystem. A "project" is
        // something a reader would recognise; climbing past home is how opening a lone
        // file in /tmp used to hand the walker the whole disk.
        if home.as_deref() == Some(d.as_path()) || d.parent().is_none() {
            break;
        }
        dir = d.parent().map(|p| p.to_path_buf());
        hops += 1;
    }
    // No marker anywhere above it: the file's own folder is the shelf.
    parent.unwrap_or_else(|| PathBuf::from("."))
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
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.selection.bg_fill = theme::alpha(pal.accent, 60);
    visuals.selection.stroke = Stroke::new(1.0, pal.accent);
    visuals.hyperlink_color = pal.accent;
    visuals.override_text_color = None;
    visuals.widgets.noninteractive.bg_fill = pal.panel;
    visuals.widgets.noninteractive.weak_bg_fill = pal.panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.edge);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.ink_soft);
    visuals.widgets.inactive.bg_fill = pal.panel_deep;
    visuals.widgets.inactive.weak_bg_fill = theme::alpha(pal.ink, 10);
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
        w.corner_radius = CornerRadius::same(5);
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

const HELP_ROWS: &[(&str, &str)] = &[
    ("j k, ↑ ↓", "down and up a line"),
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
    ("[ ]", "previous / next chapter (the outline)"),
    ("ctrl-o ctrl-i", "back and forward through where you have been"),
    ("J K", "next / previous file in the shelf"),
    ("j k l h", "in the tree: move, open, and back out"),
    ("enter, G, gg", "in the tree: open, last row, first row"),
    ("⌘P or ⌘K or ctrl-p", "find a file in the shelf"),
    ("/ or ⌘F", "search inside this file"),
    ("tab", "move the keyboard into the tree"),
    ("o", "contents panel"),
    ("Z", "zen mode — no chrome at all"),
    ("f", "focus lens — dim everything but this block"),
    ("d", "distilled: fade types and punctuation away"),
    ("y Y", "copy the block / the line"),
    ("m", "bookmark this file"),
    ("s or ⌘,", "settings"),
    ("escape", "close anything, and leave zen mode"),
    ("1-6", "pick a theme"),
    ("- =", "smaller / larger type"),
    ("#", "toggle line numbers"),
    ("x", "render markdown, or its source"),
    ("r", "re-read from disk"),
    ("q", "close the file"),
    ("{mod}Q", "quit zenith"),
    ("⌘O", "open a folder to read"),
    ("gd", "go to definition (no language server configured yet)"),
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
            TreeRow::Team,
            TreeRow::OpenFolder,
            dir("src", 0, true),
            file("src/main.rs", 1),
            dir("src/app", 1, true),
            file("src/app/ui.rs", 2),
            file("src/lib.rs", 1),
            dir("docs", 0, false),
        ];
        // A nested file belongs to its folder, not to the row above it.
        assert_eq!(parent_index(&rows, 5), Some(4));
        // A file at the top of the shelf belongs to the shelf itself, which is depth 0.
        assert_eq!(parent_index(&rows, 3), Some(2));
        // A closed folder's children are not on screen, so there is nothing to go back to.
        assert_eq!(parent_index(&rows, 3).map(|i| i != 7), Some(true));
        // The first row has no parent.
        assert_eq!(parent_index(&rows, 0), None);
    }

    #[test]
    fn a_repository_belongs_to_the_team() {
        let rows = vec![
            TreeRow::Team,
            TreeRow::Repo {
                name: "zenith".into(),
                kind: "rust".into(),
                path: std::path::PathBuf::from("/tmp/zenith"),
                named: false,
            },
        ];
        assert_eq!(parent_index(&rows, 1), Some(0));
    }
}
