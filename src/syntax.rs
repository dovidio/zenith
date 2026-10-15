//! Parsing and tone assignment.
//!
//! We use real Sublime grammars (via syntect) because a reader should see the file
//! the way a parser sees it, not the way a regex sees it. But we deliberately throw
//! away syntect's *styles* and keep only its *scopes*: every scope is mapped onto one
//! of zenith's fifteen [`Tone`]s. No editor theme can leak in here.

use std::path::Path;

use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

use crate::theme::Tone;

/// Hard limits. A reader should open anything instantly, so absurd files are
/// truncated rather than left to stall the frame loop.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_LINES: usize = 60_000;

/// One run of text sharing a tone. Text never contains the line's own indentation.
#[derive(Clone, Debug)]
pub struct Token {
    pub text: String,
    pub tone: Tone,
}

/// How a line participates in block structure — this is what the *whisper gutter*
/// draws (structural hairlines) and what the blank-line compressor keys off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// Whitespace only.
    Blank,
    /// Ends by opening a block: `{`, `(`, `:`, `do`, `then`, `=>`.
    Opens,
    /// Begins by closing a block: `}`, `)`, `]`, `end`, `fi`.
    Closes,
    /// A closing line that also opens (`} else {`).
    Both,
    /// Neither.
    Line,
}

/// A fully parsed source line.
#[derive(Clone, Debug)]
pub struct Line {
    /// Indentation width in columns (tab = 4).
    pub indent: usize,
    /// Indentation width of the *first* row only, for hanging structures.
    pub tokens: Vec<Token>,
    pub flow: Flow,
    /// The most semantically weighty tone on the line.
    pub dominant: Tone,
}

impl Line {
    /// A line whose text is known but whose tones are not — yet.
    ///
    /// Reading a large file happens in two passes: the first screenful is toned
    /// immediately, and the rest of the file arrives as lines like this, so that search,
    /// motions, wrapping and scrolling all work at once while the tones are still being
    /// worked out in the background. This is why lowering a file of a few megabytes no
    /// longer stops the window.
    pub fn plain(text: &str) -> Line {
        finish_line(
            text,
            vec![Token {
                text: text.to_string(),
                tone: Tone::Plain,
            }],
        )
    }

    pub fn is_blank(&self) -> bool {
        self.flow == Flow::Blank
    }

    /// Plain text of the line, indentation stripped (for copy/search).
    pub fn text(&self) -> String {
        let mut s = String::new();
        for t in &self.tokens {
            s.push_str(&t.text);
        }
        s
    }
}

/// A parsed file.
#[derive(Clone, Debug)]
pub struct Document {
    pub syntax: String,
    pub lines: Vec<Line>,
    /// For a Markdown file, a rendering of those lines: the same lines in the same order,
    /// with the markers taken out and what each run *is* recorded. The lines above stay
    /// exactly as the file wrote them, which is what search, motions and copying work on.
    pub md: Option<Vec<crate::markdown::MdLine>>,
    /// True if we stopped early because of [`MAX_BYTES`]/[`MAX_LINES`].
    pub truncated: bool,
    /// True if the bytes were not valid UTF-8 and were decoded lossily.
    pub lossy: bool,
    pub bytes: usize,
}

