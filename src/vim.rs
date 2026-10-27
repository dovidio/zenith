//! Vim-shaped navigation, for a reader that has no caret.
//!
//! The reading cursor is a *place* — a line and a column — and every motion here is a
//! pure function from one place to another. Keeping them pure matters: navigation is
//! the part of an interface that is easiest to get subtly wrong (a `w` that skips a
//! word, a `%` that lands one bracket off), and it is the part a test can actually
//! check. The app supplies the keys; this module supplies the meaning.
//!
//! What is deliberately *not* here: editing motions (`d`, `c`, `p`), because there is
//! nothing to edit, and `f`/`t` character search, which needs a caret on the page.

use crate::syntax::Document;

/// A place in a file: a line, and a column counted in characters of that line's text
/// (indentation excluded, because zenith does not draw indentation as text).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub struct Caret {
    pub line: usize,
    pub col: usize,
}

impl Caret {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// A movement, before it is resolved against a document.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Down(usize),
    Up(usize),
    /// Start of the next word.
    WordNext,
    /// Start of the previous word.
    WordPrev,
    /// End of the current or next word.
    WordEnd,
    /// First non-blank character of the line.
    LineFirst,
    /// Column zero.
    LineStart,
    /// Last character of the line.
    LineEnd,
    ParagraphNext,
    ParagraphPrev,
    /// The bracket matching the first one at or after the caret.
    BracketMatch,
    FileTop,
    FileBottom,
}

/// Two-key sequences. `gd` is reserved for the language-server step: a reader that can
/// navigate to a definition is the whole point of having one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chord {
    FileTop,
    Centre,
    Top,
    Bottom,
    Definition,
    Review,
    ReviewBase,
}

pub fn chord(first: char, second: char) -> Option<Chord> {
    match (first, second) {
        ('g', 'g') => Some(Chord::FileTop),
        ('z', 'z') => Some(Chord::Centre),
        ('z', 't') => Some(Chord::Top),
        ('z', 'b') => Some(Chord::Bottom),
        ('g', 'd') => Some(Chord::Definition),
        // `g r` review, `g b` its base. Both are additions to the reader's grammar,
        // not editing commands.
        ('g', 'r') => Some(Chord::Review),
        ('g', 'b') => Some(Chord::ReviewBase),
        _ => None,
    }
}

/// Which keys begin a two-key sequence.
pub fn starts_chord(c: char) -> bool {
    matches!(c, 'g' | 'z')
}

/// What to do with one typed character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feed {
    /// A two-key sequence completed.
    Chord(Chord),
    /// This character begins a sequence; the next one decides.
    Pending,
    /// Not part of a sequence — handle it as an ordinary key.
    Pass,
}

/// The state of a two-key sequence, across frames.
///
/// Three rules keep a sequence from eating the reader's keys. A chord completes only on a
/// real key press, never on incidental text. A key that does not complete the sequence is
/// passed through to the ordinary handler. And a shifted letter is a different character,
/// so `⇧Z` can never complete a `z`-prefix chord.
#[derive(Default, Debug)]
pub struct Chords {
    pending: Option<char>,
}

