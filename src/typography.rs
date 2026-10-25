//! Typography — the part of a reader that most editors ignore completely.
//!
//! A code editor is set in one monospace face, everywhere, forever. Zenith sets code
//! in a monospace *voice*, comments in a serif *prose* voice, and all chrome in a
//! humanist sans. Three voices, each doing the job it was designed for.
//!
//! Nothing is bundled and nothing is hard-coded to a platform: zenith scans the font
//! directories of whatever machine it is running on (macOS, Linux, Windows), matches
//! the files it finds against a preference list of families, and then asks each
//! candidate to prove itself before using it. A face has to parse under `skrifa` — the
//! same parser egui rasterises with — cover every character source code depends on, and
//! draw a real underscore rather than a hairline. See `underscore_size` for why that
//! last test exists.

use std::path::{Path, PathBuf};

use egui::{FontData, FontDefinitions, FontFamily, FontId};
use skrifa::attribute::Style;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

use crate::catalog::home_dir;

/// Which of the three voices a face is a candidate for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Code,
    Prose,
    Ui,
}

impl Role {
    pub fn title(self) -> &'static str {
        match self {
            Role::Code => "CODE (monospace)",
            Role::Prose => "PROSE (serif, for comments and titles)",
            Role::Ui => "CHROME (sans, for the interface)",
        }
    }
}

/// A family zenith knows how to look for, by the stem of its font files.
struct Family {
    label: &'static str,
    /// Normalised file-stem fragments. The longest match wins, so `dejavusansmono`
    /// is picked as a monospace face rather than as a sans.
    stems: &'static [&'static str],
    note: &'static str,
}