impl Document {
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

pub struct Highlighter {
    ps: SyntaxSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            ps: SyntaxSet::load_defaults_newlines(),
        }
    }

    /// Pick a grammar from the extension, then from the first line (`#!`, `<?php`,
    /// `<!DOCTYPE`, shebangs), then fall back to Plain Text.
    pub fn syntax_for(&self, path: &Path, head: &str) -> &SyntaxReference {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(s) = self.ps.find_syntax_by_extension(ext) {
                return s;
            }
            // syntect's bundled set has no TypeScript, no Kotlin, no Svelte. Those are
            // set with the grammar of the language they grew out of rather than with
            // plain text: the tones will be imperfect, but a `// comment`, a string and
            // a number will still read as themselves.
            if let Some(alias) = grammar_alias(ext) {
                if let Some(s) = self.ps.find_syntax_by_extension(alias) {
                    return s;
                }
            }
        }
        // Files like `Dockerfile`, `Makefile`, `Gemfile` are named, not extensioned.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(s) = self.ps.find_syntax_by_extension(name) {
                return s;
            }
            let lower = name.to_ascii_lowercase();
            if let Some(s) = self.ps.find_syntax_by_extension(&lower) {
                return s;
            }
        }
        if let Some(s) = self.ps.find_syntax_by_first_line(head) {
            return s;
        }
        self.ps.find_syntax_plain_text()
    }

    /// Parse a whole file in one pass. Multi-line constructs (block comments, raw
    /// strings, heredocs) therefore highlight correctly — which per-line regex
    /// highlighting never gets right.
    pub fn parse(&self, syntax: &SyntaxReference, text: &str, bytes: usize) -> Document {
        // syntect's default set has no TOML and no INI, and those are the two most
        // common configuration formats in any repository (Cargo.toml is usually the
        // first file a reader opens). Rather than show them as plain text, zenith
        // sets them with a small grammar of its own.
        if syntax.name == "Plain Text" {
            if let Some(doc) = parse_config(text, bytes) {
                return doc;
            }
        }
        let t_runs = std::time::Instant::now();
        let (runs, truncated) = self.runs(syntax, text, MAX_LINES, false);
        let runs_ms = t_runs.elapsed();
        let t_finish = std::time::Instant::now();
        let mut lines: Vec<Line> = runs
            .into_iter()
            .map(|line| {
                let joined: String = line.iter().map(|r| r.text.as_str()).collect();
                let toks: Vec<Token> = line
                    .into_iter()
                    .map(|r| Token {
                        text: r.text,
                        tone: r.tone,
                    })
                    .collect();
                finish_line(&joined, toks)
            })
            .collect();

        if std::env::var("ZENITH_TIMING").is_ok() {
            eprintln!(
                "timing:   runs (syntect) {:.0?}, finish_line {} lines {:.0?}",
                runs_ms,
                lines.len(),
                t_finish.elapsed()
            );
        }

        if lines.is_empty() {
            lines.push(Line {
                indent: 0,
                tokens: Vec::new(),
                flow: Flow::Blank,
                dominant: Tone::Plain,
            });
        }

        Document {
            syntax: syntax.name.clone(),
            lines,
            md: None,
            truncated,
            lossy: false,
            bytes,
        }
    }

    /// The one parsing pass every consumer shares. When `keep_scopes` is set, each
    /// run also carries the scope stack it was parsed under (used by `--scopes`).
    fn runs(
        &self,
        syntax: &SyntaxReference,
        text: &str,
        max_lines: usize,
        keep_scopes: bool,
    ) -> (Vec<Vec<Run>>, bool) {
        let mut state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let mut out: Vec<Vec<Run>> = Vec::new();
        let mut truncated = false;

        for (idx, raw) in text.split_inclusive('\n').enumerate() {
            if idx >= max_lines {
                truncated = true;
                break;
            }
            let line = raw.strip_suffix('\n').unwrap_or(raw);
            let line = line.strip_suffix('\r').unwrap_or(line);

            let mut runs: Vec<Run> = Vec::new();
            let ops = state.parse_line(raw, &self.ps).unwrap_or_default();
            let mut prev = 0usize;
            let push = |slice: &str, stack: &ScopeStack, runs: &mut Vec<Run>| {
                if slice.is_empty() {
                    return;
                }
                let tone = tone_for_stack(stack, slice);
                if let Some(last) = runs.last_mut() {
                    if last.tone == tone {
                        last.text.push_str(slice);
                        return;
                    }
                }
                runs.push(Run {
                    text: slice.to_string(),
                    tone,
                    scopes: if keep_scopes {
                        stack.as_slice().iter().map(|s| s.to_string()).collect()
                    } else {
                        Vec::new()
                    },
                });
            };
            // `parse_line` was given the line *with* its newline, so an op that
            // closes a scope at end-of-line can point one past the text we draw.
            // Clamping is what keeps `// this comment` from losing its comment:
            // without it the trailing op swallows the rest of the line.
            for (offset, op) in ops {
                let offset = offset.min(line.len());
                if offset > prev {
                    push(&line[prev..offset], &stack, &mut runs);
                    prev = offset;
                }
                let _ = stack.apply(&op);
            }
            if prev < line.len() {
                push(&line[prev..], &stack, &mut runs);
            }
            out.push(runs);
        }

        (out, truncated)
    }

    /// Every grammar zenith can set a file in, with the extensions that select it.
    pub fn grammars(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = self
            .ps
            .syntaxes()
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    s.file_extensions.iter().map(|e| e.to_lowercase()).collect(),
                )
            })
            .collect();
        out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        out
    }

    /// `zenith --scopes FILE [N]`: show the scope stack behind every tone, so the
    /// mapping in [`tone_for_scope`] can be checked against reality rather than faith.
    pub fn dump_scopes(
        &self,
        syntax: &SyntaxReference,
        text: &str,
        max_lines: usize,
    ) -> Vec<Vec<Run>> {
        self.runs(syntax, text, max_lines, true).0
    }
}