impl Chords {
    /// Feed one character. A key that does not complete the sequence is *passed through*
    /// rather than swallowed, so nothing the reader pressed is ever lost.
    pub fn feed(&mut self, c: char) -> Feed {
        if let Some(first) = self.pending.take() {
            return match chord(first, c) {
                Some(done) => Feed::Chord(done),
                None => Feed::Pass,
            };
        }
        if starts_chord(c) {
            self.pending = Some(c);
            return Feed::Pending;
        }
        Feed::Pass
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// Keep a caret inside the document, on a line that has that many characters.
pub fn clamp(doc: &Document, caret: Caret) -> Caret {
    if doc.lines.is_empty() {
        return Caret::default();
    }
    let line = caret.line.min(doc.lines.len() - 1);
    Caret {
        line,
        col: caret.col.min(line_len(doc, line)),
    }
}

/// A place worth stopping at: the first non-blank character of a line.
pub fn landing(doc: &Document, line: usize) -> Caret {
    let line = line.min(doc.lines.len().saturating_sub(1));
    let text: Vec<char> = chars(doc, line);
    let col = text.iter().position(|c| !c.is_whitespace()).unwrap_or(0);
    Caret { line, col }
}

fn chars(doc: &Document, line: usize) -> Vec<char> {
    doc.lines
        .get(line)
        .map(|l| l.text().chars().collect())
        .unwrap_or_default()
}

fn line_len(doc: &Document, line: usize) -> usize {
    chars(doc, line).len().saturating_sub(1)
}

fn is_blank(doc: &Document, line: usize) -> bool {
    doc.lines.get(line).map(|l| l.is_blank()).unwrap_or(true)
}

/// Vim's three character classes: whitespace, word characters, everything else.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

/// The same class, treated as "not a space" for the purpose of grouping.
fn same_run(a: char, b: char) -> bool {
    let (a, b) = (class(a), class(b));
    a != Class::Space && a == b
}

pub fn resolve(doc: &Document, caret: Caret, motion: Motion) -> Caret {
    let caret = clamp(doc, caret);
    let n = doc.lines.len();
    if n == 0 {
        return caret;
    }
    let moved = match motion {
        Motion::Down(k) => clamp(doc, Caret::new(caret.line + k, caret.col)),
        Motion::Up(k) => clamp(doc, Caret::new(caret.line.saturating_sub(k), caret.col)),
        Motion::FileTop => landing(doc, 0),
        Motion::FileBottom => landing(doc, n - 1),
        Motion::LineStart => Caret::new(caret.line, 0),
        Motion::LineFirst => landing(doc, caret.line),
        Motion::LineEnd => Caret::new(caret.line, line_len(doc, caret.line)),
        Motion::ParagraphNext => {
            // Walk off the end of this paragraph, cross the gap, land on the next one.
            let mut line = caret.line + 1;
            while line < n && !is_blank(doc, line) {
                line += 1;
            }
            while line < n && is_blank(doc, line) {
                line += 1;
            }
            landing(doc, line.min(n - 1))
        }
        Motion::ParagraphPrev => {
            // Up over this paragraph, back across the gap, then to its first line.
            let mut line = caret.line;
            while line > 0 && is_blank(doc, line) {
                line -= 1;
            }
            while line > 0 && !is_blank(doc, line) {
                line -= 1;
            }
            while line > 0 && is_blank(doc, line) {
                line -= 1;
            }
            while line > 0 && !is_blank(doc, line - 1) {
                line -= 1;
            }
            landing(doc, line)
        }
        Motion::WordNext => word_next(doc, caret),
        Motion::WordPrev => word_prev(doc, caret),
        Motion::WordEnd => word_end(doc, caret),
        Motion::BracketMatch => bracket_match(doc, caret).unwrap_or(caret),
    };
    clamp(doc, moved)
}

fn word_next(doc: &Document, caret: Caret) -> Caret {
    let n = doc.lines.len();
    let mut line = caret.line;
    let mut text = chars(doc, line);
    let mut i = caret.col;

    // Within the line: finish the run we are in, cross the space, land on the next run.
    while i < text.len() {
        if class(text[i]) == Class::Space {
            while i < text.len() && class(text[i]) == Class::Space {
                i += 1;
            }
            break;
        }
        let start = text[i];
        i += 1;
        while i < text.len() && same_run(start, text[i]) {
            i += 1;
        }
        if i < text.len() {
            while i < text.len() && class(text[i]) == Class::Space {
                i += 1;
            }
            break;
        }
    }
    if i < text.len() {
        return Caret::new(line, i);
    }
    // Otherwise: the first non-blank of the next line that has one.
    while line + 1 < n {
        line += 1;
        text = chars(doc, line);
        if let Some(c) = text.iter().position(|c| !c.is_whitespace()) {
            return Caret::new(line, c);
        }
        if line + 1 == n {
            break;
        }
    }
    Caret::new(line, i.min(text.len().saturating_sub(1)))
}

fn word_prev(doc: &Document, caret: Caret) -> Caret {
    let mut line = caret.line;
    let mut text = chars(doc, line);
    let mut i = caret.col;

    loop {
        if i == 0 {
            if line == 0 {
                return Caret::new(0, 0);
            }
            line -= 1;
            text = chars(doc, line);
            i = text.len();
            continue;
        }
        i -= 1;
        if class(text[i]) == Class::Space {
            continue;
        }
        let mine = text[i];
        while i > 0 && same_run(mine, text[i - 1]) {
            i -= 1;
        }
        return Caret::new(line, i);
    }
}

fn word_end(doc: &Document, caret: Caret) -> Caret {
    let n = doc.lines.len();
    let mut line = caret.line;
    let mut text = chars(doc, line);
    let mut i = caret.col;

    loop {
        if i + 1 < text.len() {
            i += 1;
            if class(text[i]) == Class::Space {
                continue;
            }
            let mine = text[i];
            while i + 1 < text.len() && same_run(mine, text[i + 1]) {
                i += 1;
            }
            return Caret::new(line, i);
        }
        if line + 1 >= n {
            return Caret::new(line, text.len().saturating_sub(1));
        }
        line += 1;
        text = chars(doc, line);
        i = 0;
        if text.iter().all(|c| c.is_whitespace()) {
            continue;
        }
        while i + 1 < text.len() && same_run(text[0], text[i + 1]) {
            i += 1;
        }
        if class(text[i]) != Class::Space {
            return Caret::new(line, i);
        }
    }
}

/// The bracket matching the first one at or after the caret, searched across lines.
fn bracket_match(doc: &Document, caret: Caret) -> Option<Caret> {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    let (open, close, from, forward) = {
        let text = chars(doc, caret.line);
        let mut found = None;
        for (i, ch) in text.iter().enumerate().skip(caret.col) {
            for (o, c) in PAIRS {
                if *ch == o {
                    found = Some((o, c, i, true));
                    break;
                }
                if *ch == c {
                    found = Some((c, o, i, false));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        found?
    };

    let mut depth = 0i32;
    if forward {
        let mut line = caret.line;
        let mut col = from;
        loop {
            let text = chars(doc, line);
            while col < text.len() {
                let ch = text[col];
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(Caret::new(line, col));
                    }
                }
                col += 1;
            }
            if line + 1 >= doc.lines.len() {
                return None;
            }
            line += 1;
            col = 0;
        }
    } else {
        let mut line = caret.line;
        let mut col = from as isize;
        loop {
            let text = chars(doc, line);
            while col >= 0 {
                let ch = text[col as usize];
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(Caret::new(line, col as usize));
                    }
                }
                col -= 1;
            }
            if line == 0 {
                return None;
            }
            line -= 1;
            col = chars(doc, line).len() as isize - 1;
        }
    }
}

/// Every place `needle` occurs, in document order.
pub fn matches(doc: &Document, needle: &str) -> Vec<Caret> {
    if needle.is_empty() {
        return Vec::new();
    }
    let fold = !doc.lines.iter().any(|l| l.text().contains(needle));
    let needle_cmp = if fold {
        needle.to_lowercase()
    } else {
        needle.to_string()
    };
    let mut out = Vec::new();
    for (i, line) in doc.lines.iter().enumerate() {
        let text = line.text();
        let hay = if fold {
            text.to_lowercase()
        } else {
            text.clone()
        };
        let mut from = 0usize;
        while let Some(byte) = hay[from..].find(&needle_cmp) {
            let at = from + byte;
            let col = hay[..at].chars().count();
            out.push(Caret::new(i, col));
            from = at + needle_cmp.len().max(1);
            if from >= hay.len() {
                break;
            }
        }
    }
    out
}

/// The next or previous match after `from`, wrapping around the file.
pub fn search(doc: &Document, needle: &str, from: Caret, forward: bool) -> Option<Caret> {
    let hits = matches(doc, needle);
    if hits.is_empty() {
        return None;
    }
    if forward {
        hits.iter()
            .find(|c| **c > from)
            .or_else(|| hits.first())
            .copied()
    } else {
        hits.iter()
            .rev()
            .find(|c| **c < from)
            .or_else(|| hits.last())
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{Flow, Line, Token};
    use crate::theme::Tone;

    fn doc(lines: &[&str]) -> Document {
        Document {
            syntax: "Test".into(),
            md: None,
            lines: lines
                .iter()
                .map(|t| {
                    let indent = t.len() - t.trim_start().len();
                    let body = t.trim_start().to_string();
                    Line {
                        indent,
                        indent_bytes: indent,
                        flow: if body.is_empty() {
                            Flow::Blank
                        } else {
                            Flow::Line
                        },
                        dominant: Tone::Plain,
                        tokens: if body.is_empty() {
                            Vec::new()
                        } else {
                            vec![Token {
                                text: body,
                                tone: Tone::Plain,
                            }]
                        },
                    }
                })
                .collect(),
            truncated: false,
            lossy: false,
            bytes: 0,
        }
    }

    #[test]
    fn w_moves_to_the_start_of_the_next_word() {
        let d = doc(&["let total = compute(fee);"]);
        let mut c = Caret::new(0, 0);
        let mut seen = Vec::new();
        for _ in 0..5 {
            c = resolve(&d, c, Motion::WordNext);
            seen.push(c.col);
        }
        // let | total | = | compute | ( | fee
        assert_eq!(seen, vec![4, 10, 12, 19, 20]);
    }

    #[test]
    fn w_crosses_lines_when_the_line_ends() {
        let d = doc(&["fn main() {", "    let x = 1;", "}"]);
        let c = resolve(&d, Caret::new(0, 10), Motion::WordNext);
        assert_eq!(
            c,
            Caret::new(1, 0),
            "lands on the first non-blank of the next line"
        );
    }

    #[test]
    fn b_goes_back_a_word_and_stops_at_the_top() {
        let d = doc(&["let total = compute(fee);"]);
        // From the first letter of `fee`, `b` reaches the start of the previous
        // word, which is the punctuation run `(`.
        let c = resolve(&d, Caret::new(0, 20), Motion::WordPrev);
        assert_eq!(c.col, 19);
        let c = resolve(&d, Caret::new(0, 18), Motion::WordPrev);
        assert_eq!(c.col, 12, "the start of compute");
        let c = resolve(&d, Caret::new(0, 2), Motion::WordPrev);
        assert_eq!(c.col, 0);
        let c = resolve(&d, Caret::new(0, 0), Motion::WordPrev);
        assert_eq!(c, Caret::new(0, 0));
    }

    #[test]
    fn e_lands_on_the_last_character_of_a_word() {
        let d = doc(&["let total = 1;"]);
        // From the start: end of "let" is column 2.
        assert_eq!(resolve(&d, Caret::new(0, 0), Motion::WordEnd).col, 2);
        assert_eq!(resolve(&d, Caret::new(0, 4), Motion::WordEnd).col, 8);
    }

    #[test]
    fn line_motions_use_the_first_non_blank() {
        let d = doc(&["", "        indented();"]);
        assert_eq!(resolve(&d, Caret::new(0, 0), Motion::LineFirst).col, 0);
        let c = resolve(&d, Caret::new(1, 0), Motion::LineFirst);
        // Indentation is not text, so column zero is already the first token.
        assert_eq!(c.col, 0);
        let c = resolve(&d, Caret::new(1, 0), Motion::LineEnd);
        assert_eq!(c.col, "indented();".len() - 1);
    }

    #[test]
    fn braces_find_their_partner_across_lines() {
        let d = doc(&["fn main() {", "    if x {", "        y();", "    }", "}"]);
        let c = resolve(&d, Caret::new(0, 10), Motion::BracketMatch);
        assert_eq!(c.line, 4, "the outer brace closes on the last line");
        // Indentation is not text, so the inner `{` sits at column 5 of "if x {".
        let c = resolve(&d, Caret::new(1, 5), Motion::BracketMatch);
        assert_eq!((c.line, c.col), (3, 0));
        // And backwards, from the closing brace to its opener.
        let c = resolve(&d, Caret::new(3, 0), Motion::BracketMatch);
        assert_eq!((c.line, c.col), (1, 5));
    }

    #[test]
    fn parentheses_are_matched_too() {
        let d = doc(&["compute(a, (b + c))"]);
        let c = resolve(&d, Caret::new(0, 7), Motion::BracketMatch);
        assert_eq!(c.col, 18);
    }

    #[test]
    fn paragraphs_are_separated_by_blank_lines() {
        let d = doc(&["one();", "two();", "", "three();", "", "four();"]);
        let c = resolve(&d, Caret::new(0, 0), Motion::ParagraphNext);
        assert_eq!(c.line, 3);
        let c = resolve(&d, Caret::new(5, 0), Motion::ParagraphPrev);
        assert_eq!(c.line, 3);
    }

    #[test]
    fn search_finds_every_occurrence_and_wraps() {
        let d = doc(&["let total = 0;", "let sum = total + total;"]);
        let hits = matches(&d, "total");
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0], Caret::new(0, 4));
        let next = search(&d, "total", Caret::new(1, 20), true).unwrap();
        assert_eq!(next, Caret::new(0, 4), "wraps to the first hit");
        assert_eq!(hits[2], Caret::new(1, 18));
        let prev = search(&d, "total", Caret::new(0, 4), false).unwrap();
        assert_eq!(prev, Caret::new(1, 18), "wraps backwards to the last hit");
    }

