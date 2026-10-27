//! Outline extraction — "chapters" for the reading pane.
//!
//! This is *deliberately* heuristic and language-agnostic-ish: it reads the token
//! stream zenith already parsed, skips anything that is prose, and looks for
//! declaration keywords. It is not semantic, and it does not pretend to be — real
//! precision is what a language server is for. What matters today is that
//! a reader can see the shape of a file at a glance and jump by structure.

use std::path::Path;

use crate::syntax::Line;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymKind {
    Function,
    Type,
    Trait,
    Enum,
    Module,
    Constant,
    Macro,
    Heading,
}

impl SymKind {
    /// A single glyph for the margin. No icon fonts, no emoji, just typography — and a
    /// letter, when this machine's fonts cannot draw the mark at all.
    pub fn glyph(self, marks: &crate::typography::Marks) -> &'static str {
        marks.pick(match self {
            SymKind::Function => "ƒ",
            SymKind::Type => "◆",
            SymKind::Trait => "◇",
            SymKind::Enum => "≡",
            SymKind::Module => "❖",
            SymKind::Constant => "○",
            SymKind::Macro => "✦",
            SymKind::Heading => "§",
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            SymKind::Function => "function",
            SymKind::Type => "type",
            SymKind::Trait => "trait / interface",
            SymKind::Enum => "enum",
            SymKind::Module => "module",
            SymKind::Constant => "constant",
            SymKind::Macro => "macro",
            SymKind::Heading => "heading",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub line: usize,
    pub name: String,
    pub kind: SymKind,
    pub depth: usize,
    /// Trailing part of the declaration, for context in the outline.
    pub detail: String,
}

/// Language family — broader than the grammar, because grammars proliferate and
/// declaration syntax does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Rust,
    Python,
    Js,
    Go,
    CLike,
    Ruby,
    Shell,
    Markdown,
    Yaml,
    Html,
    Css,
    Other,
}

pub fn family(path: &Path, syntax: &str) -> Family {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => return Family::Rust,
        "py" | "pyi" | "pyw" => return Family::Python,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" => return Family::Js,
        "go" => return Family::Go,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "java" | "cs" | "kt" | "kts"
        | "swift" | "scala" | "m" | "mm" | "php" | "dart" => return Family::CLike,
        "rb" | "rake" => return Family::Ruby,
        "sh" | "bash" | "zsh" | "fish" => return Family::Shell,
        "md" | "markdown" | "mdx" => return Family::Markdown,
        "yml" | "yaml" | "toml" | "ini" | "cfg" | "conf" | "properties" => return Family::Yaml,
        "html" | "htm" | "xml" | "vue" | "svelte" => return Family::Html,
        "css" | "scss" | "sass" | "less" => return Family::Css,
        _ => {}
    }
    let s = syntax.to_ascii_lowercase();
    if s.contains("rust") {
        Family::Rust
    } else if s.contains("python") {
        Family::Python
    } else if s.contains("javascript") || s.contains("typescript") {
        Family::Js
    } else if s.contains("go") {
        Family::Go
    } else if s.contains("markdown") {
        Family::Markdown
    } else if s.contains("yaml") || s.contains("toml") {
        Family::Yaml
    } else if s.contains("html") {
        Family::Html
    } else if s.contains("css") {
        Family::Css
    } else {
        Family::Other
    }
}

/// Words that may precede the real declaration keyword. Stripped before a second
/// matching pass so `pub async unsafe fn` still reads as a function.
const MODIFIERS: &[&str] = &[
    "pub(crate)",
    "pub(super)",
    "pub",
    "export",
    "default",
    "async",
    "await",
    "unsafe",
    "extern",
    "public",
    "private",
    "protected",
    "internal",
    "final",
    "abstract",
    "open",
    "override",
    "sealed",
    "inline",
    "partial",
    "mut",
    "late",
    "lazy",
];

/// Control-flow words that must never be mistaken for a declaration.
const NOT_DECLS: &[&str] = &[
    "if", "else", "for", "while", "switch", "return", "catch", "try", "do", "match", "case",
    "with", "assert", "print", "echo", "then", "loop", "when", "unless", "throw", "new", "delete",
    "typeof", "sizeof", "raise", "yield", "break", "continue", "import", "use", "from", "require",
];

const TYPE_WORDS: &[&str] = &[
    "void", "int", "long", "short", "char", "float", "double", "bool", "boolean", "string",
    "String", "byte", "uint", "usize", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32",
    "f64", "Self", "auto",
];