/// One run of text with the tone it was assigned (and, for debugging, its scopes).
#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub tone: Tone,
    pub scopes: Vec<String>,
}

/// Strip the line's indentation out of the tokens, measure it, and classify flow.
fn finish_line(line: &str, mut toks: Vec<Token>) -> Line {
    let mut indent = 0usize;
    let mut stripping = true;
    let mut out: Vec<Token> = Vec::with_capacity(toks.len());

    for tok in toks.drain(..) {
        if !stripping {
            out.push(tok);
            continue;
        }
        let mut rest = tok.text.as_str();
        for ch in tok.text.chars() {
            if ch == ' ' {
                indent += 1;
            } else if ch == '\t' {
                indent += 4 - (indent % 4);
            } else {
                break;
            }
        }
        // Recompute the slice boundary in bytes.
        let mut cut = 0usize;
        for ch in rest.chars() {
            if ch == ' ' || ch == '\t' {
                cut += ch.len_utf8();
            } else {
                break;
            }
            let _ = ch;
        }
        rest = &rest[cut..];
        if rest.is_empty() {
            continue;
        }
        stripping = false;
        out.push(Token {
            text: rest.to_string(),
            tone: tok.tone,
        });
    }

    let blank = out.is_empty();
    let flow = if blank {
        Flow::Blank
    } else {
        classify_flow(&out)
    };

    let mut dominant = Tone::Plain;
    let mut best = 0u8;
    for t in &out {
        let w = weight(t.tone);
        if w > best {
            best = w;
            dominant = t.tone;
        }
    }

    let _ = line;
    Line {
        indent,
        tokens: out,
        flow,
        dominant,
    }
}

/// Semantic weight, used to pick the tone that "names" a line in the gutter.
fn weight(t: Tone) -> u8 {
    match t {
        Tone::Keyword => 9,
        Tone::Meta => 8,
        Tone::KeywordSoft => 7,
        Tone::Type => 6,
        Tone::Call => 5,
        Tone::Constant => 4,
        Tone::Str | Tone::Number => 3,
        Tone::Doc => 2,
        Tone::Comment => 1,
        _ => 0,
    }
}

fn classify_flow(toks: &[Token]) -> Flow {
    // Prose cannot open a block, even when it ends in a colon.
    if toks.iter().all(|t| t.tone.is_prose()) {
        return Flow::Line;
    }
    let text: String = toks.iter().map(|t| t.text.as_str()).collect();
    let trimmed = text.trim_end();
    let first = trimmed.chars().next().unwrap_or(' ');
    let last = trimmed.chars().last().unwrap_or(' ');

    let opens_tail = {
        let tail = trimmed.trim_end_matches([';', ',', ' ']);
        tail.ends_with('{')
            || tail.ends_with('(')
            || tail.ends_with("=>")
            || tail.ends_with(" do")
            || tail.ends_with(" then")
            || (tail.ends_with(':') && !tail.ends_with("::"))
            || tail.ends_with("begin")
    };
    let closes_head = matches!(first, '}' | ')' | ']')
        || trimmed.starts_with("end")
        || trimmed.starts_with("fi")
        || trimmed.starts_with("else")
        || trimmed.starts_with("elif")
        || trimmed.starts_with("catch")
        || trimmed.starts_with("finally");
    // `} else {` closes and opens on the same line.
    let both = closes_head
        && (trimmed.contains('{') || trimmed.contains(" do") || trimmed.ends_with(':'));

    if both {
        Flow::Both
    } else if closes_head {
        Flow::Closes
    } else if opens_tail || matches!(last, '{' | '(') {
        Flow::Opens
    } else {
        Flow::Line
    }
}

// ---------------------------------------------------------------------------
// Scope -> tone
// ---------------------------------------------------------------------------

