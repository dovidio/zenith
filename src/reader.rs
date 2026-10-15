//! The typographic layout engine.
//!
//! This is where zenith stops being an editor. Five decisions, all of them
//! deliberate:
//!
//! 1. **Indentation is not whitespace.** Leading whitespace is stripped out of the
//!    text and drawn as *whisper hairlines* in a gutter, one per nesting level,
//!    fading with depth. You feel the structure of a block instead of counting
//!    spaces.
//! 2. **Comments are prose.** They are set in a serif face, italic, in the faintest
//!    ink on the page — the same way a book sets an aside — while the `//` or `#`
//!    mark is left behind in faint mono as a marginal sign.
//! 3. **Blank lines are compressed.** A blank line costs 46% of a line, so a file's
//!    paragraph rhythm survives without the page turning into a corridor.
//! 4. **Long lines wrap and declare themselves.** A wrapped row is marked in the
//!    gutter with a continuation mark, so you are never lost mid-expression, and you
//!    never scroll sideways.
//! 5. **Attention is a gradient.** Dimming is painted as a scrim over the *page*, not
//!    as a colour change in the text, so the galley cache survives and the fade is
//!    literally the paper coming forward.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{
    Align, Align2, Color32, Context, CornerRadius, FontId, Galley, Painter, Pos2, Rangef, Rect,
    Stroke, TextFormat, vec2,
};

use crate::syntax::{Document, Line};
use crate::theme::{Palette, Tone, alpha, mix};

/// Indentation beyond this many columns is not drawn as guides (deep enough).
const INDENT_CAP: usize = 28;
/// Nesting levels that get a hairline.
const MAX_GUIDES: usize = 8;
/// How much of a row height a blank line costs.
const BLANK_RATIO: f32 = 0.46;
/// Extra air given to doc comments (they are addressed to a reader).
const DOC_AIR: f32 = 1.18;

#[derive(Clone)]
pub struct Style {
    pub code: FontId,
    pub code_italic: FontId,
    pub prose: FontId,
    pub prose_italic: FontId,
    /// The interface voice, used for colophons and metadata.
    pub ui: FontId,
    /// Multiple of font size per line. Generous: this is a reading room.
    pub leading: f32,
    /// Reading measure in points.
    pub measure: f32,
    pub wrap: bool,
    pub guides: bool,
    pub compress_blank: bool,
    pub line_numbers: bool,
    /// Set comments as prose (serif) rather than as code.
    pub serif_comments: bool,
    /// The whole file is prose (a Markdown document, say). Body text and headings are
    /// then set in the reading face, while code blocks stay monospace.
    pub prose_document: bool,
    /// Render a Markdown document rather than setting its source: markers removed,
    /// headings sized, bullets drawn, code fenced off.
    pub markdown: bool,
    /// Recede what a human is not reading in a strongly typed language: type
    /// annotations, modifiers, and the punctuation only the compiler needs.
    pub distilled: bool,
    /// How this language writes its types, which is what [`crate::distill`] needs.
    pub lang: crate::distill::Lang,
    /// The margin mark for a wrapped line, if this machine can draw it.
    pub wrap_mark: &'static str,

    pub tones: [Color32; Tone::COUNT],
    pub ink_faint: Color32,
    pub guide_ink: Color32,
    pub paper: Color32,
    pub accent: Color32,
}

impl Style {
    pub fn row_h(&self) -> f32 {
        self.code.size * self.leading
    }

