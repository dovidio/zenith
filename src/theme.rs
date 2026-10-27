//! Themes are *tonal*, not chromatic.
//!
//! Every editor color scheme you have ever read is a rainbow: six or seven hues at
//! full saturation, competing for attention. That is a fine way to debug and a
//! terrible way to read. Zenith instead assigns each token class a position on a
//! single muted tonal ladder — the same low-chroma band, shifted only slightly in
//! hue and lightness. Keywords are not "blue", they are the *darkest ink*; comments
//! are not "green", they are the *faintest*. Contrast then encodes meaning, and the
//! page stays quiet enough to read for an hour.

use egui::Color32;

// ---------------------------------------------------------------------------
// Tones
// ---------------------------------------------------------------------------

/// The complete vocabulary of ink in zenith. Fourteen tones, no more.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tone {
    /// Identifiers, variables, everything unnamed.
    Plain,
    /// Prose in the margin: line and block comments.
    Comment,
    /// Doc comments — comments that address the reader directly.
    Doc,
    /// Control and declaration keywords: the darkest ink on the page.
    Keyword,
    /// Modifiers, storage classes: keywords spoken softly.
    KeywordSoft,
    /// Types, traits, classes, tags.
    Type,
    /// Function and macro names.
    Call,
    /// Named constants and macro invocations.
    Constant,
    /// String literals.
    Str,
    /// Numeric and boolean literals.
    Number,
    /// Operators.
    Operator,
    /// Punctuation, brackets, separators. Nearly a whisper.
    Punct,
    /// Attributes, decorators, annotations, markup tags.
    Meta,
    /// A code block inside a prose document: always monospace, never serif, even when
    /// the rest of the file is being set as prose.
    Raw,
    /// Something the parser could not make sense of.
    Invalid,
}

impl Tone {
    pub const COUNT: usize = 15;

    pub const ALL: [Tone; Tone::COUNT] = [
        Tone::Plain,
        Tone::Comment,
        Tone::Doc,
        Tone::Keyword,
        Tone::KeywordSoft,
        Tone::Type,
        Tone::Call,
        Tone::Constant,
        Tone::Str,
        Tone::Number,
        Tone::Operator,
        Tone::Punct,
        Tone::Meta,
        Tone::Raw,
        Tone::Invalid,
    ];

    #[inline]
    pub fn index(self) -> usize {
        match self {
            Tone::Plain => 0,
            Tone::Comment => 1,
            Tone::Doc => 2,
            Tone::Keyword => 3,
            Tone::KeywordSoft => 4,
            Tone::Type => 5,
            Tone::Call => 6,
            Tone::Constant => 7,
            Tone::Str => 8,
            Tone::Number => 9,
            Tone::Operator => 10,
            Tone::Punct => 11,
            Tone::Meta => 12,
            Tone::Raw => 13,
            Tone::Invalid => 14,
        }
    }

    /// Should this tone be set in the serif prose face rather than the mono face?
    /// Comments are prose, so they are typeset as prose. Nothing else is.
    #[inline]
    pub fn is_prose(self) -> bool {
        matches!(self, Tone::Comment | Tone::Doc)
    }
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    /// Light themes for daylight reading.
    Daylight,
    /// Dark themes for the small hours.
    Lamplight,
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    /// One line of poetry shown in the theme switcher, because taste needs words.
    pub mood: &'static str,
    pub kind: Mood,

    /// The page.
    pub paper: Color32,
    /// The furniture: tree, headers, footer.
    pub panel: Color32,
    /// Recessed furniture.
    pub panel_deep: Color32,
    /// Hairline separators.
    pub edge: Color32,

    pub ink: Color32,
    pub ink_soft: Color32,
    pub ink_faint: Color32,

    pub accent: Color32,
    pub accent_soft: Color32,
    pub selection: Color32,

    /// Indentation whispers.
    pub guide: Color32,
    /// The color the page fades *to* when a region loses focus.
    pub fade: Color32,

    /// Whether comment ink leans italic (calligraphic) or upright (epigraph).
    pub comment_italic: bool,