/// Walk the scope stack from the innermost scope outward and take the first rule
/// that speaks. Outermost scopes are structural noise (`source.rust`, `meta.block`)
/// and are skipped so they cannot mask an inner decision.
///
/// Grammars disagree about how much they tell us — several scope primitives as
/// `storage.type`, exactly like `struct` — so the text of the run gets a say too.
/// `str` is a type; `struct` is a keyword; `HashMap` is a type because it is set in
/// capitals. That last rule is a heuristic, and it is the reader's friend.
pub fn tone_for_stack(stack: &ScopeStack, text: &str) -> Tone {
    // Punctuation is the weakest signal on the stack: `//` is
    // `punctuation.definition.comment`, and it must not hide the fact that the line
    // is a comment. So we keep looking outward and only fall back to punctuation if
    // nothing more meaningful encloses it.
    let mut weak: Option<Tone> = None;
    for scope in stack.as_slice().iter().rev() {
        let s = scope.to_string();
        match tone_for_scope(&s, text) {
            Some(Tone::Punct) => weak = Some(Tone::Punct),
            Some(tone) => return tone,
            None => {}
        }
    }
    if weak.is_none() && looks_like_type_name(text) {
        return Tone::Type;
    }
    weak.unwrap_or(Tone::Plain)
}

/// `HashMap`, `Probe`, `Ok` — capitalised words that the grammar left unclassified are
/// types, not identifiers. This is what makes a reader see shapes in unfamiliar code.
fn looks_like_type_name(text: &str) -> bool {
    let t = text.trim();
    if t.len() < 2 {
        return false;
    }
    let mut chars = t.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    // All-caps is a constant, which the grammar usually already told us.
    if t.chars().all(|c| !c.is_ascii_lowercase()) {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_') && t.chars().next().is_some()
}

/// Primitive type names, which several grammars scope as plain `storage.type`.
const PRIMITIVES: &[&str] = &[
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64", "Self", "int", "long", "short", "float", "double", "void",
    "string", "byte", "uint",
];

fn is_primitive(text: &str) -> bool {
    PRIMITIVES.contains(&text.trim())
}

/// The mapping table. Order matters: earlier rules win.
pub fn tone_for_scope(scope: &str, text: &str) -> Option<Tone> {
    // Prose first: comments must never be captureable by a later "constant" rule.
    if scope.starts_with("comment") {
        return Some(if scope.contains("documentation") {
            Tone::Doc
        } else {
            Tone::Comment
        });
    }
    if scope.starts_with("invalid") {
        return Some(Tone::Invalid);
    }
    // A fenced or indented code block inside a prose document.
    if scope.starts_with("markup.raw") {
        return Some(Tone::Raw);
    }
    if scope.starts_with("string") {
        return Some(Tone::Str);
    }
    if scope.starts_with("constant.numeric") {
        return Some(Tone::Number);
    }
    if scope.starts_with("constant.language") {
        return Some(Tone::Constant);
    }
    if scope.starts_with("constant") {
        return Some(Tone::Constant);
    }
    // Punctuation is decided early, and weakly: `punctuation.separator.annotation`
    // is a colon, not an annotation, but `#[derive]` must still come out as `Meta`
    // (its enclosing `meta.annotation` scope outranks this weak signal).
    if scope.starts_with("punctuation") {
        return Some(Tone::Punct);
    }
    // Macros, decorators and attributes get their own voice. The names are spelled
    // out rather than matched loosely on "annotation": Python scopes type annotations
    // as `meta.function.annotation`, and an annotation is not an annotation.
    let is_meta = scope.contains("macro")
        || scope.starts_with("meta.annotation")
        || scope.starts_with("meta.attribute")
        || scope.starts_with("meta.declaration.annotation")
        || scope.starts_with("storage.type.annotation")
        || scope.starts_with("entity.other.attribute")
        || scope.contains("decorator");
    if is_meta {
        return Some(Tone::Meta);
    }
    if scope.contains("arrow") {
        return Some(Tone::Operator);
    }
    if scope.starts_with("keyword.operator") {
        return Some(Tone::Operator);
    }
    if scope.starts_with("keyword") {
        return Some(Tone::Keyword);
    }
    // Primitive types are types; declaration keywords (`fn`, `struct`, `class`) are
    // the darkest ink on the page; modifiers merely qualify them.
    if scope.starts_with("storage.type.primitive") || scope.starts_with("storage.type.built-in") {
        return Some(Tone::Type);
    }
    if scope.starts_with("storage.modifier") {
        return Some(Tone::KeywordSoft);
    }
    if scope.starts_with("storage.type") {
        return Some(if is_primitive(text) {
            Tone::Type
        } else {
            Tone::Keyword
        });
    }
    if scope.starts_with("storage") {
        return Some(Tone::KeywordSoft);
    }
    if scope.starts_with("entity.name.function")
        || scope.starts_with("entity.name.method")
        || scope.starts_with("support.function")
        || scope.starts_with("variable.function")
        || scope.starts_with("entity.name.tag")
    {
        return Some(Tone::Call);
    }
    if scope.starts_with("entity.name.type")
        || scope.starts_with("entity.name.class")
        || scope.starts_with("entity.name.struct")
        || scope.starts_with("entity.name.enum")
        || scope.starts_with("entity.name.trait")
        || scope.starts_with("entity.name.interface")
        || scope.starts_with("entity.name.namespace")
        || scope.starts_with("support.type")
        || scope.starts_with("support.class")
    {
        return Some(Tone::Type);
    }
    // Headings are what a reader navigates by: give them the darkest ink.
    if scope.starts_with("entity.name.section") {
        return Some(Tone::Keyword);
    }
    if scope.starts_with("entity.name") {
        return Some(Tone::Type);
    }
    // Structural containers: say nothing so an inner scope can speak instead.
    None
}


/// The grammar to borrow for an extension the syntax set does not know.
fn grammar_alias(ext: &str) -> Option<&'static str> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "ts" | "tsx" | "mts" | "cts" | "jsx" | "mjs" | "cjs" => "js",
        "svelte" | "vue" | "astro" => "html",
        "kt" | "kts" => "java",
        "scss" | "sass" | "less" => "css",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// A built-in grammar for key/value configuration files
// ---------------------------------------------------------------------------

/// Tokenise a `key = value` configuration file (TOML, INI, `.env`, `*.cfg`).
///
/// Deliberately shallow — sections, keys, operators, comments, and the three kinds of
/// literal a config file can hold. A reader needs to see the *shape* of a config at a
/// glance, and the tone ladder does the rest.
fn parse_config(text: &str, bytes: usize) -> Option<Document> {
    let stripped = text.trim_start();
    if stripped.is_empty() {
        return None;
    }
    // Only claim files that really are key/value config. Two tests, both strict: no line
    // may contain the punctuation of a programming language (outside quotes), and almost
    // every line must have the shape of a config entry. A file set in the wrong grammar
    // reads worse than a file set in none, so the gate is deliberately narrow.
    let mut shaped = 0usize;
    let mut total = 0usize;
    for line in stripped.lines().take(400) {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        total += 1;
        if contains_code_punctuation(t) {
            return None;
        }
        if config_shaped(t) {
            shaped += 1;
        }
    }
    if total == 0 || shaped * 10 < total * 9 {
        return None;
    }

    let lines: Vec<Line> = text
        .split_inclusive('\n')
        .take(MAX_LINES)
        .map(|raw| {
            let line = raw.strip_suffix('\n').unwrap_or(raw);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let toks = config_tokens(line);
            finish_line(line, toks)
        })
        .collect();

    Some(Document {
        md: None,
        syntax: "Config (zenith)".to_string(),
        lines: if lines.is_empty() {
            vec![Line {
                indent: 0,
                tokens: Vec::new(),
                flow: Flow::Blank,
                dominant: Tone::Plain,
            }]
        } else {
            lines
        },
        truncated: text.lines().count() > MAX_LINES,
        lossy: false,
        bytes,
    })
}

/// `{`, `;`, `(`, `=>`, `::`, `->` outside of quotes: the fingerprints of code.
fn contains_code_punctuation(line: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' | '}' | ';' | '(' | ')' => return true,
            _ => {}
        }
    }
    let outside = strip_quoted(line);
    outside.contains("=>") || outside.contains("::") || outside.contains("->")
}

