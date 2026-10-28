//! Distilling a line down to what a human actually reads.
//!
//! In a strongly typed language, a reader is following names and values; the types are
//! checked by a compiler that is not in the room. This module decides which runs of a
//! line are *metadata* — a type annotation, a declared type, a modifier — so the reader
//! can recede them.
//!
//! It is the only place in zenith that hides meaning rather than restyling it, so it is
//! written defensively and tested per language. The rule that matters most is the one
//! that refuses: **fading a value is far worse than fading nothing**, because the reader
//! cannot tell that something is missing. A ternary's `else`, a dictionary's value, a
//! slice's bound and a struct literal's field all use a colon, and none of them is a type
//! annotation.
//!
//! What each language gets:
//!
//! | shape | languages | example |
//! |---|---|---|
//! | annotation *after* the name | Rust, TypeScript, Python, Kotlin, Swift, Scala, Dart, JS | `let total: Vec<u8> = ...` |
//! | type *before* the name | Java, C#, C, C++, Dart, PHP-typed | `String name = ...` |
//! | type after `var` / in a field | Go | `var x int = 1`, `Name string` |
//! | nothing | everything else | the punctuation fading still applies |

use std::path::Path;

use crate::syntax::Line;
use crate::tone::Tone;

/// How a language writes its types, which is what decides how to find them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    /// `name: Type` — the annotation follows the name.
    Postfix,
    /// `Type name` — the type comes first.
    Prefix,
    /// Go, which does both `var name Type` and `name Type`.
    Go,
    /// Nothing to distil beyond punctuation.
    Other,
}

pub fn lang_of(path: &Path, syntax: &str) -> Lang {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "py" | "pyi"
        | "kt" | "kts" | "swift" | "scala" | "svelte" | "vue" => Lang::Postfix,
        "java" | "cs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "m" | "mm" | "dart"
        | "php" | "zig" => Lang::Prefix,
        "go" => Lang::Go,
        _ => {
            let s = syntax.to_ascii_lowercase();
            if s.contains("rust")
                || s.contains("typescript")
                || s.contains("python")
                || s.contains("kotlin")
                || s.contains("swift")
            {
                Lang::Postfix
            } else if s.contains("java") || s.contains("c#") || s.contains("c++") {
                Lang::Prefix
            } else if s.contains("go") {
                Lang::Go
            } else {
                Lang::Other
            }
        }
    }
}

/// Which tokens of this line are metadata? `None` when nothing can be said safely.
pub fn mask(lang: Lang, line: &Line) -> Option<Vec<bool>> {
    if line.tokens.is_empty() {
        return None;
    }
    match lang {
        Lang::Postfix => postfix(line),
        Lang::Prefix => prefix(line),
        Lang::Go => go(line),
        Lang::Other => None,
    }
}

/// How much of a token is metadata.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// The whole run.
    All,
    /// Characters `start..end` of the run, counted in characters.
    Part(usize, usize),
}

/// The marks to apply, which is [`mask`] where the grammar gave the colon a token of its
/// own — plus a fallback for the grammars that did not.
///
/// The fallback matters for TypeScript: syntect ships no TypeScript grammar, so `.ts`
/// files are set with the JavaScript grammar, under which `total: number` arrives as a
/// *single* run of text with no colon token to find. That run is split here, but only
/// when the shape is unmistakable — a bare identifier, a colon, and something that reads
/// as a type before the `=`.
pub fn marks(lang: Lang, line: &Line) -> Vec<Option<Mark>> {
    let n = line.tokens.len();
    let whole = |m: Option<Vec<bool>>| -> Vec<Option<Mark>> {
        match m {
            Some(flags) => flags
                .into_iter()
                .map(|f| if f { Some(Mark::All) } else { None })
                .collect(),
            None => vec![None; n],
        }
    };
    let base = mask(lang, line);
    if base.is_some() || lang != Lang::Postfix {
        return whole(base);
    }
    match split_inline_annotation(line) {
        Some((idx, start, eq)) => {
            let mut out = vec![None; n];
            let len = line.tokens[idx].text.chars().count();
            out[idx] = Some(Mark::Part(start.min(len), len));
            for m in out.iter_mut().take(eq).skip(idx + 1) {
                *m = Some(Mark::All);
            }
            out
        }
        None => vec![None; n],
    }
}