    /// Hash of everything that changes layout or ink. The app re-lays out when the
    /// key changes, and otherwise reuses the cached jobs every frame.
    ///
    /// Note that the column position is *not* part of the key: changing it only moves
    /// the page, it does not re-typeset the text.
    pub fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.code.size.to_bits().hash(&mut h);
        self.leading.to_bits().hash(&mut h);
        self.measure.to_bits().hash(&mut h);
        self.wrap.hash(&mut h);
        self.guides.hash(&mut h);
        self.compress_blank.hash(&mut h);
        self.line_numbers.hash(&mut h);
        self.serif_comments.hash(&mut h);
        self.prose_document.hash(&mut h);
        self.markdown.hash(&mut h);
        self.distilled.hash(&mut h);
        (self.lang as u8).hash(&mut h);
        self.wrap_mark.hash(&mut h);
        for t in self.tones {
            t.to_array().hash(&mut h);
        }
        self.paper.to_array().hash(&mut h);
        h.finish()
    }

    /// Which face does this tone speak in?
    pub fn font_for(&self, tone: Tone) -> FontId {
        if self.serif_comments {
            match tone {
                Tone::Comment => self.prose_italic.clone(),
                Tone::Doc => self.prose.clone(),
                // In a prose document, the body and its headings are prose too.
                Tone::Plain | Tone::Keyword if self.prose_document => self.prose.clone(),
                _ => self.code.clone(),
            }
        } else {
            match tone {
                Tone::Comment | Tone::Doc => self.code_italic.clone(),
                _ => self.code.clone(),
            }
        }
    }

    pub fn color_for(&self, tone: Tone) -> Color32 {
        self.tones[tone.index()]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GuideRun {
    pub level: usize,
    pub first: usize,
    pub last: usize,
}

/// A typeset document: one layout *job* per source line, plus its place on the page.
///
/// Note what is cached here and what is not. We keep the jobs — the text and its
/// formats — and we re-lay them out every frame for the lines that are actually
/// visible. That is not laziness, it is correctness: epaint throws its font atlas away
/// and builds a new one whenever the atlas fills past 80%, and a [`Galley`] held across
/// that rebuild still points at the *old* atlas. The result is glyphs rendered as
/// slivers of other glyphs — intermittently, depending on how full the atlas happens
/// to be. epaint's own galley cache is invalidated correctly on rebuild; ours would
/// not have been.
pub struct Laid {
    /// `None` for blank lines (they are pure rhythm, nothing to typeset).
    pub jobs: Vec<Option<LayoutJob>>,
    /// Content-space y offset of the top of each line.
    pub tops: Vec<f32>,
    /// Height of each line.
    pub heights: Vec<f32>,
    pub guide_runs: Vec<GuideRun>,
    pub total: f32,
    pub col_x: f32,
    pub char_w: f32,
    pub measure: f32,
    pub row_h: f32,
    pub key: u64,
}

impl Laid {
    pub fn line_at(&self, y: f32) -> usize {
        if self.tops.is_empty() {
            return 0;
        }
        match self
            .tops
            .binary_search_by(|t| t.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        }
    }

    pub fn top_of(&self, line: usize) -> f32 {
        self.tops.get(line).copied().unwrap_or(0.0)
    }

    pub fn height_of(&self, line: usize) -> f32 {
        self.heights.get(line).copied().unwrap_or(self.row_h)
    }

    /// Indentation of a line in points, capped so a wrapped line keeps a usable
    /// measure.
    pub fn indent_px(&self, indent_cols: usize) -> f32 {
        (indent_cols.min(INDENT_CAP)) as f32 * self.char_w
    }

    /// Convert a scroll fraction (0..1) into a content-space y.
    pub fn y_for_frac(&self, frac: f32) -> f32 {
        (self.total * frac.clamp(0.0, 1.0)).min((self.total - self.row_h).max(0.0))
    }
}

// ---------------------------------------------------------------------------
// Laying out
// ---------------------------------------------------------------------------

pub fn layout(
    ctx: &Context,
    doc: &Document,
    style: &Style,
    col_x: f32,
    header_h: f32,
) -> Laid {
    let key = style.key();
    let row_h = style.row_h();
    let char_w = ctx.fonts_mut(|f| f.glyph_width(&style.code, 'M')).max(1.0);
    let blank_h = row_h * BLANK_RATIO;

    let mut jobs: Vec<Option<LayoutJob>> = Vec::with_capacity(doc.lines.len());
    let mut tops: Vec<f32> = Vec::with_capacity(doc.lines.len());
    let mut heights: Vec<f32> = Vec::with_capacity(doc.lines.len());

    let mut y = header_h;
    for (index, line) in doc.lines.iter().enumerate() {
        tops.push(y);

        // A rendered blank line is not always a blank source line: a fence is a blank
        // source line with a meaning, and it deserves its own air.
        if line.is_blank() {
            let h = if style.compress_blank { blank_h } else { row_h };
            heights.push(h);
            jobs.push(None);
            y += h;
            continue;
        }

        let indent_px = (line.indent.min(INDENT_CAP)) as f32 * char_w;
        let wrap_w = if style.wrap {
            (style.measure - indent_px).max(style.measure * 0.45)
        } else {
            f32::INFINITY
        };
        let line_row_h = if line.dominant == Tone::Doc {
            row_h * DOC_AIR
        } else {
            row_h
        };

        let md_line = doc.md.as_ref().and_then(|m| m.get(index));
        let job = line_job(style, line, md_line, wrap_w, line_row_h);
        // Measure once to learn how tall the line is when wrapped. The galley itself
        // is thrown away: it will be rebuilt when the line is painted.
        let h = ctx
            .fonts_mut(|f| f.layout_job(job.clone()))
            .size()
            .y
            .max(line_row_h);
        heights.push(h);
        jobs.push(Some(job));
        y += h;
    }

    let guide_runs = if style.guides {
        compute_guides(doc)
    } else {
        Vec::new()
    };

    Laid {
        jobs,
        tops,
        heights,
        guide_runs,
        total: y + row_h * 2.0, // a little air at the end of the page
        col_x,
        char_w,
        measure: style.measure,
        row_h,
        key,
    }
}

/// Build one line's job. Comments are split into their marginal mark and their
/// prose body so the two can be set in different faces and inks.
fn line_job(
    style: &Style,
    line: &Line,
    md: Option<&crate::markdown::MdLine>,
    wrap_w: f32,
    row_h: f32,
) -> LayoutJob {
    let mut job = LayoutJob {
        break_on_newline: false, // one source line per job
        halign: Align::LEFT,
        first_row_min_height: row_h,
        wrap: egui::text::TextWrapping {
            max_width: wrap_w,
            break_anywhere: false,
            ..Default::default()
        },
        ..Default::default()
    };

    // A rendered Markdown line is built from the roles the scanner found, not from the
    // source's tokens: the markers are gone by now.
    if style.markdown {
        if let Some(md) = md {
            return markdown_job(style, md, wrap_w, row_h);
        }
    }

    // Which runs are *metadata* rather than the sentence? See `crate::distill` for
    // how that is decided, per language, and why it refuses more often than it fires.
    let marks = if style.distilled {
        crate::distill::marks(style.lang, line)
    } else {
        Vec::new()
    };

    for (index, tok) in line.tokens.iter().enumerate() {
        let text = if tok.text.contains('\t') {
            tok.text.replace('\t', "    ")
        } else {
            tok.text.clone()
        };

        // In a prose document, emphasis and code markers are marginal signs: the words
        // they wrap are the sentence, the markers themselves are punctuation.
        if style.prose_document && tok.tone == Tone::Plain && text.contains(['*', '_', '`', '~']) {
            for (chunk, is_mark) in prose_marks(&text) {
                let mut f = TextFormat {
                    font_id: style.font_for(Tone::Plain),
                    color: style.color_for(Tone::Plain),
                    line_height: Some(row_h),
                    ..Default::default()
                };
                if is_mark {
                    f.font_id = style.code.clone();
                    f.color = mix(style.color_for(Tone::Plain), style.paper, 0.55);
                }
                job.append(&chunk, 0.0, f);
            }
            continue;
        }

        let mut fmt = TextFormat {
            font_id: style.font_for(tok.tone),
            color: style.color_for(tok.tone),
            line_height: Some(row_h),
            ..Default::default()
        };

        if style.distilled {
            let metadata = mix(style.color_for(Tone::Punct), style.paper, 0.15);
            let is_trailing_semicolon =
                tok.text.trim() == ";" && index + 1 == line.tokens.len();
            if is_trailing_semicolon {
                // The compiler needs it; the reader does not. Kept in place, so the
                // line still wraps exactly as the file does.
                fmt.color = style.paper;
            } else {
                match marks.get(index).cloned().flatten() {
                    Some(crate::distill::Mark::All) => fmt.color = metadata,
                    Some(crate::distill::Mark::Part(start, end)) => {
                        // Part of a run is metadata: the grammar gave `total: number` a
                        // single token, so it has to be cut here.
                        let chars: Vec<char> = text.chars().collect();
                        let (s, e) = (start.min(chars.len()), end.min(chars.len()));
                        let head: String = chars[..s].iter().collect();
                        let middle: String = chars[s..e].iter().collect();
                        let tail: String = chars[e..].iter().collect();
                        let mut faded = fmt.clone();
                        faded.color = metadata;
                        job.append(&head, 0.0, fmt.clone());
                        job.append(&middle, 0.0, faded);
                        job.append(&tail, 0.0, fmt);
                        continue;
                    }
                    None => match tok.tone {
                        Tone::Punct | Tone::Operator => {
                            fmt.color = mix(fmt.color, style.paper, 0.45);
                        }
                        Tone::KeywordSoft => {
                            fmt.color = mix(fmt.color, style.paper, 0.30);
                        }
                        _ => {}
                    },
                }
            }
        }

        if tok.tone.is_prose() {
            // Leave the comment mark behind: it is a marginal sign, not the sentence.
            let (mark, body) = split_comment_mark(&text);
            if !mark.is_empty() {
                let mut mfmt = fmt.clone();
                mfmt.font_id = style.code.clone();
                mfmt.color = mix(style.tones[Tone::Comment.index()], style.paper, 0.25);
                job.append(mark, 0.0, mfmt);
            }
            if body.is_empty() {
                continue;
            }
            if tok.tone == Tone::Doc {
                // A doc comment is an epigraph: upright serif, slightly more ink.
                fmt.color = mix(style.tones[Tone::Doc.index()], style.tones[Tone::Plain.index()], 0.15);
            }
            job.append(body, 0.0, fmt);
            continue;
        }

        job.append(&text, 0.0, fmt);
    }

    job
}


/// One rendered Markdown line: the roles the scanner found, turned into faces and inks.
///
/// The rules worth knowing: a heading is the same serif as the body, larger, in the
/// page's strongest ink, with air above it. Emphasis is the italic face, which the
/// typography already resolves. `strong` has no bold face to use — fonts are chosen for
/// reading, and a faux-bold would be a lie — so it is the body face in the strongest ink
/// with a hair more spacing, which reads as emphasis without pretending to be a weight.
/// Code is the monospace voice at a slightly smaller size on a tinted ground.
fn markdown_job(style: &Style, md: &crate::markdown::MdLine, wrap_w: f32, row_h: f32) -> LayoutJob {
    use crate::markdown::{Kind, Role};

    let level = match md.kind {
        Kind::Heading(n) => n.clamp(1, 6) as usize,
        _ => 0,
    };
    // 1.7, 1.4, 1.22, 1.12, 1.06, 1.02 — a document's ladder, not a UI's.
    const SCALE: [f32; 6] = [1.7, 1.4, 1.22, 1.12, 1.06, 1.02];
    let body_size = style.prose.size;
    let base = if level > 0 {
        body_size * SCALE[level - 1]
    } else {
        body_size
    };
    let line_h = (row_h * base / body_size).max(row_h * 0.9);
    // A heading's own row carries its air, so the page does not need a blank line to
    // separate sections.
    let min_h = if level > 0 { line_h * 1.5 } else { line_h };

    // The faces at this line's size. `style.prose` carries the body size, so a heading
    // needs the same family at a larger one — setting the family alone renders a heading
    // the size of a sentence, which is what happened first.
    let prose = FontId::new(base, style.prose.family.clone());
    let prose_italic = FontId::new(base, style.prose_italic.family.clone());
    let code = FontId::new(base * 0.94, style.code.family.clone());

    let mut job = LayoutJob {
        break_on_newline: false,
        halign: Align::LEFT,
        first_row_min_height: min_h,
        wrap: egui::text::TextWrapping {
            max_width: wrap_w,
            break_anywhere: false,
            ..Default::default()
        },
        ..Default::default()
    };

    // A tint of the page rather than a second colour: code sits *on* the paper.
    let band = mix(style.paper, style.ink_faint, 0.10);
    for span in &md.spans {
        let mut fmt = TextFormat {
            font_id: prose.clone(),
            color: style.color_for(Tone::Plain),
            line_height: Some(line_h),
            ..Default::default()
        };
        match span.role {
            Role::Body => {}
            Role::Strong => {
                fmt.color = style.color_for(Tone::Keyword);
                fmt.extra_letter_spacing = 0.15;
            }
            Role::Emphasis => fmt.font_id = prose_italic.clone(),
            Role::Link => fmt.color = style.accent,
            Role::Code => {
                fmt.font_id = FontId::monospace(base * 0.94);
                fmt.color = style.color_for(Tone::Str);
                fmt.background = band;
            }
            Role::CodeLine => {
                fmt.font_id = code.clone();
                fmt.color = style.color_for(Tone::Plain);
            }
            Role::Marker => {
                fmt.font_id = code.clone();
                fmt.color = style.ink_faint;
            }
        }
        if matches!(md.kind, Kind::Quote) && span.role == Role::Body {
            fmt.font_id = prose_italic.clone();
            fmt.color = style.ink_faint;
        }
        job.append(&span.text, 0.0, fmt);
    }
    if job.sections.is_empty() {
        // Rules and empty lines carry no text; the painter draws them.
        job.append(" ", 0.0, TextFormat {
            font_id: prose.clone(),
            color: style.paper,
            line_height: Some(row_h),
            ..Default::default()
        });
    }
    job
}

/// Split prose into content and markup runs, so `**` can be set as a marginal sign
/// while the words it wraps keep the ink of the sentence.
fn prose_marks(text: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut buf = String::new();
    let mut in_mark = false;
    for c in text.chars() {
        let is_mark = matches!(c, '*' | '_' | '`' | '~');
        if is_mark != in_mark {
            if !buf.is_empty() {
                out.push((std::mem::take(&mut buf), in_mark));
            }
            in_mark = is_mark;
        }
        buf.push(c);
    }
    if !buf.is_empty() {
        out.push((buf, in_mark));
    }
    out
}

/// `"// this is prose"` -> (`"//"`, `" this is prose"`).
///
/// A divisor line made only of punctuation (`//-----`) is returned whole as a mark,
/// because a rule is a rule.
pub fn split_comment_mark(text: &str) -> (&str, &str) {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && i < 12 {
        let c = bytes[i] as char;
        if matches!(c, '/' | '#' | '*' | '-' | ';' | '!' | '{' | '}' | '=' | '\'' | '"') {
            i += 1;
        } else {
            break;
        }
    }
    if i == 0 {
        return ("", text);
    }
    if i >= bytes.len() {
        // Entirely punctuation: a divider.
        return (text, "");
    }
    text.split_at(i)
}

/// Structural hairlines: for each nesting level, the maximal runs of lines that sit
/// *inside* that level. Blank lines inside a run keep it open; trailing blank lines
/// are trimmed, so a guide ends where the block ends.
fn compute_guides(doc: &Document) -> Vec<GuideRun> {
    let mut runs = Vec::new();
    for level in 0..MAX_GUIDES {
        let threshold = (level + 1) * 4;
        let mut open: Option<usize> = None;
        let mut last_code = 0usize;
        for (i, line) in doc.lines.iter().enumerate() {
            if line.is_blank() {
                continue; // blank lines neither open nor close a block
            }
            if line.indent >= threshold {
                if open.is_none() {
                    open = Some(i);
                }
                last_code = i;
            } else if let Some(start) = open.take() {
                if last_code > start {
                    runs.push(GuideRun {
                        level,
                        first: start,
                        last: last_code,
                    });
                }
            }
        }
        if let Some(start) = open {
            if last_code > start {
                runs.push(GuideRun {
                    level,
                    first: start,
                    last: last_code,
                });
            }
        }
    }
    runs
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

pub struct Paint<'a> {
    /// Needed to typeset the visible slice each frame (see [`Laid`]).
    pub ctx: &'a Context,
    pub laid: &'a Laid,
    pub doc: &'a Document,
    pub style: &'a Style,
    pub palette: &'a Palette,
    /// Screen y of content y = 0.
    pub origin_y: f32,
    /// Visible slice of the page, in content coordinates. Everything outside it is
    /// skipped: a reader may open a 60,000-line file, and painting every galley in it
    /// would be absurd.
    pub visible: (f32, f32),
    /// Where the reader is. The line gets the wash and the ribbon; the column gets a
    /// hairline, which is what makes `w`, `b`, `e` and `%` visible.
    pub caret: crate::vim::Caret,
    /// Optional (first, last) inclusive line range to keep at full strength.
    pub focus: Option<(usize, usize)>,
    /// Strength of the fade for unfocused lines, 0..=255.
    pub fade_alpha: u8,
    pub zen: bool,
    /// Include every line, ignoring the focus lens (used by --smoke shots).
    pub show_cursor: bool,
}

pub fn paint(painter: &Painter, p: &Paint) {
    let ox = p.laid.col_x;
    let oy = p.origin_y;
    let t = |y: f32| oy + y;
    let (v_top, v_bottom) = p.visible;
    let first = p.laid.line_at(v_top).saturating_sub(1);
    let last = (p.laid.line_at(v_bottom) + 2).min(p.doc.lines.len());

    // Typeset just the lines that can be seen. epaint caches galleys internally and
    // invalidates that cache when it rebuilds the atlas, so this is both cheap and
    // always correct.
    let visible: Vec<Option<Arc<Galley>>> = (first..last)
        .map(|i| {
            p.laid.jobs[i]
                .as_ref()
                .map(|job| p.ctx.fonts_mut(|f| f.layout_job(job.clone())))
        })
        .collect();
    let galley_at = |i: usize| -> Option<&Arc<Galley>> {
        if i < first || i >= last {
            return None;
        }
        visible[i - first].as_ref()
    };

    // 1. Indentation whispers.
    if p.style.guides {
        for run in &p.laid.guide_runs {
            if p.laid.top_of(run.last) < v_top || p.laid.top_of(run.first) > v_bottom {
                continue;
            }
            let x = ox + (run.level * 4 + 2) as f32 * p.laid.char_w;
            let y0 = t(p.laid.top_of(run.first));
            let y1 = t(p.laid.top_of(run.last) + p.laid.height_of(run.last));
            let depth = run.level as f32 / MAX_GUIDES as f32;
            let ink = mix(p.style.paper, p.style.guide_ink, 0.65 - depth * 0.35);
            painter.vline(x, Rangef::new(y0, y1), Stroke::new(1.0, ink));
        }
    }

    // 2. Marginalia: line numbers, continuation marks, block marks.
    let small = FontId::new(p.style.code.size * 0.78, p.style.code.family.clone());
    for i in first..last {
        let top = t(p.laid.top_of(i));
        let h = p.laid.height_of(i);

        if p.style.line_numbers && !p.doc.lines[i].is_blank() {
            painter.text(
                Pos2::new(ox - p.laid.char_w * 0.9, top),
                Align2::RIGHT_TOP,
                format!("{}", i + 1),
                small.clone(),
                mix(p.style.paper, p.style.ink_faint, 0.85),
            );
        }

        if let Some(galley) = galley_at(i) {
            if galley.rows.len() > 1 {
                for row in galley.rows.iter().skip(1) {
                    painter.text(
                        Pos2::new(ox - p.laid.char_w * 1.15, top + row.pos.y),
                        Align2::LEFT_TOP,
                        p.style.wrap_mark,
                        small.clone(),
                        mix(p.style.paper, p.style.ink_faint, 0.7),
                    );
                }
            }
        }
        let _ = h;
    }

    // 2b. A rendered document has furniture the text cannot draw: the ground behind a
    // fenced block or a quotation, and a rule where the source said `---`.
    if let Some(md) = &p.doc.md {
        if p.style.markdown {
            let band = mix(p.style.paper, p.style.ink_faint, 0.10);
            let mut run: Option<(usize, usize, bool)> = None;
            let flush = |run: (usize, usize, bool), painter: &Painter| {
                let (a, b, quoted) = run;
                // One band per continuous run of code, and one per quotation.
                let y0 = t(p.laid.top_of(a)) - p.laid.row_h * 0.15;
                let y1 = t(p.laid.top_of(b) + p.laid.height_of(b)) + p.laid.row_h * 0.15;
                let rect = Rect::from_min_max(
                    Pos2::new(ox - p.laid.char_w * 1.6, y0),
                    Pos2::new(ox + p.laid.measure, y1),
                );
                painter.rect_filled(rect, CornerRadius::same(3), band);
                if quoted {
                    // A quotation gets a rule down its left edge as well.
                    let edge = Rect::from_min_max(rect.left_top(), Pos2::new(rect.left() + 2.0, rect.bottom()));
                    painter.rect_filled(edge, CornerRadius::ZERO, p.palette.accent_soft);
                }
            };
            for i in first..last {
                let Some(line) = md.get(i) else { continue };
                let quoted = matches!(line.kind, crate::markdown::Kind::Quote);
                let is_band = line.in_band() || quoted;
                match (&mut run, is_band) {
                    (None, true) => run = Some((i, i, quoted)),
                    (Some(r), true) => {
                        if r.2 == quoted {
                            r.1 = i;
                        } else {
                            flush(*r, painter);
                            run = Some((i, i, quoted));
                        }
                    }
                    (Some(_), false) => {
                        flush(run.take().unwrap(), painter);
                    }
                    (None, false) => {}
                }
            }
            if let Some(r) = run {
                flush(r, painter);
            }

            // Rules are a hairline across the measure, not a row of dashes.
            for i in first..last {
                if let Some(line) = md.get(i) {
                    if matches!(line.kind, crate::markdown::Kind::Rule) {
                        let y = t(p.laid.top_of(i) + p.laid.height_of(i) * 0.5);
                        painter.hline(
                            (ox - p.laid.char_w * 1.6)..=(ox + p.laid.measure),
                            y,
                            Stroke::new(1.0, p.palette.edge),
                        );
                    }
                }
            }
        }
    }

    // 3. The text itself.
    for i in first..last {
        let Some(galley) = galley_at(i) else {
            continue;
        };
        let indent_px = p.laid.indent_px(p.doc.lines[i].indent);
        painter.galley(
            Pos2::new(ox + indent_px, t(p.laid.top_of(i))),
            galley.clone(),
            p.style.tones[Tone::Plain.index()],
        );
    }

    // 4. Fades: one scrim per run of unfocused lines, painted over the page.
    if let Some((f0, f1)) = p.focus {
        let scrim = alpha(p.palette.fade, p.fade_alpha);
        let mut run: Option<(usize, usize)> = None;
        let flush = |run: (usize, usize), painter: &Painter| {
            let (a, b) = run;
            let y0 = t(p.laid.top_of(a));
            let y1 = t(p.laid.top_of(b) + p.laid.height_of(b));
            let rect = Rect::from_min_max(
                Pos2::new(ox - p.laid.char_w * 2.0, y0),
                Pos2::new(ox + p.laid.measure, y1),
            );
            painter.rect_filled(rect, CornerRadius::ZERO, scrim);
        };
        for i in first..last {
            let inside = i >= f0 && i <= f1;
            match (&mut run, inside) {
                (None, false) => run = Some((i, i)),
                (Some(r), false) => r.1 = i,
                (Some(r), true) => {
                    flush(*r, painter);
                    run = None;
                }
                (None, true) => {}
            }
        }
        if let Some(r) = run {
            flush(r, painter);
        }
    }

    // 5. The reading cursor: a ribbon in the margin, the faintest wash, and a
    //    hairline at the column.
    let line = p.caret.line;
    if p.show_cursor && line < p.doc.lines.len() {
        let top = t(p.laid.top_of(line));
        let h = p.laid.height_of(line);
        let rect = Rect::from_min_max(
            Pos2::new(ox - p.laid.char_w * 2.0, top),
            Pos2::new(ox + p.laid.measure, top + h),
        );
        painter.rect_filled(rect, CornerRadius::same(2), alpha(p.style.accent, 14));
        if !p.zen {
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(ox - p.laid.char_w * 2.0, top + 2.0),
                    vec2(2.0, (h - 4.0).max(2.0)),
                ),
                CornerRadius::same(1),
                alpha(p.style.accent, 190),
            );
        }
        // The column, when the text is monospaced: a hairline the height of the line.
        // In a prose document the columns are not a grid, so it is not drawn.
        if !p.style.prose_document {
            let indent = p.laid.indent_px(p.doc.lines[line].indent);
            let x = ox + indent + p.caret.col as f32 * p.laid.char_w;
            painter.vline(
                x,
                Rangef::new(top + 1.0, top + h - 1.0),
                Stroke::new(1.0, alpha(p.style.accent, if p.zen { 120 } else { 90 })),
            );
        }
    }
}