fn strip_quoted(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// Does this line look like a config entry?
fn config_shaped(t: &str) -> bool {
    if t.starts_with('[') && t.ends_with(']') {
        return true; // a TOML/INI section
    }
    let Some(sep) = t.find(['=', ':']) else {
        // A bare word is a valid YAML-ish key or a lone value in an array.
        return t
            .chars()
            .all(|c| c.is_alphanumeric() || "_-.,\"' ".contains(c));
    };
    let key = t[..sep].trim().trim_matches(['"', '\'']);
    !key.is_empty()
        && key.len() < 80
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || "_-.,\"' ".contains(c))
}

fn config_tokens(line: &str) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let push = |text: &str, tone: Tone, out: &mut Vec<Token>| {
        if text.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut() {
            if last.tone == tone {
                last.text.push_str(text);
                return;
            }
        }
        out.push(Token {
            text: text.to_string(),
            tone,
        });
    };

    let trimmed = line.trim_start();
    let indent: String = line[..line.len() - trimmed.len()].to_string();
    if !indent.is_empty() {
        push(&indent, Tone::Plain, &mut out);
    }
    if trimmed.is_empty() {
        return out;
    }
    // Comment.
    if trimmed.starts_with('#') || trimmed.starts_with(';') {
        push(trimmed, Tone::Comment, &mut out);
        return out;
    }
    // Section header.
    if trimmed.starts_with('[') {
        push(trimmed, Tone::Meta, &mut out);
        return out;
    }

    // Key, then separator, then value. A line with no separator is a value in a
    // multi-line array, not a key.
    let Some(sep) = trimmed.find(['=', ':']) else {
        config_value(trimmed, &push, &mut out);
        return out;
    };
    let key = trimmed[..sep].trim_end();
    push(key, Tone::KeywordSoft, &mut out);
    let gap = &trimmed[key.len()..sep];
    if !gap.is_empty() {
        push(gap, Tone::Plain, &mut out);
    }
    push(&trimmed[sep..sep + 1], Tone::Operator, &mut out);

    config_value(&trimmed[sep + 1..], &push, &mut out);
    out
}