pub fn extract(path: &Path, syntax: &str, lines: &[Line]) -> Vec<Symbol> {
    let fam = family(path, syntax);
    if fam == Family::Markdown {
        return markdown(lines);
    }
    if fam == Family::Yaml {
        return yaml(lines);
    }

    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        // A line that is entirely prose is a comment, never a declaration.
        if line.tokens.is_empty() || line.tokens.iter().all(|t| t.tone.is_prose()) {
            continue;
        }
        let text = line.text();
        if let Some(sym) = match_decl(&fam, &text, line, idx) {
            out.push(sym);
        }
    }
    // Heading/section nesting for C-like languages is derived from the *previous*
    // brace depth, which we approximate; a light depth pass keeps the outline tidy.
    reindent(&mut out);
    out
}

fn match_decl(fam: &Family, text: &str, line: &Line, idx: usize) -> Option<Symbol> {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    // Multi-line declarations: only the first line carries the keyword in almost
    // every language, so nothing special is needed — but `#` prefixed preprocessor
    // lines must not be scanned.
    if trimmed.starts_with('#') && *fam != Family::Python {
        return None;
    }

    let (start, keyword, kind) = find_keyword(*fam, trimmed, line.indent)?;
    let name = if keyword == METHOD_SENTINEL {
        method_like(trimmed)?.0
    } else {
        // Slice from where the keyword *actually* starts: `pub async fn foo` must not
        // be measured from the front of the line.
        extract_name(*fam, keyword, trimmed.get(start + keyword.len()..)?)?
    };
    let detail = signature_tail(trimmed, &name);
    Some(Symbol {
        line: idx,
        name,
        kind,
        depth: line.indent / 4,
        detail,
    })
}

/// Pseudo-keyword for the C-family method heuristic (which has no keyword at all).
const METHOD_SENTINEL: &str = "__method__";

/// Returns (byte offset of the keyword, the keyword, its kind).
fn find_keyword(fam: Family, s: &str, indent: usize) -> Option<(usize, &'static str, SymKind)> {
    use SymKind::*;
    let table: &[(&str, SymKind)] = match fam {
        Family::Rust => &[
            ("macro_rules!", Macro),
            ("fn", Function),
            ("trait", Trait),
            ("struct", Type),
            ("enum", Enum),
            ("union", Type),
            ("impl", Type),
            ("mod", Module),
            ("type", Type),
            ("const", Constant),
            ("static", Constant),
        ],
        Family::Python => &[("def", Function), ("class", Type)],
        Family::Js => &[
            ("function", Function),
            ("class", Type),
            ("interface", Trait),
            ("type", Type),
            ("enum", Enum),
            ("const", Constant),
            ("let", Constant),
            ("var", Constant),
        ],
        Family::Go => &[
            ("func", Function),
            ("type", Type),
            ("var", Constant),
            ("const", Constant),
        ],
        Family::CLike => &[
            ("class", Type),
            ("interface", Trait),
            ("struct", Type),
            ("enum", Enum),
            ("record", Type),
            ("object", Type),
            ("protocol", Trait),
            ("fun", Function),
            ("func", Function),
            ("function", Function),
            ("def", Function),
        ],
        Family::Ruby => &[("def", Function), ("class", Type), ("module", Module)],
        Family::Shell => &[("function", Function)],
        Family::Html | Family::Css | Family::Markdown | Family::Yaml | Family::Other => &[],
    };

    if let Some((kw, kind)) = word_match(s, table) {
        return Some((0, kw, kind));
    }

    // Second pass: strip modifiers and try again.
    let mut rest = s;
    let mut stripped_any = true;
    while stripped_any {
        stripped_any = false;
        for m in MODIFIERS {
            if let Some(after) = strip_word(rest, m) {
                rest = after.trim_start();
                stripped_any = true;
                break;
            }
        }
    }
    if rest.len() != s.len() {
        let offset = s.len() - rest.len();
        if let Some((kw, kind)) = word_match(rest, table) {
            return Some((offset, kw, kind));
        }
    }

    // Shell shorthand: `name() {`
    if fam == Family::Shell {
        if let Some(open) = rest.find("()") {
            let name = rest[..open].trim();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some((0, METHOD_SENTINEL, SymKind::Function));
            }
        }
    }

    // Method heuristic for the C family: `ReturnType name(args) {`. Only at shallow
    // indentation, so that *calls* inside a function body do not become "symbols".
    if indent <= 4
        && matches!(
            fam,
            Family::CLike | Family::Js | Family::Go | Family::Rust | Family::Python
        )
        && method_like(rest).is_some()
    {
        return Some((0, METHOD_SENTINEL, SymKind::Function));
    }
    None
}