/// The title page of a file. Set like the head of a chapter, not like a tab.
#[allow(clippy::too_many_arguments)]
pub fn paint_header(
    painter: &Painter,
    style: &Style,
    palette: &Palette,
    col_x: f32,
    top: f32,
    title: &str,
    path: &str,
    badge: Option<(String, Color32)>,
) {
    let size = style.code.size;
    let title_font = FontId::new(size * 1.85, style.prose.family.clone());
    let path_font = FontId::new(size * 0.95, style.prose_italic.family.clone());
    let badge_font = style.ui.clone();

    let mut y = top + size * 1.4;
    let mut job = LayoutJob::default();
    job.wrap.max_width = style.measure;
    job.append(
        title,
        size * 0.5,
        TextFormat {
            font_id: title_font,
            color: palette.ink,
            line_height: Some(size * 2.3),
            ..Default::default()
        },
    );
    let g = painter.layout_job(job);
    painter.galley(Pos2::new(col_x, y), g.clone(), palette.ink);
    y += g.size().y + size * 0.15;

    if let Some((text, color)) = badge {
        painter.text(
            Pos2::new(col_x + style.measure, top + size * 1.5),
            Align2::RIGHT_TOP,
            text,
            badge_font.clone(),
            color,
        );
    }

    painter.text(
        Pos2::new(col_x + size * 0.5, y),
        Align2::LEFT_TOP,
        path,
        path_font,
        palette.ink_faint,
    );
    y += size * 1.5;

    // A hairline rule closes the title block. No colophon: the reader asked for the
    // page, not the file's vital statistics.
    painter.hline(
        Rangef::new(col_x, col_x + style.measure),
        y,
        Stroke::new(1.0, alpha(palette.edge, 255)),
    );
}