/// The value half of a config line, and also whole lines that are values.
fn config_value(
    mut rest: &str,
    push: &impl Fn(&str, Tone, &mut Vec<Token>),
    out: &mut Vec<Token>,
) {
    while !rest.is_empty() {
        let c = rest.chars().next().unwrap();
        if c == '#' {
            push(rest, Tone::Comment, out);
            break;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            let mut end = rest.len();
            let mut escaped = false;
            for (i, ch) in rest.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                } else if ch == quote {
                    end = i + ch.len_utf8();
                    break;
                }
            }
            let (head, tail) = rest.split_at(end);
            push(head, Tone::Str, out);
            rest = tail;
            continue;
        }
        if c.is_ascii_digit() || ((c == '-' || c == '+') && rest.len() > 1) {
            let len: usize = rest
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | '_' | 'e' | 'E' | 'x'))
                .map(|ch| ch.len_utf8())
                .sum();
            if len > 0 && rest[..len].chars().any(|ch| ch.is_ascii_digit()) {
                let (head, tail) = rest.split_at(len);
                push(head, Tone::Number, out);
                rest = tail;
                continue;
            }
        }
        if c.is_alphabetic() || c == '_' {
            let len: usize = rest
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '-')
                .map(|ch| ch.len_utf8())
                .sum();
            let (head, tail) = rest.split_at(len);
            let tone = match head {
                "true" | "false" | "yes" | "no" | "on" | "off" | "null" => Tone::Constant,
                _ => Tone::Plain,
            };
            push(head, tone, out);
            rest = tail;
            continue;
        }
        // Anything else: a bracket, comma, space, or stray mark.
        let len = c.len_utf8();
        let tone = if c.is_whitespace() {
            Tone::Plain
        } else {
            Tone::Punct
        };
        push(&rest[..len], tone, out);
        rest = &rest[len..];
    }
}

// ---------------------------------------------------------------------------
// Reading bytes
// ---------------------------------------------------------------------------

/// A file only enters the reading room if it is text and of sane size.
pub enum Readable {
    Text { text: String, bytes: usize, lossy: bool, truncated: bool },
    Binary { bytes: usize, reason: &'static str },
}

/// Byte offset just past the first `n` lines of `text`, always on a line boundary.
///
/// Used to tone the first screenful of a large file immediately, while the rest of it is
/// toned in the background.
pub fn prefix_end(text: &str, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            seen += 1;
            if seen >= n {
                return i + 1;
            }
        }
    }
    text.len()
}