/// Matches `keyword` at a word boundary at the start of `s`.
fn word_match(s: &str, table: &[(&'static str, SymKind)]) -> Option<(&'static str, SymKind)> {
    // Longest first so `macro_rules!` is not shadowed by `macro`.
    for (kw, kind) in table {
        if let Some(_after) = strip_word(s, kw) {
            return Some((kw, *kind));
        }
    }
    None
}

/// If `s` starts with `word` followed by a boundary, return the remainder.
fn strip_word<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(word)?;
    if word.ends_with('!') {
        return Some(rest);
    }
    match rest.chars().next() {
        None => Some(rest),
        Some(c) if c.is_whitespace() || c == '(' || c == '<' || c == ':' || c == '{' => Some(rest),
        _ => None,
    }
}

/// Heuristic: `[type] name(args)`, ending in `{`, not a call statement.
fn method_like(s: &str) -> Option<(String, usize)> {
    let s = s.trim_end();
    if !(s.ends_with('{') || s.ends_with(')')) {
        return None;
    }
    let open = s.find('(')?;
    let before = s[..open].trim_end();
    let name: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '~')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if name.is_empty()
        || name
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(true)
    {
        return None;
    }
    if NOT_DECLS.contains(&name.as_str()) {
        return None;
    }
    // The head must look like a type (capitalised or a known primitive), and there
    // must be a space before the name — that is what distinguishes `T foo(` from a call.
    let head = before[..before.len() - name.len()].trim_end();
    if head.is_empty() {
        return None;
    }
    let first = head.split_whitespace().next().unwrap_or("");
    let typish = first
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
        || TYPE_WORDS.contains(&first)
        || head.contains('*')
        || head.contains('&');
    if !typish {
        return None;
    }
    Some((name, open))
}

fn extract_name(fam: Family, keyword: &str, after: &str) -> Option<String> {
    let mut s = after.trim_start();

    // Skip generic/parameter/receiver groups: `impl<T> Foo`, `func (r *T) Name`,
    // `fn f<T>`, `class C<T>`.
    while let Some(first) = s.chars().next() {
        let (open, close) = match first {
            '<' => ('<', '>'),
            '(' => ('(', ')'),
            '[' => ('[', ']'),
            _ => break,
        };
        let mut depth = 0i32;
        let mut cut = None;
        for (i, c) in s.char_indices() {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    cut = Some(i + c.len_utf8());
                    break;
                }
            }
        }
        match cut {
            Some(c) => s = s[c..].trim_start(),
            None => break,
        }
    }

    if fam == Family::Shell {
        // `name() {` or `name ()`
        let name: String = s
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        return (!name.is_empty()).then_some(name);
    }

    let name: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '!' | ':' | '~'))
        .collect();
    // Trailing punctuation that some grammars glue onto identifiers.
    let name = name.trim_end_matches([':', '!']).to_string();

    if name.is_empty() {
        // `impl Trait for Type` where we already consumed the type paren; try again
        // on the raw remainder for `impl` blocks.
        if keyword == "impl" {
            if let Some(rest) = after.trim_start().split(['{', ';']).next() {
                let r = rest.trim();
                if !r.is_empty() {
                    return Some(r.to_string());
                }
            }
        }
        return None;
    }
    if keyword == "impl" {
        // Show the whole `Trait for Type` clause; that is the useful label.
        if let Some(rest) = after.trim_start().split(['{', ';']).next() {
            let r = rest.trim();
            if !r.is_empty() {
                return Some(r.to_string());
            }
        }
    }
    Some(name)
}

/// The bit of the declaration after the name — `(a: u32) -> bool` etc.
fn signature_tail(text: &str, name: &str) -> String {
    let Some(pos) = text.find(name) else {
        return String::new();
    };
    let tail = text[pos + name.len()..].trim_start();
    let tail = tail.trim_start_matches(':').trim_start();
    let tail = tail.trim_end_matches('{').trim_end();
    let mut out: String = tail.chars().take(72).collect();
    if tail.chars().count() > 72 {
        out.push('…');
    }
    // A signature that continues on the next line should show nothing rather than a
    // lone opening bracket.
    let trimmed = out.trim_end_matches(['(', ' ', ',']);
    if trimmed.is_empty() {
        String::new()
    } else {
        out.trim_end().to_string()
    }
}

/// Collapse absurd depth jumps so the outline reads as a tidy tree.
fn reindent(syms: &mut [Symbol]) {
    let mut prev = 0usize;
    for s in syms.iter_mut() {
        s.depth = s.depth.min(prev + 1);
        prev = s.depth;
    }
}

fn markdown(lines: &[Line]) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (idx, line) in lines.iter().enumerate() {
        let text = line.text();
        let t = text.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = t.strip_prefix('#') {
            let hashes = 1 + rest.chars().take_while(|c| *c == '#').count();
            let title = t.trim_start_matches('#').trim();
            if title.is_empty() {
                continue;
            }
            out.push(Symbol {
                line: idx,
                name: title.to_string(),
                kind: SymKind::Heading,
                depth: hashes.saturating_sub(1),
                detail: String::new(),
            });
        }
    }
    out
}