/// Height the header needs for a given style.
pub fn header_height(style: &Style) -> f32 {
    style.code.size * 5.9
}

/// The measure actually available on a page of this width: the reader's preference,
/// clamped so a narrow window still shows whole lines.
pub fn effective_measure(page: Rect, wanted: f32, gutter: f32) -> f32 {
    wanted.min(page.width() - gutter * 2.0).max(240.0)
}

/// Draw the reading-progress spine down the right edge of the page.
pub fn paint_spine(painter: &Painter, x: f32, top: f32, height: f32, palette: &Palette, frac: f32) {
    let y = top + height * frac.clamp(0.0, 1.0);
    painter.vline(
        x,
        Rangef::new(top, top + height),
        Stroke::new(2.0, alpha(palette.edge, 90)),
    );
    painter.vline(
        x,
        Rangef::new((y - height * 0.06).max(top), y),
        Stroke::new(2.0, alpha(palette.accent, 200)),
    );
}

/// Where should the reading column sit inside a page of this width?
pub fn column_x(page: Rect, measure: f32, gutter: f32) -> f32 {
    let left = page.left() + gutter.max((page.width() - measure) * 0.5);
    left.max(page.left() + gutter)
}


#[cfg(test)]
mod tests {
    use super::split_comment_mark;

    #[test]
    fn prose_markers_are_separated_from_content() {
        let runs = super::prose_marks("**bold** and `code`");
        assert_eq!(
            runs,
            vec![
                ("**".to_string(), true),
                ("bold".to_string(), false),
                ("**".to_string(), true),
                (" and ".to_string(), false),
                ("`".to_string(), true),
                ("code".to_string(), false),
                ("`".to_string(), true),
            ]
        );
    }

    #[test]
    fn comment_marks_are_split_from_their_prose() {
        assert_eq!(split_comment_mark("// prose"), ("//", " prose"));
        assert_eq!(split_comment_mark("/// doc"), ("///", " doc"));
        assert_eq!(split_comment_mark("# prose"), ("#", " prose"));
        // A rule made only of punctuation stays whole: it is a divider, not a word.
        assert_eq!(split_comment_mark("//////"), ("//////", ""));
        assert_eq!(split_comment_mark("prose"), ("", "prose"));
    }
}