/// Find `name: Type` *inside* a single run of text.
///
/// Returns the token, the character at which the metadata starts, and the index of the
/// `=` that ends it.
fn split_inline_annotation(line: &Line) -> Option<(usize, usize, usize)> {
    let toks = &line.tokens;
    let depth = depths(toks);
    let eq = toks
        .iter()
        .enumerate()
        .find(|(i, t)| depth[*i] == 0 && is_assign(t))
        .map(|(i, _)| i)?;
    if line.tokens[..eq].iter().any(is_value_token) {
        return None;
    }

    for i in 0..eq {
        let t = &toks[i];
        if t.tone != Tone::Plain || depth[i] != 0 {
            continue;
        }
        // The colon must separate a bare identifier from something type-shaped, and
        // the identifier must be the whole of the run before it.
        let Some(colon) = t.text.find(':') else {
            continue;
        };
        let name = t.text[..colon].trim();
        // Either a bare identifier (`total`), or a declaration keyword glued to one
        // (`val name`, which is what the Java grammar makes of Kotlin).
        let ok = if name.is_empty() {
            false
        } else if name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        {
            true
        } else {
            let mut words = name.split_whitespace();
            match (words.next(), words.next(), words.next()) {
                (Some(kw), Some(rest), None) => {
                    DECL_WORDS.contains(&kw)
                        && rest
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                }
                _ => false,
            }
        };
        if !ok {
            continue;
        }
        let after = t.text[colon + 1..].trim();
        let tail_type = !after.is_empty()
            && (is_type_word(
                after
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or(""),
            ) || after
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
                || after.contains('<'));
        let later_type = toks[i + 1..eq]
            .iter()
            .any(|t| looks_like_type(std::slice::from_ref(t)));
        if !tail_type && !later_type {
            continue;
        }
        let start = t.text[..colon].chars().count();
        return Some((i, start, eq));
    }
    None
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/// Bracket depth *at* each token: a closer carries the depth it closes.
fn depths(toks: &[crate::syntax::Token]) -> Vec<i32> {
    let mut depth = 0i32;
    let mut out = Vec::with_capacity(toks.len());
    for t in toks {
        out.push(depth);
        for c in t.text.chars() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
    }
    out
}

/// A colon, however this grammar chose to tone it. TypeScript scopes `:` as an
/// *operator*, Rust as punctuation, and a rule that only accepted one of those silently
/// did nothing for half the languages it claimed to support.
fn is_colon(t: &crate::syntax::Token) -> bool {
    matches!(t.tone, Tone::Punct | Tone::Operator) && t.text.trim() == ":"
}

/// `=`, `:=`, `+=` … but not `==`, which is a comparison.
fn is_assign(t: &crate::syntax::Token) -> bool {
    if t.tone != Tone::Operator {
        return false;
    }
    let s = t.text.trim();
    s.contains('=')
        && !s.contains("==")
        && !s.contains("=>")
        && !s.contains("<=")
        && !s.contains(">=")
        && !s.contains("!=")
}

/// Does this token look like a name (so a colon after it could be an annotation)?
fn is_name(t: &crate::syntax::Token) -> bool {
    if !t.text.chars().any(|c| c.is_alphanumeric()) {
        return false;
    }
    match t.tone {
        Tone::Plain | Tone::Type | Tone::Call | Tone::Constant => true,
        // Grammars often group a declaration keyword with the name it declares
        // (`let name`, `const total`). A bare keyword is not a name.
        Tone::Keyword | Tone::KeywordSoft => t.text.contains(' '),
        _ => false,
    }
}

/// Operators that mean *this is a value*, not a type. A type may contain `<`, `>`, `&`,
/// `|`, `::`, `->`, `[`, `]`, `.` and `,`; it does not contain arithmetic, comparison,
/// a ternary or an assignment.
fn is_value_operator(text: &str) -> bool {
    matches!(
        text.trim(),
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "=="
            | "==="
            | "!="
            | "!=="
            | "<="
            | ">="
            | "&&"
            | "||"
            | "!"
            | "++"
            | "--"
            | "?"
            | "??"
            | "=>"
            | "<<"
            | ">>"
            | "+="
            | "-="
            | "*="
            | "/="
            | "%="
    )
}

fn is_value_token(t: &crate::syntax::Token) -> bool {
    match t.tone {
        Tone::Str | Tone::Number | Tone::Constant => true,
        Tone::Operator => is_value_operator(&t.text),
        _ => false,
    }
}

fn ends_statement(t: &crate::syntax::Token) -> bool {
    let s = t.text.trim_end();
    s.ends_with(';') || s.ends_with(',')
}

/// Words that can stand in front of the name they declare.
const DECL_WORDS: &[&str] = &[
    "val",
    "var",
    "let",
    "const",
    "final",
    "static",
    "public",
    "private",
    "protected",
    "pub",
    "mut",
    "readonly",
    "my",
];

const TYPE_WORDS: &[&str] = &[
    "bool", "boolean", "byte", "char", "decimal", "double", "error", "float", "float32", "float64",
    "int", "int8", "int16", "int32", "int64", "integer", "long", "rune", "short", "string", "str",
    "uint", "uint8", "uint16", "uint32", "uint64", "u8", "u16", "u32", "u64", "u128", "usize",
    "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "void", "var", "any", "number",
    "object", "Self", "self",
];

fn is_type_word(text: &str) -> bool {
    let t = text.trim();
    TYPE_WORDS.contains(&t)
        || t.to_lowercase().starts_with("uint")
        || t.to_lowercase().starts_with("int")
}

fn is_capitalised(text: &str) -> bool {
    text.trim()
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

/// Positive evidence that a run of tokens is a *type* rather than an expression.
fn looks_like_type(run: &[crate::syntax::Token]) -> bool {
    run.iter().any(|t| {
        t.tone == Tone::Type
            || is_type_word(&t.text)
            || is_capitalised(&t.text)
            || t.text.contains('<')
    })
}

// ---------------------------------------------------------------------------
// `name: Type` — Rust, TypeScript, Python, Kotlin, Swift
// ---------------------------------------------------------------------------

fn postfix(line: &Line) -> Option<Vec<bool>> {
    let toks = &line.tokens;
    let depth = depths(toks);
    let colon = toks
        .iter()
        .enumerate()
        .find(|(i, t)| *i > 0 && depth[*i] == 0 && is_colon(t))
        .map(|(i, _)| i)?;

    // The name being annotated: TypeScript's optional marker sits between the name and
    // the colon (`name?: string`), so step over it before looking for a name.
    let mut named = colon - 1;
    if toks[named].text.trim() == "?" && named > 0 {
        named -= 1;
    }
    if !is_name(&toks[named]) {
        return None;
    }
    // A `?` before the colon at the same level makes this a ternary — unless it is
    // TypeScript's optional marker, `name?: string`, which sits right against the colon.
    let ternary = toks[..colon]
        .iter()
        .enumerate()
        .any(|(i, t)| depth[i] == 0 && t.text.trim() == "?" && !is_colon(&toks[i + 1]));
    if ternary {
        return None;
    }

    // The annotation runs to the `=`, or to the end of a statement.
    let (start, end, declared) = match toks
        .iter()
        .enumerate()
        .skip(colon + 1)
        .find(|(i, t)| depth[*i] == 0 && is_assign(t))
    {
        Some((eq, _)) => (colon, eq, true),
        None => {
            if !ends_statement(&toks[toks.len() - 1]) {
                return None;
            }
            (colon, toks.len() - 1, false)
        }
    };
    let run = &toks[start..end];
    if run.len() < 2 {
        return None; // a colon and nothing else is not an annotation
    }
    // A value in the run means this colon was not announcing a type: a dictionary
    // entry, a struct literal, a match arm.
    if run.iter().any(is_value_token) {
        return None;
    }
    // Without an `=` to prove this is a declaration, require a type to be *present*.
    // `balance: i64,` qualifies; `name: user_name,` does not, and refusing it is what
    // keeps multi-line struct literals and dictionaries out of trouble.
    if !declared && !looks_like_type(run) {
        return None;
    }
    let mut mask = vec![false; toks.len()];
    for m in mask.iter_mut().take(end).skip(start) {
        *m = true;
    }
    Some(mask)
}

// ---------------------------------------------------------------------------
// `Type name` — Java, C#, C, C++
// ---------------------------------------------------------------------------

/// Leading words that qualify a declaration without being its type.
const MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "final",
    "abstract",
    "sealed",
    "readonly",
    "const",
    "volatile",
    "transient",
    "synchronized",
    "native",
    "inline",
    "extern",
    "unsafe",
    "mutable",
    "override",
    "open",
    "late",
    "var",
];

fn prefix(line: &Line) -> Option<Vec<bool>> {
    let toks = &line.tokens;
    let depth = depths(toks);
    let eq = toks
        .iter()
        .enumerate()
        .find(|(i, t)| depth[*i] == 0 && is_assign(t))
        .map(|(i, _)| i)?;
    // Neither a call nor an array index may appear before the `=`.
    if toks[..eq].iter().any(|t| t.text.contains('(')) {
        return None;
    }
    let start = toks[..eq]
        .iter()
        .position(|t| {
            let s = t.text.trim();
            !s.is_empty() && !MODIFIERS.contains(&s)
        })
        .unwrap_or(eq);
    // The variable is the last name before the `=`.
    let name = (start..eq).rev().find(|i| is_name(&toks[*i]))?;
    if name <= start {
        return None;
    }
    let run = &toks[start..name];
    if !looks_like_type(run) || run.iter().any(is_value_token) {
        return None;
    }
    let mut mask = vec![false; toks.len()];
    for m in mask.iter_mut().take(name).skip(start) {
        *m = true;
    }
    Some(mask)
}

// ---------------------------------------------------------------------------
// Go: `var name Type = ...` and `Name Type`
// ---------------------------------------------------------------------------

fn go(line: &Line) -> Option<Vec<bool>> {
    let toks = &line.tokens;
    if toks.len() < 2 {
        return None;
    }
    // `var x int = 1` — the type is the last name before the `=`, and the name before
    // it is the variable, so a comma-separated list is not a single declaration.
    if toks[0].text.trim() == "var" {
        let depth = depths(toks);
        let eq = toks
            .iter()
            .enumerate()
            .find(|(i, t)| depth[*i] == 0 && is_assign(t))
            .map(|(i, _)| i)?;
        let head = &toks[1..eq];
        if head.len() < 2 || head.iter().any(|t| t.text.contains(',')) {
            return None;
        }
        let ty = (1..eq).rev().find(|i| is_name(&toks[*i]))?;
        if !looks_like_type(&toks[ty..ty + 1]) {
            return None;
        }
        let mut mask = vec![false; toks.len()];
        if ty < mask.len() {
            mask[ty] = true;
        }
        return Some(mask);
    }
    // `Name string` — a field or a parameter, with no `=` to make it obvious. Ignore
    // whitespace, which the grammar hands us as its own token: require exactly two
    // words, the second of which looks like a type, and either the first is exported
    // (capitalised, as Go's fields are) or the second is a type the language knows.
    if toks
        .iter()
        .any(|t| t.text.contains(['=', '(', ')', '*', '[']))
    {
        return None;
    }
    let words: Vec<usize> = (0..toks.len())
        .filter(|i| !toks[*i].text.trim().is_empty())
        .collect();
    if words.len() == 2 {
        let (a, b) = (words[0], words[1]);
        if looks_like_type(&toks[b..b + 1])
            && (is_capitalised(&toks[a].text) || is_type_word(&toks[b].text))
        {
            let mut mask = vec![false; toks.len()];
            mask[b] = true;
            return Some(mask);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{Flow, Line, Token};

    /// Build a line from (text, tone) pairs, which is what the parser would produce.
    fn line(pairs: &[(&str, Tone)]) -> Line {
        Line {
            indent: 0,
            indent_bytes: 0,
            tokens: pairs
                .iter()
                .map(|(t, tone)| Token {
                    text: t.to_string(),
                    tone: *tone,
                })
                .collect(),
            flow: Flow::Line,
            dominant: Tone::Plain,
        }
    }

    fn postfix_faded(l: &Line) -> String {
        match mask(Lang::Postfix, l) {
            Some(m) => l
                .tokens
                .iter()
                .zip(m)
                .filter(|(_, f)| *f)
                .map(|(t, _)| t.text.as_str())
                .collect::<String>()
                .trim()
                .to_string(),
            None => String::new(),
        }
    }

    fn prefix_faded(l: &Line) -> String {
        match mask(Lang::Prefix, l) {
            Some(m) => l
                .tokens
                .iter()
                .zip(m)
                .filter(|(_, f)| *f)
                .map(|(t, _)| t.text.as_str())
                .collect::<String>()
                .trim()
                .to_string(),
            None => String::new(),
        }
    }

    #[test]
    fn rust_annotations_are_metadata() {
        use Tone::*;
        let l = line(&[
            ("let", Keyword),
            (" total", Plain),
            (":", Punct),
            (" ", Plain),
            ("Vec", Type),
            ("<", Punct),
            ("u8", Type),
            (">", Punct),
            (" ", Plain),
            ("=", Operator),
        ]);
        assert_eq!(postfix_faded(&l), ": Vec<u8>");
    }

    #[test]
    fn rust_struct_fields_are_metadata_but_struct_literals_are_not() {
        use Tone::*;
        // `balance: i64,` — a field declaration: the type is present, so fade it.
        let field = line(&[
            ("balance", Plain),
            (":", Punct),
            (" ", Plain),
            ("i64", Type),
            (",", Punct),
        ]);
        assert_eq!(postfix_faded(&field), ": i64");

        // `balance: total,` — a struct literal entry: `total` is a value, not a type.
        let literal = line(&[
            ("balance", Plain),
            (":", Punct),
            (" ", Plain),
            ("total", Plain),
            (",", Punct),
        ]);
        assert_eq!(postfix_faded(&literal), "", "a value must never be faded");
    }

    #[test]
    fn rust_closures_and_matches_are_not_touched() {
        use Tone::*;
        let lambda = line(&[
            ("n", Plain),
            (":", Punct),
            (" n ", Plain),
            ("+", Operator),
            (" ", Plain),
            ("1", Number),
        ]);
        assert_eq!(postfix_faded(&lambda), "");
    }

    #[test]
    fn typescript_annotations_work_even_though_its_colon_is_an_operator() {
        use Tone::*;
        let l = line(&[
            ("const total", KeywordSoft),
            (":", Operator),
            (" number ", Plain),
            ("=", Operator),
            (" ", Plain),
            ("5", Number),
            (";", Punct),
        ]);
        assert_eq!(postfix_faded(&l), ": number");
    }

    #[test]
    fn typescript_objects_and_ternaries_are_not_touched() {
        use Tone::*;
        let object = line(&[
            ("const obj", KeywordSoft),
            (" = ", Operator),
            ("{", Punct),
            (" a", Plain),
            (":", Operator),
            (" ", Plain),
            ("1", Number),
            (",", Punct),
            ("}", Punct),
            (";", Punct),
        ]);
        assert_eq!(postfix_faded(&object), "");

        let ternary = line(&[
            ("const pick", KeywordSoft),
            (" = ", Operator),
            ("flag", Plain),
            (" ? ", Operator),
            ("a", Plain),
            (" : ", Operator),
            ("b", Plain),
            (";", Punct),
        ]);
        assert_eq!(postfix_faded(&ternary), "", "a ternary's else is a value");
    }

    #[test]
    fn typescript_optional_markers_still_allow_the_annotation() {
        use Tone::*;
        let l = line(&[
            ("name", Plain),
            ("?", Operator),
            (":", Operator),
            (" string", Plain),
            (";", Punct),
        ]);
        assert_eq!(postfix_faded(&l), ": string");
    }

    #[test]
    fn python_dicts_slices_and_lambdas_are_not_touched() {
        use Tone::*;
        let dict = line(&[
            ("d ", Plain),
            ("=", Operator),
            (" ", Plain),
            ("{", Punct),
            ("\"a\"", Str),
            (":", Punct),
            (" ", Plain),
            ("1", Number),
            ("}", Punct),
        ]);
        assert_eq!(postfix_faded(&dict), "");

        let slice = line(&[
            ("y ", Plain),
            ("=", Operator),
            (" data", Plain),
            ("[", Punct),
            ("1", Number),
            (":", Punct),
            ("2", Number),
        ]);
        assert_eq!(postfix_faded(&slice), "");

        let lambda = line(&[
            ("f ", Plain),
            ("=", Operator),
            (" ", Plain),
            ("lambda", Keyword),
            (" n", Plain),
            (":", Punct),
            (" n ", Plain),
            ("+", Operator),
            (" 1", Number),
        ]);
        assert_eq!(postfix_faded(&lambda), "");
    }

    #[test]
    fn python_annotations_are_metadata() {
        use Tone::*;
        let l = line(&[
            ("x", Plain),
            (":", Punct),
            (" ", Plain),
            ("int", Type),
            (" ", Plain),
            ("=", Operator),
        ]);
        assert_eq!(postfix_faded(&l), ": int");
    }

    #[test]
    fn java_types_recede_and_ternaries_do_not() {
        use Tone::*;
        let decl = line(&[
            ("String", Type),
            (" name ", Plain),
            ("=", Operator),
            (" ", Plain),
            ("\"x\"", Str),
            (";", Punct),
        ]);
        assert_eq!(prefix_faded(&decl), "String");

        let generic = line(&[
            ("Map", Type),
            ("<", Punct),
            ("String", Type),
            (", ", Punct),
            ("Object", Type),
            (">", Punct),
            (" m ", Plain),
            ("=", Operator),
        ]);
        assert_eq!(prefix_faded(&generic), "Map<String, Object>");

        let ternary = line(&[
            ("int", Type),
            (" pick ", Plain),
            ("=", Operator),
            (" flag ", Plain),
            ("? ", Operator),
            ("1", Number),
            (" : ", Operator),
            ("2", Number),
            (";", Punct),
        ]);
        assert_eq!(
            prefix_faded(&ternary),
            "int",
            "only the type, never the value"
        );

        let foreach = line(&[
            ("for", Keyword),
            (" ", Plain),
            ("(", Punct),
            ("String", Type),
            (" s ", Plain),
            (":", Operator),
            (" list", Plain),
            (")", Punct),
        ]);
        assert_eq!(
            prefix_faded(&foreach),
            "",
            "a for-each is not a declaration"
        );
    }

    #[test]
    fn go_types_after_var_and_in_fields() {
        use Tone::*;
        let var = line(&[
            ("var", Keyword),
            (" x ", Plain),
            ("int", Type),
            (" = ", Operator),
            ("1", Number),
        ]);
        assert_eq!(
            mask(Lang::Go, &var).map(|m| var
                .tokens
                .iter()
                .zip(m)
                .filter(|(_, f)| *f)
                .map(|(t, _)| t.text.as_str())
                .collect::<String>()
                .trim()
                .to_string()),
            Some("int".into())
        );

        let field = line(&[("Name", Plain), (" string", Type)]);
        assert_eq!(
            mask(Lang::Go, &field).map(|m| field
                .tokens
                .iter()
                .zip(m)
                .filter(|(_, f)| *f)
                .map(|(t, _)| t.text.as_str())
                .collect::<String>()
                .trim()
                .to_string()),
            Some("string".into())
        );
    }

    #[test]
    fn a_type_glued_into_one_run_is_split_out() {
        use Tone::*;
        // What the JavaScript grammar makes of `const total: number = 5;` — the
        // annotation has no token of its own.
        let l = line(&[
            ("const", Keyword),
            (" total: number ", Plain),
            ("=", Operator),
            (" ", Plain),
            ("5", Number),
            (";", Punct),
        ]);
        assert_eq!(
            marks(Lang::Postfix, &l),
            vec![None, Some(Mark::Part(6, 15)), None, None, None, None],
            "the name stays, the annotation recedes"
        );
    }

    #[test]
    fn a_value_glued_into_a_run_is_not_split() {
        use Tone::*;
        // `{ total: count }` — an object literal the grammar also hands us as one run.
        let l = line(&[
            ("const", Keyword),
            (" obj ", Plain),
            ("=", Operator),
            (" ", Plain),
            ("{", Punct),
            (" total: count ", Plain),
            ("}", Punct),
        ]);
        assert_eq!(
            marks(Lang::Postfix, &l),
            vec![None; 7],
            "no `=` after the colon, so it is not a declaration"
        );
    }

    #[test]
    fn languages_with_no_distillation_say_so() {
        use Tone::*;
        let l = line(&[("foo", Plain), (" = ", Operator), ("bar", Plain)]);
        assert!(mask(Lang::Other, &l).is_none());
    }
}