fn yaml(lines: &[Line]) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line.indent > 0 {
            continue;
        }
        let text = line.text();
        let t = text.trim_start();
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        if let Some(colon) = t.find([':', '=']) {
            let key = t[..colon].trim().trim_matches(['"', '\'', '[', ']']);
            if key.is_empty() || key.len() > 60 || key.contains(' ') && !key.contains('"') {
                continue;
            }
            let detail = t[colon + 1..].trim().chars().take(48).collect();
            out.push(Symbol {
                line: idx,
                name: key.to_string(),
                kind: SymKind::Module,
                depth: 0,
                detail,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Highlighter;

    /// Run the real pipeline — grammar, tones, then the heuristic — because the point of
    /// these tests is that comments and strings do not become declarations.
    fn outline(path: &str, text: &str) -> Vec<Symbol> {
        let hl = Highlighter::new();
        let path = Path::new(path);
        let head = text.lines().next().unwrap_or("");
        let syntax = hl.syntax_for(path, head).clone();
        let doc = hl.parse(&syntax, text, text.len());
        extract(path, &doc.syntax, &doc.lines)
    }

    fn named(syms: &[Symbol]) -> Vec<(&str, SymKind, usize)> {
        syms.iter()
            .map(|s| (s.name.as_str(), s.kind, s.depth))
            .collect()
    }

    #[test]
    fn rust_declarations_keep_their_order_and_depth() {
        let text = r#"
pub struct Point { x: i32 }

impl Point {
    pub fn new(x: i32) -> Self { Self { x } }
}

pub trait Draw {
    fn draw(&self);
}

pub enum Color { Red }

mod inner {}

const MAX: usize = 1;

macro_rules! make { () => {} }
"#;
        let syms = outline("point.rs", text);
        assert_eq!(
            named(&syms),
            [
                ("Point", SymKind::Type, 0),
                ("Point", SymKind::Type, 0),
                ("new", SymKind::Function, 1),
                ("Draw", SymKind::Trait, 0),
                ("draw", SymKind::Function, 1),
                ("Color", SymKind::Enum, 0),
                ("inner", SymKind::Module, 0),
                ("MAX", SymKind::Constant, 0),
                ("make", SymKind::Macro, 0),
            ]
        );
    }

    #[test]
    fn comments_and_strings_do_not_become_declarations() {
        let text = r#"
// fn fake() {}
/* struct AlsoFake {} */
let text = "fn string_fake() {}";
fn real() {}
"#;
        let syms = outline("sample.rs", text);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"real"), "{names:?}");
        assert!(!names.contains(&"fake"), "{names:?}");
        assert!(!names.contains(&"AlsoFake"), "{names:?}");
        assert!(!names.contains(&"string_fake"), "{names:?}");
    }

    #[test]
    fn python_indentation_becomes_outline_depth() {
        let text = r#"
class Greeter:
    def hello(self):
        return 1

def top(a, b):
    return a
"#;
        assert_eq!(
            named(&outline("greeter.py", text)),
            [
                ("Greeter", SymKind::Type, 0),
                ("hello", SymKind::Function, 1),
                ("top", SymKind::Function, 0),
            ]
        );
    }

    #[test]
    fn javascript_and_typescript_declarations_are_found() {
        let text = r#"
export interface Shape { area(): number; }
export function load() {}
class Store {}
const cache = new Map();
"#;
        assert_eq!(
            named(&outline("store.ts", text)),
            [
                ("Shape", SymKind::Trait, 0),
                ("load", SymKind::Function, 0),
                ("Store", SymKind::Type, 0),
                ("cache", SymKind::Constant, 0),
            ]
        );
    }

    #[test]
    fn markdown_headings_ignore_fenced_code() {
        let text = r#"
# Title
## Part

```rust
# not a heading
```

### End
"#;
        assert_eq!(
            named(&outline("notes.md", text)),
            [
                ("Title", SymKind::Heading, 0),
                ("Part", SymKind::Heading, 1),
                ("End", SymKind::Heading, 2),
            ]
        );
    }

    #[test]
    fn yaml_keeps_top_level_keys_only() {
        let text = r#"
name: app
services:
  web:
    image: nginx
# comment: nope
plain: value
"#;
        let syms = outline("compose.yaml", text);
        assert_eq!(
            syms.iter()
                .map(|s| (s.name.as_str(), s.kind))
                .collect::<Vec<_>>(),
            [
                ("name", SymKind::Module),
                ("services", SymKind::Module),
                ("plain", SymKind::Module),
            ]
        );
    }

    #[test]
    fn the_extension_beats_the_grammar_name() {
        assert_eq!(family(Path::new("a.ts"), "Plain Text"), Family::Js);
        assert_eq!(family(Path::new("a.unknown"), "Rust"), Family::Rust);
        assert_eq!(family(Path::new("a.rs"), "Python"), Family::Rust);
    }
}