    pub tones: [Color32; Tone::COUNT],
}

impl Palette {
    #[inline]
    pub fn tone(&self, t: Tone) -> Color32 {
        self.tones[t.index()]
    }

    #[inline]
    pub fn is_dark(&self) -> bool {
        matches!(self.kind, Mood::Lamplight)
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Build a palette from the fourteen tones plus the page furniture.
#[allow(clippy::too_many_arguments)]
const fn pal(
    name: &'static str,
    mood: &'static str,
    kind: Mood,
    paper: Color32,
    panel: Color32,
    panel_deep: Color32,
    edge: Color32,
    ink: Color32,
    ink_soft: Color32,
    ink_faint: Color32,
    accent: Color32,
    accent_soft: Color32,
    selection: Color32,
    guide: Color32,
    comment_italic: bool,
    tones: [Color32; Tone::COUNT],
) -> Palette {
    Palette {
        name,
        mood,
        kind,
        paper,
        panel,
        panel_deep,
        edge,
        ink,
        ink_soft,
        ink_faint,
        accent,
        accent_soft,
        selection,
        guide,
        fade: paper,
        comment_italic,
        tones,
    }
}

// ---------------------------------------------------------------------------
// The six themes
// ---------------------------------------------------------------------------

pub const THEMES: [Palette; 6] = [LUGANO, MAIONESE, BIBIONE, KUNUNURRA, PANITTHA, NEWSPRINT];

/// **Lugano** — the lake at midday: pale stone, lake blue, cypress, terracotta roofs.
///
/// The signature is water. `Keyword` is the deepest blue in the set, everything else
/// is stone and garden, and the ink is cool rather than brown, because Lugano's paper
/// is granite, not pulp.
pub const LUGANO: Palette = pal(
    "Lugano",
    "lake light on stone, cypress, terracotta",
    Mood::Daylight,
    rgb(0xF5, 0xF1, 0xE8), // paper: pale stone
    rgb(0xEE, 0xE9, 0xDC), // panel
    rgb(0xE5, 0xDE, 0xCD), // panel_deep
    rgb(0xD6, 0xCC, 0xB8), // edge
    rgb(0x2A, 0x2C, 0x30), // ink
    rgb(0x5F, 0x64, 0x6B), // ink_soft
    rgb(0x8B, 0x90, 0x99), // ink_faint
    rgb(0x2F, 0x6B, 0x96), // accent: the lake
    rgb(0x8F, 0xB2, 0xCC), // accent_soft
    rgb(0xDE, 0xE7, 0xEE), // selection
    rgb(0xCB, 0xD2, 0xD8), // guide
    true,
    [
        rgb(0x33, 0x36, 0x3B), // Plain
        rgb(0x8A, 0x90, 0x98), // Comment
        rgb(0x6B, 0x72, 0x7B), // Doc
        rgb(0x1F, 0x4E, 0x75), // Keyword: deep water
        rgb(0x53, 0x7A, 0x9C), // KeywordSoft
        rgb(0x2E, 0x6B, 0x5E), // Type: cypress
        rgb(0xA2, 0x5B, 0x32), // Call: terracotta
        rgb(0x9C, 0x46, 0x36), // Constant
        rgb(0x56, 0x68, 0x2F), // Str: olive
        rgb(0x55, 0x50, 0x7E), // Number
        rgb(0x76, 0x7C, 0x83), // Operator
        rgb(0xA2, 0xA8, 0xAF), // Punct
        rgb(0xA8, 0x79, 0x2A), // Meta: amber
        rgb(0x33, 0x36, 0x3B), // Raw (code in prose)
        rgb(0xB2, 0x31, 0x27), // Invalid
    ],
);

/// **Maionese** — a pale emulsion: egg yolk, oil, a gherkin, white pepper.
///
/// The only high-key theme in the set. Butter-coloured paper, dark olive ink, and a
/// signature gold that is unmistakably yolk. Keywords are the colour of the jar's
/// shadow rather than a black, which is what keeps it cheerful instead of clinical.
pub const MAIONESE: Palette = pal(
    "Maionese",
    "yolk, oil, pickle, white pepper",
    Mood::Daylight,
    rgb(0xFB, 0xF7, 0xE2), // paper: butter
    rgb(0xF3, 0xED, 0xD1), // panel
    rgb(0xEA, 0xE2, 0xBF), // panel_deep
    rgb(0xDA, 0xCF, 0xAA), // edge
    rgb(0x2F, 0x2C, 0x1E), // ink
    rgb(0x6B, 0x66, 0x50), // ink_soft
    rgb(0x98, 0x92, 0x6F), // ink_faint
    rgb(0xB9, 0x8E, 0x14), // accent: yolk
    rgb(0xDF, 0xC8, 0x7E), // accent_soft
    rgb(0xEF, 0xE6, 0xBC), // selection
    rgb(0xDC, 0xD2, 0xA8), // guide
    true,
    [
        rgb(0x3A, 0x36, 0x26), // Plain
        rgb(0x94, 0x8C, 0x63), // Comment
        rgb(0x73, 0x6C, 0x46), // Doc
        rgb(0x45, 0x40, 0x1A), // Keyword: olive shadow
        rgb(0x6F, 0x68, 0x32), // KeywordSoft
        rgb(0x4F, 0x6B, 0x33), // Type: gherkin
        rgb(0xA0, 0x5A, 0x28), // Call: toast
        rgb(0xA8, 0x44, 0x2E), // Constant: paprika
        rgb(0x5E, 0x6B, 0x2E), // Str: dill
        rgb(0x6A, 0x5A, 0x8C), // Number
        rgb(0x7C, 0x76, 0x54), // Operator
        rgb(0xAD, 0xA4, 0x7E), // Punct
        rgb(0xA8, 0x79, 0x1A), // Meta: mustard
        rgb(0x3A, 0x36, 0x26), // Raw (code in prose)
        rgb(0xB2, 0x31, 0x27), // Invalid
    ],
);

/// **Bibione** — sand, pine shade, the Adriatic, a striped umbrella.
///
/// Green and teal rather than blue: Bibione is a beach behind a pine wood, and the
/// pines set the temperature of the theme. Sand paper keeps the light warm.
pub const BIBIONE: Palette = pal(
    "Bibione",
    "sand, pine shade, the Adriatic",
    Mood::Daylight,
    rgb(0xF7, 0xF2, 0xE4), // paper: sand
    rgb(0xEF, 0xE8, 0xD4), // panel
    rgb(0xE5, 0xDC, 0xC2), // panel_deep
    rgb(0xD5, 0xC8, 0xA8), // edge
    rgb(0x27, 0x30, 0x2C), // ink: pine shadow
    rgb(0x5A, 0x66, 0x60), // ink_soft
    rgb(0x8B, 0x96, 0x8F), // ink_faint
    rgb(0x1F, 0x7A, 0x8C), // accent: shallow sea
    rgb(0x86, 0xBC, 0xC4), // accent_soft
    rgb(0xDB, 0xE8, 0xE1), // selection
    rgb(0xCD, 0xD8, 0xC9), // guide
    true,
    [
        rgb(0x30, 0x3A, 0x34), // Plain
        rgb(0x88, 0x95, 0x8A), // Comment
        rgb(0x69, 0x75, 0x5F), // Doc
        rgb(0x1D, 0x5F, 0x6E), // Keyword: deep sea
        rgb(0x4E, 0x84, 0x94), // KeywordSoft
        rgb(0x2F, 0x6B, 0x45), // Type: pine
        rgb(0xB4, 0x62, 0x2F), // Call: umbrella stripe
        rgb(0xA6, 0x43, 0x2F), // Constant
        rgb(0x5B, 0x6B, 0x2C), // Str: dune grass
        rgb(0x4E, 0x5A, 0x86), // Number
        rgb(0x6F, 0x7A, 0x70), // Operator
        rgb(0x9B, 0xA4, 0x96), // Punct
        rgb(0xB0, 0x81, 0x2A), // Meta: sunlit amber
        rgb(0x30, 0x3A, 0x34), // Raw (code in prose)
        rgb(0xB2, 0x31, 0x27), // Invalid
    ],
);

/// **Kununurra** — red rock at dusk: pindan earth, sandstone, spinifex, the Ord.
///
/// The rock *is* the ink here. Keywords are the brightest vermilion in the set, types are
/// sandstone, constants are the deeper red of the escarpment, and the ground is warm
/// enough to read as earth rather than as a dark theme that happens to be brown. Turquoise
/// appears once — the accent, for links and the cursor — because in the Kimberley the
/// water is the thing you see *between* the rock, and that is how it should land: a
/// counterpoint, never the signature. An earlier version had this the other way round,
/// with turquoise keywords on a near-black ground, and it read as teal, not as country.
pub const KUNUNURRA: Palette = pal(
    "Kununurra",
    "red rock, sunset ochre, the Ord's turquoise",
    Mood::Lamplight,
    rgb(0x34, 0x1A, 0x12), // paper: pindan at dusk
    rgb(0x2B, 0x14, 0x0D), // panel
    rgb(0x22, 0x10, 0x0A), // panel_deep
    rgb(0x5A, 0x32, 0x20), // edge
    rgb(0xF2, 0xDE, 0xC4), // ink: warm sand
    rgb(0xC9, 0xA9, 0x8B), // ink_soft
    rgb(0x8E, 0x76, 0x60), // ink_faint
    rgb(0x4F, 0xB3, 0xA8), // accent: the Ord, and the only cool thing here
    rgb(0x2C, 0x6A, 0x63), // accent_soft
    rgb(0x4A, 0x27, 0x18), // selection
    rgb(0x43, 0x26, 0x1A), // guide
    true,
    [
        rgb(0xF2, 0xDE, 0xC4), // Plain
        rgb(0x9E, 0x81, 0x68), // Comment
        rgb(0xC0, 0xA1, 0x83), // Doc
        rgb(0xF4, 0x79, 0x4A), // Keyword: rock in the sun
        rgb(0xC2, 0x5B, 0x36), // KeywordSoft
        rgb(0xE8, 0xA1, 0x65), // Type: sandstone
        rgb(0xFF, 0x9E, 0x5C), // Call: a sunlit face
        rgb(0xE2, 0x63, 0x3F), // Constant: the escarpment
        rgb(0xA9, 0xC0, 0x6A), // Str: spinifex
        rgb(0xF0, 0xC0, 0x70), // Number: gold
        rgb(0xC3, 0x9C, 0x7A), // Operator
        rgb(0x8A, 0x6C, 0x55), // Punct
        rgb(0xFF, 0xE0, 0xA0), // Meta: a lit edge
        rgb(0xF2, 0xDE, 0xC4), // Raw (code in prose)
        rgb(0xFF, 0x6B, 0x5A), // Invalid
    ],
);

/// **Panittha** — temple gold, jade, teak, saffron, a little lacquer.
///
/// Gold is the signature and jade is its counterweight, on a ground of dark teak.
/// Numbers drift towards lotus pink, which is the one frivolous decision in the set
/// and is entirely in keeping with the name.
pub const PANITTHA: Palette = pal(
    "Panittha",
    "temple gold, jade, teak, saffron",
    Mood::Lamplight,
    rgb(0x16, 0x13, 0x0F), // paper: teak
    rgb(0x12, 0x0F, 0x0C), // panel
    rgb(0x0E, 0x0C, 0x0A), // panel_deep
    rgb(0x33, 0x29, 0x1D), // edge
    rgb(0xEF, 0xE6, 0xD2), // ink: silk
    rgb(0xA7, 0x97, 0x78), // ink_soft
    rgb(0x7A, 0x6C, 0x52), // ink_faint
    rgb(0xD9, 0xA9, 0x3C), // accent: gold leaf
    rgb(0x6E, 0x55, 0x22), // accent_soft
    rgb(0x2A, 0x21, 0x14), // selection
    rgb(0x2C, 0x22, 0x16), // guide
    true,
    [
        rgb(0xE7, 0xDC, 0xC4), // Plain
        rgb(0x92, 0x80, 0x5F), // Comment
        rgb(0xAF, 0x9B, 0x75), // Doc
        rgb(0xE5, 0xB8, 0x4A), // Keyword: gold leaf
        rgb(0xB0, 0x8F, 0x3C), // KeywordSoft
        rgb(0x6F, 0xBF, 0x9B), // Type: jade
        rgb(0xE3, 0x9A, 0x3C), // Call: saffron
        rgb(0xD4, 0x55, 0x3F), // Constant: lacquer
        rgb(0x9B, 0xB8, 0x6A), // Str: young jade
        rgb(0xD9, 0x8B, 0xA8), // Number: lotus
        rgb(0xA7, 0x93, 0x72), // Operator
        rgb(0x75, 0x65, 0x4A), // Punct
        rgb(0xE8, 0xC4, 0x6A), // Meta: gold
        rgb(0xE7, 0xDC, 0xC4), // Raw (code in prose)
        rgb(0xE8, 0x60, 0x4A), // Invalid
    ],
);

/// **Newsprint** — cool grey stock, set tight, and read with sharp eyes.
///
/// The one theme with no warmth in it at all. It exists for daylight offices and for
/// readers who want the code to have no opinion about atmosphere.
pub const NEWSPRINT: Palette = pal(
    "Newsprint",
    "cold type, cheap stock, sharp eyes",
    Mood::Daylight,
    rgb(0xF2, 0xF3, 0xF4), // paper
    rgb(0xEA, 0xEC, 0xEE), // panel
    rgb(0xE2, 0xE5, 0xE8), // panel_deep
    rgb(0xD2, 0xD7, 0xDC), // edge
    rgb(0x1E, 0x21, 0x24), // ink
    rgb(0x5C, 0x63, 0x69), // ink_soft
    rgb(0x8A, 0x92, 0x99), // ink_faint
    rgb(0x2F, 0x5D, 0x8A), // accent
    rgb(0x93, 0xAE, 0xC4), // accent_soft
    rgb(0xDC, 0xE3, 0xEA), // selection
    rgb(0xC9, 0xD1, 0xD8), // guide
    false,
    [
        rgb(0x2A, 0x2E, 0x32), // Plain
        rgb(0x7C, 0x84, 0x8B), // Comment
        rgb(0x62, 0x6B, 0x73), // Doc
        rgb(0x24, 0x55, 0x7F), // Keyword
        rgb(0x4A, 0x7C, 0xA8), // KeywordSoft
        rgb(0x2F, 0x6B, 0x70), // Type
        rgb(0x9A, 0x64, 0x30), // Call
        rgb(0x9C, 0x4A, 0x3F), // Constant
        rgb(0x4F, 0x6B, 0x3A), // Str
        rgb(0x4F, 0x4A, 0x8A), // Number
        rgb(0x6E, 0x76, 0x7D), // Operator
        rgb(0x98, 0xA0, 0xA7), // Punct
        rgb(0x9A, 0x64, 0x30), // Meta
        rgb(0x2A, 0x2E, 0x32), // Raw (code in prose)
        rgb(0xB0, 0x3A, 0x33), // Invalid
    ],
);

/// Look a palette up by (case-insensitive) name; falls back to Lugano.
pub fn by_name(name: &str) -> Palette {
    THEMES
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
        .copied()
        .unwrap_or(LUGANO)
}

/// Mix two colors (t=0 -> a, t=1 -> b). Used to fade text toward the page.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// Scale a color's alpha (for scrims and hairlines).
pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

// ---------------------------------------------------------------------------
// The same palette, as CSS
// ---------------------------------------------------------------------------

/// The themes as cascading style sheets: the site and the app share one source of truth.
///
/// `zenith --css > static/zenith.css` writes what a web page needs to look like the page
/// the app draws: the same six palettes as custom properties, the same tonal ladder for
/// code, the same three voices, the same measure and leading — and a mapping from the
/// classes Chroma emits (`k`, `nf`, `c1`, …) onto those tones, so syntax highlighting on
/// the web is the same fourteen inks rather than a second colour scheme that merely
/// resembles this one.
pub fn css() -> String {
    thread_local! {
        static DARK_DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    let mut out = String::new();
    out.push_str(HEAD);
    // Every theme, keyed by its lower-case name, so a page can switch with one attribute.
    for p in THEMES.iter() {
        let key = p.name.to_ascii_lowercase();
        let vars = |indent: &str| {
            let mut v = String::new();
            let mut add = |name: &str, c: Color32| {
                v.push_str(&format!("{indent}--{name}: {};\n", hex(c)));
            };
            add("paper", p.paper);
            add("panel", p.panel);
            add("panel-deep", p.panel_deep);
            add("edge", p.edge);
            add("ink", p.ink);
            add("ink-soft", p.ink_soft);
            add("ink-faint", p.ink_faint);
            add("accent", p.accent);
            add("accent-soft", p.accent_soft);
            add("selection", p.selection);
            for tone in Tone::ALL {
                let name = tone_name(tone);
                add(&format!("tone-{name}"), p.tone(tone));
            }
            v
        };
        if key == "lugano" {
            out.push_str(":root {\n");
            out.push_str(&vars("  "));
            out.push_str("}\n\n");
        }
        out.push_str(&format!("[data-theme=\"{key}\"] {{\n"));
        out.push_str(&vars("  "));
        out.push_str("}\n\n");
        // A reader whose system asks for dark gets one of the two lamplit palettes,
        // unless the page has chosen a theme of its own.
        if p.is_dark() && !DARK_DONE.with(|d| d.replace(true)) {
            out.push_str("@media (prefers-color-scheme: dark) {\n  :root {\n");
            out.push_str(&vars("    "));
            out.push_str("  }\n}\n\n");
        }
    }
    out.push_str(RULES);
    out.push_str(CHROMA);
    out
}

/// `snake_case` name for a tone, for a custom property.
fn tone_name(t: Tone) -> String {
    let raw = format!("{t:?}");
    let mut out = String::new();
    for (i, c) in raw.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// What both the site and the app agree on, whatever the palette.
const HEAD: &str = r#"/* zenith — the reading room's ink, for the web.
   Generated by `zenith --css`; the palettes live in src/theme.rs. */

"#;

const RULES: &str = r#":root {
  --voice-code: ui-monospace, "SF Mono", "JetBrains Mono", "Hack", "DejaVu Sans Mono",
    "Liberation Mono", Menlo, Consolas, monospace;
  --voice-prose: "Iowan Old Style", "Charter", "Palatino Linotype", "Athelas", Georgia,
    ui-serif, serif;
  --voice-ui: ui-sans-serif, -apple-system, "SF Pro Text", "Helvetica Neue", "Avenir Next",
    "Optima", "Segoe UI", sans-serif;

  --measure: 34rem;
  --leading: 1.62;
  --h1: 1.7em;
  --h2: 1.4em;
  --h3: 1.22em;
  --h4: 1.12em;
}

html { background: var(--paper); }
body {
  margin: 0 auto;
  padding: 3rem 1.25rem 6rem;
  max-width: var(--measure);
  background: var(--paper);
  color: var(--ink-soft);
  font-family: var(--voice-prose);
  font-size: 17px;
  line-height: var(--leading);
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
}
::selection { background: var(--selection); }

/* Chrome: the strip along the top and the foot, in the furniture's colour. */
header, footer, nav {
  font-family: var(--voice-ui);
  font-size: 0.82em;
  color: var(--ink-faint);
  letter-spacing: 0.01em;
}
header { padding-bottom: 0.6rem; border-bottom: 1px solid var(--edge); margin-bottom: 2rem; }
footer { margin-top: 4rem; border-top: 1px solid var(--edge); padding-top: 0.8rem; }

/* Headings are the same serif, larger, in the page's strongest ink, with air above. */
h1, h2, h3, h4, h5, h6 {
  font-family: var(--voice-prose);
  font-weight: 400;
  color: var(--tone-keyword);
  line-height: 1.25;
  margin: 2.2em 0 0.5em;
}
h1 { font-size: var(--h1); margin-top: 1.4em; }
h2 { font-size: var(--h2); }
h3 { font-size: var(--h3); }
h4 { font-size: var(--h4); }
h5, h6 { font-size: 1em; }

p { margin: 0 0 1.1em; }
a { color: var(--accent); text-decoration: none; border-bottom: 1px solid var(--accent-soft); }
a:hover { border-bottom-color: var(--accent); }
strong { color: var(--tone-keyword); letter-spacing: 0.01em; font-weight: 600; }
em { font-style: italic; }
/* An underscore between word characters is not emphasis, and neither is a name. */
code, kbd, samp {
  font-family: var(--voice-code);
  font-size: 0.92em;
  color: var(--tone-str);
  background: color-mix(in srgb, var(--ink-faint) 10%, transparent);
  border-radius: 3px;
  padding: 0.05em 0.28em;
}
pre {
  font-family: var(--voice-code);
  font-size: 0.86em;
  line-height: 1.5;
  background: color-mix(in srgb, var(--ink-faint) 10%, transparent);
  border-radius: 4px;
  padding: 0.9rem 1rem;
  overflow-x: auto;
}
pre code { background: none; padding: 0; font-size: 1em; color: var(--tone-plain); }

blockquote {
  margin: 1.4em 0;
  padding: 0.7rem 1rem;
  background: color-mix(in srgb, var(--ink-faint) 8%, transparent);
  border-left: 2px solid var(--accent-soft);
  font-style: italic;
  color: var(--ink-faint);
}
blockquote p:last-child { margin-bottom: 0; }

hr { border: 0; border-top: 1px solid var(--edge); margin: 2.4em 0; }
ul, ol { padding-left: 1.4em; }
li::marker { color: var(--ink-faint); }
img { max-width: 100%; height: auto; border-radius: 3px; }

/* The line-number gutter Chroma leaves in the margin, as a file's would be. */
.highlight { position: relative; }
.ln, .lnt {
  font-family: var(--voice-code);
  color: color-mix(in srgb, var(--ink-faint) 45%, transparent);
  user-select: none;
}
"#;

/// Chroma's classes onto the fourteen tones.
///
/// Chroma names a token by what it is in a grammar — `k` for keyword, `nf` for a function
/// name, `si` for a string interpolation. These are the ones the posts actually use;
/// anything left out falls through to the plain ink, which is the honest default.
const CHROMA: &str = r#"
/* Syntax highlighting, in the same fourteen inks the app uses. */
.chroma { color: var(--tone-plain); }
.chroma .c, .chroma .c1, .chroma .cm, .chroma .cs, .chroma .cd {
  font-family: var(--voice-prose);
  font-style: italic;
  color: var(--tone-comment);
}
.chroma .k, .chroma .kc, .chroma .kn, .chroma .kr, .chroma .kd, .chroma .kt {
  color: var(--tone-keyword);
}
.chroma .kc, .chroma .kn { color: var(--tone-keyword-soft); }
.chroma .nc, .chroma .nn, .chroma .no, .chroma .nb, .chroma .bp {
  color: var(--tone-type);
}
.chroma .nf, .chroma .fm, .chroma .nd { color: var(--tone-call); }
.chroma .nd, .chroma .na, .chroma .nt { color: var(--tone-meta); }
.chroma .s, .chroma .s1, .chroma .s2, .chroma .sb, .chroma .sc, .chroma .sd,
.chroma .se, .chroma .sh, .chroma .si, .chroma .sx, .chroma .sr, .chroma .ss {
  color: var(--tone-str);
}
.chroma .m, .chroma .mb, .chroma .mf, .chroma .mh, .chroma .mi, .chroma .mo,
.chroma .il, .chroma .no { color: var(--tone-number); }
.chroma .o, .chroma .ow, .chroma .p { color: var(--tone-operator); }
.chroma .p { color: var(--tone-punct); }
.chroma .n, .chroma .nx, .chroma .nv, .chroma .nl, .chroma .py { color: var(--tone-plain); }
.chroma .err, .chroma .gd { color: var(--tone-invalid); }
"#;