const fn fam(label: &'static str, stems: &'static [&'static str], note: &'static str) -> Family {
    Family { label, stems, note }
}

/// Preference order matters: it is the order a reader meets these faces.
static FAMILIES: &[(Role, Family)] = &[
    // ---- code ----
    (Role::Code, fam("SF Mono", &["sfnsmono", "sfmono"], "Apple's coding face: quiet, unambiguous")),
    (Role::Code, fam("JetBrains Mono", &["jetbrainsmono"], "tall x-height, drawn for code")),
    (Role::Code, fam("Fira Code", &["firacode"], "ligatures, if you want them")),
    (Role::Code, fam("Fira Mono", &["firamono"], "the ligature-free sibling of Fira Code")),
    (Role::Code, fam("Hack", &["hacknerd", "hack"], "drawn for source code; wide coverage")),
    (Role::Code, fam("Cascadia Code", &["cascadiacode", "cascadiamono"], "Microsoft's terminal face")),
    (Role::Code, fam("IBM Plex Mono", &["ibmplexmono"], "engineered, slightly quirky")),
    (Role::Code, fam("Source Code Pro", &["sourcecodepro"], "Adobe's reading mono")),
    (Role::Code, fam("Iosevka", &["iosevka"], "narrow; fits long lines")),
    (Role::Code, fam("Victor Mono", &["victormono"], "semi-connected italics")),
    (Role::Code, fam("Inconsolata", &["inconsolata"], "humanist, warm")),
    (Role::Code, fam("Menlo", &["menlo"], "the workhorse; a little heavy")),
    (Role::Code, fam("Monaco", &["monaco"], "the classic Macintosh terminal face")),
    (Role::Code, fam("PT Mono", &["ptmono"], "narrow, calm, slightly bookish")),
    (Role::Code, fam("Andale Mono", &["andalemono"], "wide spacing; good at small sizes")),
    (Role::Code, fam("DejaVu Sans Mono", &["dejavusansmono"], "the Linux default; very complete")),
    (Role::Code, fam("Liberation Mono", &["liberationmono"], "metric-compatible with Courier")),
    (Role::Code, fam("Noto Sans Mono", &["notosansmono"], "wide language coverage")),
    (Role::Code, fam("Ubuntu Mono", &["ubuntumono"], "distinctive, small on the body")),
    (Role::Code, fam("Roboto Mono", &["robotomono"], "neutral, geometric")),
    (Role::Code, fam("Cousine", &["cousine"], "metric-compatible with Courier New")),
    (Role::Code, fam("FreeMono", &["freemono"], "the GNU FreeFont mono")),
    (Role::Code, fam("Courier New", &["couriernew", "nimbusmono"], "typewriter. Sometimes.")),
    // ---- prose ----
    (Role::Prose, fam("New York", &["newyork"], "elegant, but its underscore is a hairline")),
    (Role::Prose, fam("Iowan Old Style", &["iowanoldstyle"], "a book face: warm, calligraphic")),
    (Role::Prose, fam("Charter", &["charter"], "designed to survive cheap printing")),
    (Role::Prose, fam("Source Serif", &["sourceserif"], "a screen serif with real italics")),
    (Role::Prose, fam("EB Garamond", &["ebgaramond"], "the renaissance letter")),
    (Role::Prose, fam("Garamond", &["garamond"], "old-style, quiet")),
    (Role::Prose, fam("Crimson", &["crimson"], "old-style, light, bookish")),
    (Role::Prose, fam("Lora", &["lora"], "brushy, contemporary")),
    (Role::Prose, fam("Libre Baskerville", &["librebaskerville"], "high contrast")),
    (Role::Prose, fam("Baskerville", &["baskerville"], "elegant, a little precious")),
    (Role::Prose, fam("Merriweather", &["merriweather"], "sturdy, screen-first")),
    (Role::Prose, fam("PT Serif", &["ptserif"], "Cyrillic roots; even colour")),
    (Role::Prose, fam("Noto Serif", &["notoserif"], "wide language coverage")),
    (Role::Prose, fam("Athelas", &["athelas"], "humanist, almost handwritten")),
    (Role::Prose, fam("Palatino", &["palatino", "p052", "urwpalladio"], "the renaissance letter, digitised")),
    (Role::Prose, fam("Georgia", &["georgia"], "sturdy screen serif, old-style figures")),
    (Role::Prose, fam("DejaVu Serif", &["dejavuserif"], "the Linux default serif")),
    (Role::Prose, fam("Liberation Serif", &["liberationserif"], "metric-compatible with Times")),
    (Role::Prose, fam("Gentium", &["gentium", "charis"], "designed for legibility")),
    (Role::Prose, fam("FreeSerif", &["freeserif"], "the GNU FreeFont serif")),
    // ---- chrome ----
    (Role::Ui, fam("SF Pro", &["sfns", "sfpro"], "the system voice")),
    (Role::Ui, fam("Inter", &["inter"], "neutral, made for screens")),
    (Role::Ui, fam("Helvetica Neue", &["helveticaneue", "helvetica"], "neutral, invisible")),
    (Role::Ui, fam("Avenir Next", &["avenirnext", "avenir"], "geometric warmth")),
    (Role::Ui, fam("Optima", &["optima"], "humanist, flared; good for labels")),
    (Role::Ui, fam("Cantarell", &["cantarell"], "GNOME's interface face")),
    (Role::Ui, fam("Ubuntu", &["ubuntu"], "Ubuntu's interface face")),
    (Role::Ui, fam("Source Sans", &["sourcesans"], "Adobe's interface face")),
    (Role::Ui, fam("Open Sans", &["opensans"], "ubiquitous, clear")),
    (Role::Ui, fam("Noto Sans", &["notosans"], "wide language coverage")),
    (Role::Ui, fam("DejaVu Sans", &["dejavusans"], "the Linux default sans")),
    (Role::Ui, fam("Liberation Sans", &["liberationsans"], "metric-compatible with Arial")),
    (Role::Ui, fam("Roboto", &["roboto"], "Android's interface face")),
    (Role::Ui, fam("FreeSans", &["freesans"], "the GNU FreeFont sans")),
    (Role::Ui, fam("Geneva", &["geneva"], "the original Mac UI face")),
];

/// Fallback faces for glyphs the chosen voices do not have (box drawing, marks).
/// These are looked for by name like everything else; absence is not an error.
/// Faces worth appending to every chain purely for their symbol coverage. The marks the
/// interface draws — triangles, diamonds, arrows — are in nobody's idea of a coding font,
/// and Linux distributions differ on which of these they install. DejaVu is the one that
/// is nearly always present.
static SYMBOL_STEMS: &[&str] = &[
    "applesymbols",
    "symbola",
    "notosanssymbols",
    "dejavusans",
    "freeserif",
    "freesans",
    "unifont",
    "segoeuisymbol",
];

#[derive(Clone, Debug)]
pub struct Voice {
    pub label: String,
    pub path: PathBuf,
    pub index: u32,
    pub note: String,
    /// The height of its lowercase letters, in em units. Faces agree about the em and
    /// disagree about everything else, so this is what makes two faces look like they are
    /// set at the same size when they are set at the same size.
    pub x_height: f32,
}

/// A font file found on this machine.
struct Found {
    label: &'static str,
    note: &'static str,
    path: PathBuf,
    role: Role,
    /// Rank within the whole preference list; lower is better.
    rank: usize,
}

/// The font directories of this platform, in the order they should be searched.
pub fn font_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        dirs.push(home.join("Library/Fonts"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(win) = std::env::var("SystemRoot") {
            dirs.push(PathBuf::from(win).join("Fonts"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        dirs.push(PathBuf::from("/run/host/fonts"));
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

/// `jetbrainsmono-regular` -> `jetbrainsmonoregular`.
fn normalize(stem: &str) -> String {
    stem.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn is_style_variant(stem: &str) -> bool {
    let s = normalize(stem);
    [
        "bold", "italic", "oblique", "light", "thin", "black", "medium", "semibold",
    ]
    .iter()
    .any(|v| s.contains(v))
}

/// How good a match is this file for its family and role? Lower is better, and
/// `None` means "never use this one".
///
/// File names are all a font directory gives us, and they lie in exactly one way that
/// matters here: a family may ship a *proportional* sibling under a name that contains
/// the family's (`HackNerdFontPropo`). Setting code in a proportional face is not a
/// stylistic error, it destroys the alignment a reader relies on, so those are refused
/// outright and the monospace sibling is preferred when both are present.
fn match_quality(key: &str, role: Role) -> Option<i32> {
    let mut score = 0;
    if role == Role::Code {
        if key.contains("propo") || key.contains("proportional") || key.contains("nfpropo") {
            return None;
        }
        if !key.contains("mono") {
            score += 10;
        }
    } else {
        if key.contains("mono") {
            return None; // a monospace face is never the prose or chrome voice
        }
        if key.contains("condensed") || key.contains("narrow") {
            score += 5;
        }
    }
    if key.contains("display") || key.contains("poster") {
        return None; // display faces are not for reading at 15 pt
    }
    Some(score)
}

/// Walk the platform's font directories and match files against the family list.
fn discover() -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    let mut best_for: std::collections::BTreeMap<String, (i32, Found)> =
        std::collections::BTreeMap::new();
    // A machine with no system fonts at all (or a test run) yields nothing here, and
    // the caller falls back to egui's bundled faces.
    if std::env::var("ZENITH_NO_SYSTEM_FONTS").is_ok() {
        return found;
    }

    let mut visited = 0usize;
    for dir in font_dirs() {
        for path in walk(&dir, 4, &mut visited) {
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if is_style_variant(stem) {
                continue; // the italic is found from the regular face, not listed here
            }
            let key = normalize(stem);
            // The longest matching family stem wins, so DejaVu Sans Mono is not
            // mistaken for DejaVu Sans.
            let mut best: Option<(usize, Role, &'static Family)> = None;
            for (role, family) in FAMILIES {
                for s in family.stems {
                    if key.contains(s) {
                        let len = s.len();
                        if best.map(|(b, _, _)| len > b).unwrap_or(true) {
                            best = Some((len, *role, family));
                        }
                    }
                }
            }
            let Some((_, role, family)) = best else {
                continue;
            };
            let Some(quality) = match_quality(&key, role) else {
                continue;
            };
            let rank = FAMILIES
                .iter()
                .position(|(r, f)| *r == role && f.label == family.label)
                .unwrap_or(usize::MAX);
            let candidate = Found {
                label: family.label,
                note: family.note,
                path,
                role,
                rank,
            };
            // One voice per family per role: the best-scoring file wins, and among
            // equals the shorter path (so `X.ttf` beats `X-VariableFont_wght.ttf`).
            let key = format!("{}:{role:?}", family.label);
            let better = match best_for.get(&key) {
                None => true,
                Some((score, existing)) => {
                    quality < *score || (quality == *score && candidate.path < existing.path)
                }
            };
            if better {
                best_for.insert(key, (quality, candidate));
            }
        }
    }
    found.extend(best_for.into_values().map(|(_, f)| f));
    found.sort_by(|a, b| a.rank.cmp(&b.rank).then(a.path.cmp(&b.path)));
    found
}

/// Recursively list font files, bounded so a pathological directory cannot stall
/// startup.
fn walk(dir: &Path, depth: usize, visited: &mut usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if depth == 0 || *visited > 40_000 {
        return out;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        *visited += 1;
        let Ok(kind) = entry.file_type() else { continue };
        // `file_type` does not follow symlinks, so a symlinked directory is skipped
        // rather than risking a loop.
        if kind.is_dir() {
            out.extend(walk(&entry.path(), depth - 1, visited));
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
    out
}

/// Candidate voices for a role, best first, from what this machine actually has.
pub fn voices(role: Role) -> Vec<Voice> {
    discover()
        .into_iter()
        .filter(|f| f.role == role)
        .map(|f| Voice {
            x_height: x_height(&f.path, 0).unwrap_or(0.5),
            label: f.label.to_string(),
            path: f.path,
            index: 0,
            note: f.note.to_string(),
        })
        .collect()
}

/// Every essential character a face must be able to draw to be fit for reading code.
///
/// This list is not decoration. A face that cannot draw the underscore does not get to
/// set a source file: `snake_case_name` is not `snake case name`.
pub const ESSENTIAL: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 \
_-+*/\\|&%$#@!?.,;:'\"`^~()[]{}<>=";

/// Which essential characters this face cannot draw?
pub fn missing(bytes: &[u8], index: u32) -> Vec<char> {
    let Ok(font) = FontRef::from_index(bytes, index) else {
        return ESSENTIAL.chars().filter(|c| !c.is_whitespace()).collect();
    };
    let map = font.charmap();
    ESSENTIAL
        .chars()
        .filter(|c| !c.is_whitespace() && map.map(*c).is_none())
        .collect()
}

pub fn loads(voice: &Voice) -> bool {
    loads_path(&voice.path, voice.index)
}

pub fn loads_path(path: &Path, index: u32) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    parseable(&bytes, index)
}

fn parseable(bytes: &[u8], index: u32) -> bool {
    // Identical to epaint's own font loading call, including TrueType-collection
    // face selection.
    let Ok(font) = FontRef::from_index(bytes, index) else {
        return false;
    };
    let Some(gid) = font.charmap().map('M') else {
        return false;
    };
    if font.outline_glyphs().get(gid).is_none() {
        return false;
    }
    missing(bytes, index).is_empty() && underscore_visible(&font)
}

/// The size of a face's underscore glyph, in em units: `(width, height, depth)`.
///
/// A character map is a claim, not a promise. New York — Apple's handsome screen serif
/// — maps `_` to a glyph drawn **0.005 em thick**, which at a 15 pt reading size is
/// under a fifth of a pixel and therefore invisible: `snake_case_name` is rendered as
/// `snake case name`. Measured across the faces on this machine, nothing else is
/// thinner than 0.047 em, so the test below has a ten-fold margin and is a small-text
/// fitness check rather than a matter of taste.
fn underscore_size(font: &FontRef) -> Option<(f32, f32, f32)> {
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let em = metrics.units_per_em as f32;
    if em <= 0.0 {
        return None;
    }
    let gid = font.charmap().map('_')?;
    let glyph = font.outline_glyphs().get(gid)?;
    let mut pen = Bounds::default();
    let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
    glyph.draw(settings, &mut pen).ok()?;
    let (w, h) = (pen.width(), pen.height());
    Some((w / em, h / em, -pen.min.1 / em))
}

/// The height of a face's lowercase letters, in em units, measured from a drawn `x`.
///
/// A monospace drawn for source code has an x-height around 0.53 em; a book serif around
/// 0.45. Set at the same point size, the prose is then visibly smaller than the code it
/// sits among — not because it is set smaller, but because a point is an em and ems are
/// not the same size in the eye. This is the measurement that lets the page correct for
/// it, and it is measured from the glyph rather than read from `OS/2`, because a table
/// is a claim and an outline is a fact.
fn x_height(path: &Path, index: u32) -> Option<f32> {
    let bytes = std::fs::read(path).ok()?;
    // `from_index`, not `new`: a `.ttc` holds several faces, and the first of them is
    // usually not the one the page is set in — which is how every serif on this machine
    // measured the same and none of them measured right.
    let font = FontRef::from_index(&bytes, index).ok()?;
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let em = metrics.units_per_em as f32;
    if em <= 0.0 {
        return None;
    }
    let glyph = font.outline_glyphs().get(font.charmap().map('x')?)?;
    let mut pen = Bounds::default();
    let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
    glyph.draw(settings, &mut pen).ok()?;
    let height = pen.height() / em;
    // Sane lowercase heights only: a face that reports something wild is not measured.
    (0.3..=0.8).contains(&height).then_some(height)
}

const UNDERSCORE_MIN_WIDTH: f32 = 0.10;
const UNDERSCORE_MIN_HEIGHT: f32 = 0.02;

fn underscore_visible(font: &FontRef) -> bool {
    match underscore_size(font) {
        Some((w, h, _)) => w >= UNDERSCORE_MIN_WIDTH && h >= UNDERSCORE_MIN_HEIGHT,
        None => false,
    }
}

/// An outline pen that measures the box a glyph actually covers.
#[derive(Default)]
struct Bounds {
    min: (f32, f32),
    max: (f32, f32),
    any: bool,
}

impl Bounds {
    fn see(&mut self, x: f32, y: f32) {
        if self.any {
            self.min = (self.min.0.min(x), self.min.1.min(y));
            self.max = (self.max.0.max(x), self.max.1.max(y));
        } else {
            self.min = (x, y);
            self.max = (x, y);
            self.any = true;
        }
    }

    fn width(&self) -> f32 {
        if self.any {
            self.max.0 - self.min.0
        } else {
            0.0
        }
    }

    fn height(&self) -> f32 {
        if self.any {
            self.max.1 - self.min.1
        } else {
            0.0
        }
    }
}

impl OutlinePen for Bounds {
    fn move_to(&mut self, x: f32, y: f32) {
        self.see(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.see(x, y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.see(cx0, cy0);
        self.see(x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.see(cx0, cy0);
        self.see(cx1, cy1);
        self.see(x, y);
    }
    fn close(&mut self) {}
}

/// One face inside a font file (a `.ttc` holds several).
pub struct Face {
    pub index: u32,
    pub italic: bool,
    pub weight: f32,
    pub missing: Vec<char>,
    pub usable: bool,
    /// Underscore size in em units: `(width, height, depth)`.
    pub underscore: Option<(f32, f32, f32)>,
}

/// Enumerate the faces in a font file, for `--faces`.
pub fn faces(path: &Path) -> Vec<Face> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0..32u32 {
        let Ok(font) = FontRef::from_index(&bytes, index) else {
            break;
        };
        let attrs = font.attributes();
        let miss = missing(&bytes, index);
        let under = underscore_visible(&font);
        out.push(Face {
            index,
            italic: matches!(attrs.style, Style::Italic | Style::Oblique(_)),
            weight: attrs.weight.value(),
            usable: miss.is_empty() && under,
            missing: miss,
            underscore: underscore_size(&font),
        });
    }
    out
}

/// A matching italic face for a voice, found by asking the fonts themselves.
///
/// Filenames are the fast path (`Georgia.ttf` -> `Georgia Italic.ttf`), but a `.ttc`
/// holds several faces in one file and the suffix says nothing, so there we read each
/// face's OS/2 style and weight and pick the italic that matches the regular.
fn italic_sibling(voice: &Voice) -> Option<(PathBuf, u32)> {
    let stem = voice.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let dir = voice.path.parent()?;

    let mut candidates: Vec<PathBuf> = Vec::new();
    for suffix in [" Italic", "-Italic", "Italic", "-Oblique", " Oblique"] {
        for ext in ["ttf", "otf", "ttc"] {
            candidates.push(dir.join(format!("{stem}{suffix}.{ext}")));
        }
    }
    if let Some(without) = stem.strip_suffix("-Regular") {
        for suffix in ["-Italic", " Italic", "Italic"] {
            for ext in ["ttf", "otf"] {
                candidates.push(dir.join(format!("{without}{suffix}.{ext}")));
            }
        }
    }
    for cand in candidates {
        if cand == voice.path {
            continue;
        }
        if cand.is_file() && loads_path(&cand, 0) {
            return Some((cand, 0));
        }
    }

    // Another face in the same file (a collection, or a variable font with an
    // italic instance recorded as a separate face).
    let regular_weight = faces(&voice.path)
        .into_iter()
        .find(|f| f.index == voice.index)
        .map(|f| f.weight);
    for face in faces(&voice.path) {
        if face.index == voice.index || !face.italic || !face.usable {
            continue;
        }
        if let Some(w) = regular_weight {
            if (face.weight - w).abs() > 50.0 {
                continue; // a bold italic is not the italic of a regular
            }
        }
        return Some((voice.path.clone(), face.index));
    }
    None
}

/// Pick the voice at `wanted`, or the nearest usable one after it.
fn resolve(voices: &[Voice], wanted: usize) -> Option<(usize, Voice)> {
    if voices.is_empty() {
        return None;
    }
    for step in 0..voices.len() {
        let idx = (wanted + step) % voices.len();
        if loads(&voices[idx]) {
            return Some((idx, voices[idx].clone()));
        }
    }
    None
}

/// Everything the UI needs to know about type.
pub struct Typography {
    /// What this machine can actually draw, out of [`MARKS`].
    pub marks: Marks,
    pub defs: FontDefinitions,

    /// The candidates this machine actually has, in preference order.
    pub mono_voices: Vec<Voice>,
    pub prose_voices: Vec<Voice>,
    pub ui_voices: Vec<Voice>,

    pub mono_idx: usize,
    pub prose_idx: usize,
    pub ui_idx: usize,
    pub mono_label: String,
    pub prose_label: String,
    pub ui_label: String,

    /// True when no system face could be used and egui's bundled fonts are carrying
    /// the page. The app says so rather than pretending.
    pub bundled_fallback: bool,

    pub code: FontFamily,
    pub code_italic: FontFamily,
    pub prose: FontFamily,
    pub prose_italic: FontFamily,
    pub ui: FontFamily,
}

pub const CODE: &str = "zenith-code";
pub const CODE_ITALIC: &str = "zenith-code-italic";
pub const PROSE: &str = "zenith-prose";
pub const PROSE_ITALIC: &str = "zenith-prose-italic";

/// Register a font file under `key`, returning false if it cannot be parsed.
fn add_font(defs: &mut FontDefinitions, key: &str, path: &Path, index: u32, label: &str) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    if !parseable(&bytes, index) {
        eprintln!(
            "zenith: skipping {label} ({}) — cannot be used to set code",
            path.display()
        );
        return false;
    }
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    defs.font_data
        .insert(key.to_string(), std::sync::Arc::new(data));
    true
}

/// Symbol and fallback faces, discovered by stem like the rest.
fn symbol_fonts() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = discover()
        .into_iter()
        .filter(|f| {
            let key = normalize(f.path.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
            SYMBOL_STEMS.iter().any(|s| key.contains(s))
        })
        .map(|f| f.path)
        .collect();
    out.dedup();
    out
}

pub fn build(mono_idx: usize, prose_idx: usize, ui_idx: usize) -> Typography {
    let mut defs = FontDefinitions::default();

    // Keep egui's bundled faces as the tail of every fallback chain.
    let mono_fallback = defs
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    let prop_fallback = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mono_voices = voices(Role::Code);
    let prose_voices = voices(Role::Prose);
    let ui_voices = voices(Role::Ui);

    let mono = resolve(&mono_voices, mono_idx);
    let prose = resolve(&prose_voices, prose_idx);
    let ui = resolve(&ui_voices, ui_idx);
    let bundled_fallback = mono.is_none() && prose.is_none();

    let code = FontFamily::Name(CODE.into());
    let code_italic = FontFamily::Name(CODE_ITALIC.into());
    let prose_fam = FontFamily::Name(PROSE.into());
    let prose_italic_fam = FontFamily::Name(PROSE_ITALIC.into());

    let mut symbols: Vec<String> = Vec::new();
    for (i, path) in symbol_fonts().iter().enumerate() {
        let key = format!("zenith-symbol-{i}");
        if add_font(&mut defs, &key, path, 0, "symbol fallback") {
            symbols.push(key);
        }
    }

    let chain = |head: Vec<String>, tail: &[String]| -> Vec<String> {
        let mut v = head;
        v.extend(symbols.iter().cloned());
        v.extend(tail.iter().cloned());
        v
    };

    // ---- code ----
    let code_key = "zenith-mono".to_string();
    let (mono_idx, mono_label, mono_italic) = match &mono {
        Some((idx, v)) => {
            add_font(&mut defs, &code_key, &v.path, v.index, &v.label);
            let italic = italic_sibling(v).and_then(|(path, index)| {
                let ikey = "zenith-mono-italic".to_string();
                add_font(&mut defs, &ikey, &path, index, &v.label).then_some(ikey)
            });
            (*idx, v.label.clone(), italic)
        }
        None => (0, "egui's bundled mono".to_string(), None),
    };
    let code_chain = if mono.is_some() {
        chain(vec![code_key.clone()], &mono_fallback)
    } else {
        mono_fallback.clone()
    };
    defs.families.insert(FontFamily::Monospace, code_chain.clone());
    defs.families.insert(code.clone(), code_chain.clone());
    let code_italic_chain = match &mono_italic {
        Some(ikey) => chain(vec![ikey.clone(), code_key.clone()], &mono_fallback),
        None => code_chain.clone(),
    };
    defs.families.insert(code_italic.clone(), code_italic_chain);

    // ---- prose ----
    let prose_key = "zenith-prose-regular".to_string();
    let (prose_idx, prose_label) = match &prose {
        Some((idx, v)) => {
            add_font(&mut defs, &prose_key, &v.path, v.index, &v.label);
            let italic = italic_sibling(v).and_then(|(path, index)| {
                let ikey = "zenith-prose-italic-face".to_string();
                add_font(&mut defs, &ikey, &path, index, &v.label).then_some(ikey)
            });
            let regular_chain = chain(vec![prose_key.clone()], &prop_fallback);
            defs.families.insert(prose_fam.clone(), regular_chain);
            defs.families.insert(
                prose_italic_fam.clone(),
                match &italic {
                    Some(ikey) => chain(vec![ikey.clone(), prose_key.clone()], &prop_fallback),
                    None => chain(vec![prose_key.clone()], &prop_fallback),
                },
            );
            (*idx, v.label.clone())
        }
        None => {
            defs.families.insert(prose_fam.clone(), prop_fallback.clone());
            defs.families
                .insert(prose_italic_fam.clone(), prop_fallback.clone());
            (0, "egui's bundled serif".to_string())
        }
    };

    // ---- chrome ----
    let ui_named = FontFamily::Name("zenith-ui".into());
    let (ui_idx, ui_label) = match &ui {
        Some((idx, v)) => {
            let key = "zenith-ui".to_string();
            add_font(&mut defs, &key, &v.path, v.index, &v.label);
            let ui_chain = chain(vec![key.clone()], &prop_fallback);
            defs.families.insert(ui_named.clone(), ui_chain.clone());
            defs.families.insert(FontFamily::Proportional, ui_chain);
            (*idx, v.label.clone())
        }
        None => {
            defs.families
                .insert(ui_named.clone(), prop_fallback.clone());
            (0, "egui's bundled sans".to_string())
        }
    };

    if bundled_fallback {
        eprintln!("zenith: no usable system typeface; egui's bundled fonts will set the page");
    }

    // The marks are drawn in the interface voice, so that is the chain to ask.
    let marks = Marks::of(&defs, &ui_named);
    Typography {
        marks,
        defs,
        mono_voices,
        prose_voices,
        ui_voices,
        mono_idx,
        prose_idx,
        ui_idx,
        mono_label,
        prose_label,
        ui_label,
        bundled_fallback,
        code,
        code_italic,
        prose: prose_fam,
        prose_italic: prose_italic_fam,
        ui: ui_named,
    }
}

impl Typography {
    pub fn code_id(&self, size: f32) -> FontId {
        FontId::new(size, self.code.clone())
    }
    pub fn code_italic_id(&self, size: f32) -> FontId {
        FontId::new(size, self.code_italic.clone())
    }
    /// How much larger the prose has to be set to look the same size as the code.
    ///
    /// The ratio of the two faces' x-heights: give or take the way a serif and a
    /// monospace sit next to each other, matching lowercase heights is what makes them
    /// read as one page rather than two sizes.
    pub fn prose_scale(&self) -> f32 {
        let code = self.mono_voices.get(self.mono_idx).map(|v| v.x_height);
        let prose = self.prose_voices.get(self.prose_idx).map(|v| v.x_height);
        match (code, prose) {
            (Some(code), Some(prose)) if code > 0.0 && prose > 0.0 => (code / prose).clamp(0.95, 1.35),
            _ => 1.0,
        }
    }

    pub fn prose_id(&self, size: f32) -> FontId {
        FontId::new(size, self.prose.clone())
    }
    pub fn prose_italic_id(&self, size: f32) -> FontId {
        FontId::new(size, self.prose_italic.clone())
    }
    pub fn ui_id(&self, size: f32) -> FontId {
        FontId::new(size, self.ui.clone())
    }

}

fn show_chars(chars: &[char]) -> String {
    chars
        .iter()
        .take(8)
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Why is this face not usable, or what is it good for?
fn verdict(path: &Path, index: u32, note: &str) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return "not readable".to_string();
    };
    let miss = missing(&bytes, index);
    if !miss.is_empty() {
        return format!("unusable: no glyph for {}", show_chars(&miss));
    }
    match FontRef::from_index(&bytes, index).map(|f| underscore_size(&f)) {
        Ok(Some((_, h, _))) if h < UNDERSCORE_MIN_HEIGHT => {
            format!("unusable: its underscore is a {h:.3} em hairline")
        }
        Ok(None) | Err(_) => "unusable: it has no underscore".to_string(),
        Ok(Some(_)) => note.to_string(),
    }
}

/// `zenith --fonts`
pub fn print_available() {
    let dirs = font_dirs();
    println!(
        "zenith typography — three voices, searched in {} director{}",
        dirs.len(),
        if dirs.len() == 1 { "y" } else { "ies" }
    );
    if std::env::var("ZENITH_NO_SYSTEM_FONTS").is_ok() {
        println!("(ZENITH_NO_SYSTEM_FONTS is set: pretending none are installed)");
    }
    println!();
    for role in [Role::Code, Role::Prose, Role::Ui] {
        println!("{}", role.title());
        let list = voices(role);
        if list.is_empty() {
            println!("  ·  none found — egui's bundled faces will be used\n");
            continue;
        }
        for (i, v) in list.iter().enumerate() {
            println!(
                "  {} {}  {:<22} x{:<4.2} {:<52} {}",
                if loads(v) { "✓" } else { "·" },
                if i == 0 { "*" } else { " " },
                v.label,
                v.x_height,
                v.path.display(),
                verdict(&v.path, v.index, &v.note)
            );
        }
        println!();
    }
    println!("x = the height of its lowercase letters, in em. Prose is set larger than code");
    println!("    by the ratio of the two, so a book serif and a coding face look the same size.");
    println!("* = the voice zenith would choose. A face must parse, cover every character");
    println!("  source code needs, and draw a real underscore — a hairline is refused.");
}

/// `zenith --faces FILE`: every face in a font file, with what it can and cannot draw.
pub fn print_faces(path: &Path) {
    let list = faces(path);
    if list.is_empty() {
        println!("{}: not a readable font file", path.display());
        return;
    }
    println!("{} — {} face(s)\n", path.display(), list.len());
    for f in &list {
        let u = match f.underscore {
            Some((w, h, d)) => format!("underscore {w:.2} x {h:.3} em, {d:.3} deep"),
            None => "underscore: no glyph".to_string(),
        };
        println!(
            "  [{}] {:<8} weight {:<5} {:<9} {u}{}",
            f.index,
            if f.italic { "italic" } else { "upright" },
            f.weight,
            if f.usable { "ok" } else { "UNUSABLE" },
            if f.missing.is_empty() {
                String::new()
            } else {
                format!("  missing {}", show_chars(&f.missing))
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_are_normalised() {
        assert_eq!(normalize("JetBrainsMono-Regular"), "jetbrainsmonoregular");
        assert_eq!(normalize("DejaVu Sans Mono"), "dejavusansmono");
    }

    #[test]
    fn style_variants_are_not_primary_voices() {
        assert!(is_style_variant("DejaVuSerif-Italic"));
        assert!(is_style_variant("HackNerdFontMono-Bold"));
        assert!(!is_style_variant("DejaVuSerif"));
    }

    #[test]
    fn the_longest_family_match_wins() {
        // `dejavusansmono` must be read as the mono family, not as DejaVu Sans.
        let key = normalize("DejaVuSansMono");
        let mut best = 0usize;
        let mut winner = "";
        for (_, family) in FAMILIES {
            for s in family.stems {
                if key.contains(s) && s.len() > best {
                    best = s.len();
                    winner = family.label;
                }
            }
        }
        assert_eq!(winner, "DejaVu Sans Mono");
    }
}

// ---------------------------------------------------------------------------
// Marks: can this machine actually draw the interface's punctuation?
// ---------------------------------------------------------------------------

/// The marks the interface draws as icons, with a plain-text stand-in for each.
///
/// egui falls back through the font chain for a glyph the chosen face lacks, but if *no*
/// face in the chain has it the reader gets a tofu box. System fonts disagree wildly here:
/// Apple's have all of these, a minimal Linux install often has none of the dingbats. So
/// the interface asks what this machine can draw, once, and uses letters where it cannot.
pub const MARKS: &[(&str, &str)] = &[
    ("✦", "*"),   // bookmarked
    ("▾", "v"),   // an open folder
    ("▸", ">"),   // a closed folder
    ("❖", "*"),   // a module
    ("◆", "+"),   // a type
    ("◇", "-"),   // a trait
    ("○", "o"),   // a constant
    ("≡", "="),   // an enum
    ("ƒ", "f"),   // a function
    ("§", "#"),   // a heading
    ("↳", "~"),   // a wrapped line
    ("◦", "-"),   // a doorway file
    ("›", "/"),   // a breadcrumb
];

/// What this machine can draw, out of [`MARKS`].
#[derive(Clone, Debug, Default)]
pub struct Marks {
    missing: std::collections::BTreeSet<char>,
}

impl Marks {
    /// Which of the pretty forms can be drawn by *this chain*?
    ///
    /// The chain matters, not the set of installed fonts: egui only looks through the
    /// fallback list of the family it was asked for, so a mark is drawable only if some
    /// face in that list has it.
    pub fn of(defs: &FontDefinitions, family: &FontFamily) -> Marks {
        let mut missing = std::collections::BTreeSet::new();
        let chain = defs.families.get(family).cloned().unwrap_or_default();
        let charmaps: Vec<skrifa::charmap::Charmap> = chain
            .iter()
            .filter_map(|name| defs.font_data.get(name))
            .filter_map(|data| skrifa::FontRef::new(data.font.as_ref()).ok())
            .map(|f| f.charmap())
            .collect();
        for (pretty, _) in MARKS {
            for ch in pretty.chars() {
                let covered = charmaps.iter().any(|cm| cm.map(ch as u32).is_some());
                if !covered {
                    missing.insert(ch);
                }
            }
        }
        Marks { missing }
    }

    /// The pretty form, or its plain stand-in when no face here can draw it.
    pub fn pick(&self, pretty: &'static str) -> &'static str {
        let usable = pretty.chars().all(|c| !self.missing.contains(&c));
        if usable {
            return pretty;
        }
        MARKS
            .iter()
            .find(|(p, _)| *p == pretty)
            .map(|(_, plain)| *plain)
            .unwrap_or(pretty)
    }

    /// One line per mark: what it is, whether it can be drawn, and what will be used.
    pub fn describe(&self) -> Vec<(String, String, bool)> {
        MARKS
            .iter()
            .flat_map(|(pretty, _)| pretty.chars())
            .map(|c| {
                (
                    format!("U+{:04X}  {c}", c as u32),
                    self.pick_str(c),
                    self.missing.contains(&c),
                )
            })
            .collect()
    }

    fn pick_str(&self, c: char) -> String {
        let pretty: String = c.to_string();
        MARKS
            .iter()
            .find(|(p, _)| *p == pretty)
            .map(|(_, plain)| (*plain).to_string())
            .unwrap_or(pretty)
    }
}

/// The modifier key label for this platform, and its shift key.
///
/// `⌘` exists in Apple's fonts and in very few others, so a Linux reader would see a
/// missing glyph — and would not expect a Command key anyway.
pub fn mod_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "\u{2318}"
    } else {
        "Ctrl"
    }
}

pub fn shift_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "\u{21E7}"
    } else {
        "Shift"
    }
}