pub fn read_readable(path: &Path) -> std::io::Result<Readable> {
    let data = std::fs::read(path)?;
    let bytes = data.len();
    if bytes > MAX_BYTES {
        let head = &data[..MAX_BYTES];
        if looks_binary(head) {
            return Ok(Readable::Binary {
                bytes,
                reason: "binary file",
            });
        }
        let mut text = String::from_utf8_lossy(head).into_owned();
        if let Some(idx) = text.rfind('\n') {
            text.truncate(idx);
        }
        return Ok(Readable::Text {
            text,
            bytes,
            lossy: false,
            truncated: true,
        });
    }
    if looks_binary(&data) {
        return Ok(Readable::Binary {
            bytes,
            reason: "binary file",
        });
    }
    match String::from_utf8(data) {
        Ok(text) => Ok(Readable::Text {
            text,
            bytes,
            lossy: false,
            truncated: false,
        }),
        Err(e) => {
            let text = String::from_utf8_lossy(e.as_bytes()).into_owned();
            Ok(Readable::Text {
                text,
                bytes,
                lossy: true,
                truncated: false,
            })
        }
    }
}

fn looks_binary(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(8192)];
    if sample.contains(&0u8) {
        return true;
    }
    let mut weird = 0usize;
    for &b in sample {
        if b < 0x09 || (b > 0x0d && b < 0x20) {
            weird += 1;
        }
    }
    weird * 100 / sample.len().max(1) > 12
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntect::parsing::Scope;

    fn stack(scopes: &[&str]) -> ScopeStack {
        let mut s = ScopeStack::new();
        for sc in scopes {
            if let Ok(parsed) = Scope::new(sc) {
                s.push(parsed);
            }
        }
        s
    }

    #[test]
    fn prose_wins_over_its_own_punctuation() {
        // `//` is scoped as punctuation *inside* the comment scope. Reading only the
        // innermost scope would set a comment as punctuation — which is exactly the
        // bug this rule exists to prevent.
        let s = stack(&[
            "source.rust",
            "comment.line.documentation.rust",
            "punctuation.definition.comment.rust",
        ]);
        assert_eq!(tone_for_stack(&s, "///"), Tone::Doc);
        let s = stack(&[
            "source.rust",
            "comment.line.double-slash.rust",
            "punctuation.definition.comment.rust",
        ]);
        assert_eq!(tone_for_stack(&s, "//"), Tone::Comment);
    }

    #[test]
    fn declaration_keywords_are_darker_than_primitives() {
        assert_eq!(tone_for_scope("storage.type.struct.rust", "struct"), Some(Tone::Keyword));
        assert_eq!(tone_for_scope("storage.type.rust", "str"), Some(Tone::Type));
        // A grammar that scopes a capitalised name as `storage.type` still gets a
        // keyword; the capitalisation heuristic only rescues unclassified runs.
        assert_eq!(tone_for_scope("storage.type.rust", "HashMap"), Some(Tone::Keyword));
    }

    #[test]
    fn attributes_beat_their_brackets() {
        let s = stack(&[
            "source.rust",
            "meta.annotation.rust",
            "punctuation.definition.annotation.rust",
        ]);
        assert_eq!(tone_for_stack(&s, "#["), Tone::Meta);
    }

    #[test]
    fn annotation_colons_are_punctuation_not_annotations() {
        assert_eq!(
            tone_for_scope("punctuation.separator.annotation.python", ":"),
            Some(Tone::Punct)
        );
    }

    #[test]
    fn capitalised_words_are_types() {
        assert_eq!(tone_for_stack(&stack(&["source.rust"]), "HashMap"), Tone::Type);
        assert_eq!(tone_for_stack(&stack(&["source.rust"]), "name"), Tone::Plain);
        assert_eq!(tone_for_stack(&stack(&["source.rust"]), "MAX_LIMIT"), Tone::Plain);
    }

}

#[cfg(test)]
mod loader_tests {
    use super::*;

    #[test]
    fn plain_lines_keep_their_text_and_lose_their_indentation() {
        // Reading a large file shows these before the tones arrive, so search, motions
        // and wrapping have to work on them exactly as on a toned line.
        let l = Line::plain("        let total = 1;");
        assert_eq!(l.text(), "let total = 1;");
        assert_eq!(l.indent, 8);
        assert!(!l.is_blank());
        assert!(Line::plain("   ").is_blank());
    }

    #[test]
    fn prefix_end_stops_on_a_line_boundary() {
        let text = "one\ntwo\nthree\nfour\n";
        assert_eq!(prefix_end(text, 2), 8); // "one\ntwo\n"
        assert!(&text[..prefix_end(text, 2)].ends_with('\n'));
        assert_eq!(prefix_end(text, 99), text.len());
        assert_eq!(prefix_end(text, 0), 0);
    }
}