    #[test]
    fn search_falls_back_to_ignoring_case() {
        let d = doc(&["pub struct Ledger;"]);
        assert_eq!(matches(&d, "ledger").len(), 1);
        assert_eq!(matches(&d, "Ledger").len(), 1);
    }

    #[test]
    fn a_key_that_does_not_complete_a_chord_is_passed_through() {
        let mut c = Chords::default();
        assert_eq!(c.feed('z'), Feed::Pending);
        // `⇧Z` arrives as 'Z': not a continuation of `z`, so it must reach the
        // ordinary handler rather than being swallowed by the prefix.
        assert_eq!(c.feed('Z'), Feed::Pass);
        assert!(
            !c.is_pending(),
            "the prefix must not survive a failed chord"
        );
    }

    #[test]
    fn chords_complete_and_clear() {
        let mut c = Chords::default();
        assert_eq!(c.feed('g'), Feed::Pending);
        assert_eq!(c.feed('g'), Feed::Chord(Chord::FileTop));
        assert!(!c.is_pending());
        assert_eq!(c.feed('z'), Feed::Pending);
        assert_eq!(c.feed('t'), Feed::Chord(Chord::Top));
        assert_eq!(c.feed('x'), Feed::Pass);
    }

    #[test]
    fn a_shifted_letter_never_completes_a_lowercase_chord() {
        let mut c = Chords::default();
        assert_eq!(c.feed('g'), Feed::Pending);
        assert_eq!(c.feed('G'), Feed::Pass, "⇧G is not `gg`");
    }

    #[test]
    fn chords_are_recognised() {
        assert_eq!(chord('g', 'g'), Some(Chord::FileTop));
        assert_eq!(chord('z', 'z'), Some(Chord::Centre));
        assert_eq!(chord('g', 'd'), Some(Chord::Definition));
        assert_eq!(chord('g', 'x'), None);
        assert!(starts_chord('g'));
        assert!(!starts_chord('q'));
    }

    #[test]
    fn motions_never_leave_the_document() {
        let d = doc(&["one", "two"]);
        for motion in [
            Motion::Down(50),
            Motion::Up(50),
            Motion::WordEnd,
            Motion::LineEnd,
            Motion::FileBottom,
        ] {
            let c = resolve(&d, Caret::new(1, 100), motion);
            assert!(c.line < 2);
            assert!(c.col <= line_len(&d, c.line));
        }
    }
}
