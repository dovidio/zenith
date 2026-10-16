//! Rendering Markdown as a document rather than as source.
//!
//! Zenith's reading pane is line-based: one source line, one row, which is what lets the
//! cursor, the search and every motion work on a file's real coordinates. Markdown is
//! rendered *within* that model rather than by replacing it. Each line is scanned into
//! spans with roles — a heading, strong text, inline code, a bullet — and the markers
//! themselves are dropped, so `## Notes` becomes a heading called `Notes` in place of the
//! line that said `## Notes`.
//!
//! Nothing here knows what a font is, or a colour, or egui. It is a scanner, it is pure,
//! and it is tested, because the interesting failures are all in the parsing: a `*` inside
//! `snake_case`, a fence inside a fence, a bullet after a blank line.
//!
//! What it deliberately does not do: tables, footnotes, reference links, setext headings
//! (`===` under a line) and nested lists beyond their indentation. A reader sees the text
//! of a table as a paragraph rather than as a grid, which is a loss, and the one to fix
//! next.

use crate::syntax::Line;

/// What a run of text is, which is all the renderer needs to choose a face for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Body,
    /// `**strong**`
    Strong,
    /// `*emphasised*`
    Emphasis,
    /// `` `inline code` ``
    Code,
    /// A link's text; the destination is dropped.
    Link,
    /// A list's bullet or number.
    Marker,
    /// A line inside a fenced block.
    CodeLine,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub text: String,
    pub role: Role,
}

/// What kind of line this is, which decides how it is framed on the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Body,
    Blank,
    /// A line inside a fenced code block.
    CodeLine,
    Heading(u8),
    Quote,
    /// A list item, with its depth.
    Item(usize),
    /// The ``` fence itself: nothing is drawn, the block's edge is.
    Fence,
    /// A horizontal rule: drawn as a hairline, not as text.
    Rule,
    /// The front matter at the top of the file: metadata for a build, not prose for a
    /// reader. The lines are still there — searching and every motion still see the file
    /// as it is written — but nothing of them is drawn.
    Meta,
}

#[derive(Clone, Debug)]
pub struct MdLine {
    pub kind: Kind,
    pub spans: Vec<Span>,
}

impl MdLine {
    /// The line as it will read on the page, markers removed. Used by the tests, and by
    /// anything that needs to know what the reader will actually see.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }


    /// Is this line inside a fenced block, or part of a quote? The painter fills those
    /// bands before the text goes down, so it needs to know the run of them.
    pub fn in_band(&self) -> bool {
        matches!(self.kind, Kind::Fence | Kind::Quote | Kind::CodeLine)
    }
}

/// Render a parsed document line by line.
pub fn render(lines: &[Line]) -> Vec<MdLine> {
    let mut out = Vec::with_capacity(lines.len());
    let mut fence: Option<char> = None;
    let meta = front_matter(lines);
    for (index, line) in lines.iter().enumerate() {
        let text = line.text();
        let source = text.trim_end();

        // The front matter is a build's business. It is not drawn, and it does not
        // interrupt the document: the reader opens a page and sees a page.
        if meta.is_some_and(|(a, b)| index >= a && index <= b) {
            out.push(MdLine {
                kind: Kind::Meta,
                spans: Vec::new(),
            });
            continue;
        }

        // Inside a fence, nothing is markdown: not a bullet, not a heading, not emphasis.
        if let Some(fence_char) = fence {
            if is_fence(source, fence_char) {
                fence = None;
                out.push(MdLine {
                    kind: Kind::Fence,
                    spans: Vec::new(),
                });
                continue;
            }
            out.push(MdLine {
                kind: Kind::CodeLine,
                spans: vec![Span {
                    text: source.to_string(),
                    role: Role::CodeLine,
                }],
            });
            continue;
        }

        // An ATX heading: `#` to `######`, then the text.
        if let Some((level, rest)) = heading(source) {
            out.push(MdLine {
                kind: Kind::Heading(level),
                spans: inline(rest.trim()),
            });
            continue;
        }

        // A horizontal rule: three or more of one mark, nothing else.
        if is_rule(source) {
            out.push(MdLine {
                kind: Kind::Rule,
                spans: Vec::new(),
            });
            continue;
        }

        // A fence, opening a code block.
        if let Some(c) = fence_open(source) {
            fence = Some(c);
            out.push(MdLine {
                kind: Kind::Fence,
                spans: Vec::new(),
            });
            continue;
        }

        // A quote, however deeply nested; `>` markers are the frame, not the words.
        if source.trim_start().starts_with('>') {
            let rest = strip_quotes(source);
            out.push(MdLine {
                kind: Kind::Quote,
                spans: inline(rest.trim()),
            });
            continue;
        }

        // A list item, numbered or bulleted.
        // Nesting is in the line's indentation, which the parser stripped out of the
        // text, so it is passed in rather than counted from spaces that are no longer there.
        if let Some((depth, marker, rest)) = item(line.indent, source) {
            let mut spans = vec![Span {
                text: marker,
                role: Role::Marker,
            }];
            spans.extend(inline(rest.trim()));
            out.push(MdLine {
                kind: Kind::Item(depth),
                spans,
            });
            continue;
        }

        if source.trim().is_empty() {
            out.push(MdLine {
                kind: Kind::Blank,
                spans: Vec::new(),
            });
            continue;
        }

        out.push(MdLine {
            kind: Kind::Body,
            spans: inline(source.trim()),
        });
    }
    out
}

/// The front matter at the top of a document: the lines from an opening `---` or `+++`
/// to its match, if that block is closed.
///
/// Only at the very top, and only when closed. A file that begins with a horizontal rule
/// and never says `---` again is a rule, not metadata, and a document whose metadata is
/// never closed is better read as the text it is than silently swallowed.
fn front_matter(lines: &[Line]) -> Option<(usize, usize)> {
    let first = lines.iter().position(|l| !l.text().trim().is_empty())?;
    let mark = lines[first].text();
    let mark = mark.trim();
    if mark != "---" && mark != "+++" {
        return None;
    }
    let close = lines
        .iter()
        .enumerate()
        .skip(first + 1)
        .take(400)
        .find(|(_, l)| l.text().trim() == mark)
        .map(|(i, _)| i)?;
    Some((first, close))
}

fn heading(source: &str) -> Option<(u8, &str)> {
    let t = source.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &t[hashes..];
    // `#hashtag` is not a heading: there must be a space, or nothing at all.
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    // A closing run of `#` is decoration.
    let rest = rest.trim().trim_end_matches('#').trim_end();
    Some((hashes as u8, rest))
}

fn is_rule(source: &str) -> bool {
    let t = source.trim();
    if t.len() < 3 {
        return false;
    }
    for mark in ['-', '*', '_'] {
        if t.chars().all(|c| c == mark || c == ' ') && t.chars().filter(|c| *c == mark).count() >= 3 {
            return true;
        }
    }
    false
}

fn fence_open(source: &str) -> Option<char> {
    let t = source.trim_start();
    ['`', '~'].into_iter().find(|c| {
        t.chars().take_while(|x| x == c).count() >= 3
    })
}

fn is_fence(source: &str, c: char) -> bool {
    let t = source.trim();
    t.len() >= 3 && t.chars().all(|x| x == c)
}

fn strip_quotes(source: &str) -> &str {
    let mut rest = source.trim_start();
    while let Some(r) = rest.strip_prefix('>') {
        rest = r.trim_start();
    }
    rest
}

/// A list item: its nesting depth, the marker to draw, and the text.
fn item(indent: usize, source: &str) -> Option<(usize, String, &str)> {
    let depth = indent / 2;
    let t = source.trim_start();
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(bullet) {
            return Some((depth, "• ".to_string(), rest));
        }
    }
    // `1. ` or `12) `
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let after = &t[digits..];
        if let Some(rest) = after.strip_prefix(". ").or_else(|| after.strip_prefix(") ")) {
            return Some((depth, format!("{}. ", &t[..digits]), rest));
        }
    }
    None
}

/// Split a line of prose into runs, dropping the emphasis and code markers.
///
/// The markers are *removed* rather than demoted here — that is the whole difference
/// between rendering a document and setting its source. What they wrapped keeps its own
/// role so the renderer can give `**this**` more ink than `this`.
fn inline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let mut i = 0usize;

    let flush = |buf: &mut String, role: Role, spans: &mut Vec<Span>| {
        if !buf.is_empty() {
            spans.push(Span {
                text: std::mem::take(buf),
                role,
            });
        }
    };

    while i < chars.len() {
        let c = chars[i];

        // Inline code wins over everything: `*not emphasis*` inside backticks is text.
        if c == '`' {
            let run = chars[i..].iter().take_while(|x| **x == '`').count();
            if let Some(end) = find_run(&chars, i + run, '`', run) {
                flush(&mut buf, Role::Body, &mut spans);
                spans.push(Span {
                    text: chars[i + run..end].iter().collect(),
                    role: Role::Code,
                });
                i = end + run;
                continue;
            }
        }

        // A link: keep the words, drop the destination.
        if c == '[' {
            if let Some((label, end)) = link(&chars, i) {
                flush(&mut buf, Role::Body, &mut spans);
                spans.push(Span {
                    text: label,
                    role: Role::Link,
                });
                i = end;
                continue;
            }
        }

        // An image: its alt text is what a reader can be given.
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            if let Some((label, end)) = link(&chars, i + 1) {
                flush(&mut buf, Role::Body, &mut spans);
                // Square brackets rather than angle quotes: they exist in every face on
                // every machine, and a description in italics needs no exotic glyph to
                // read as one.
                spans.push(Span {
                    text: format!("[{label}]"),
                    role: Role::Emphasis,
                });
                i = end;
                continue;
            }
        }

        for (mark, wide, role) in [
            ('*', true, Role::Strong),
            ('*', false, Role::Emphasis),
            ('_', true, Role::Strong),
            ('_', false, Role::Emphasis),
        ] {
            if c != mark {
                continue;
            }
            // An underscore between word characters is part of a word: `snake_case_name`
            // is an identifier, not emphasis, and a document about code is full of them.
            // This is the one CommonMark rule worth implementing exactly.
            if mark == '_' && i > 0 && chars[i - 1].is_alphanumeric() {
                continue;
            }
            let run = chars[i..].iter().take_while(|x| **x == mark).count();
            if (run >= 2) != wide {
                continue;
            }
            let width = if wide { 2 } else { 1 };
            if run < width {
                continue;
            }
            let closer = |j: usize| {
                mark != '_' || j + width >= chars.len() || !chars[j + width].is_alphanumeric()
            };
            if let Some(end) = find_run_where(&chars, i + run, mark, width, closer) {
                flush(&mut buf, Role::Body, &mut spans);
                let inner: String = chars[i + run..end].iter().collect();
                // The inside may hold further markup, but emphasis inside emphasis is
                // rare enough that treating it as text is the honest simplification.
                spans.push(Span {
                    text: inner,
                    role,
                });
                i = end + width;
                break;
            }
        }
        if i < chars.len() && chars[i] == c {
            // Nothing matched: the character is ordinary text.
            buf.push(c);
            i += 1;
        }
    }
    flush(&mut buf, Role::Body, &mut spans);

    // `[text](url)` and `**` runs that never closed leave stray marks behind; the caller
    // would rather see the source than a document with holes in it.
    if spans.is_empty() {
        spans.push(Span {
            text: text.to_string(),
            role: Role::Body,
        });
    }
    spans
}

/// Find the end of a run of `mark` of at least `width`, starting at `from`.
fn find_run(chars: &[char], from: usize, mark: char, width: usize) -> Option<usize> {
    find_run_where(chars, from, mark, width, |_| true)
}

/// The same, for closers that have to satisfy a rule of their own — an underscore may not
/// close in the middle of a word.
fn find_run_where(
    chars: &[char],
    from: usize,
    mark: char,
    width: usize,
    ok: impl Fn(usize) -> bool,
) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == mark {
            let run = chars[i..].iter().take_while(|x| **x == mark).count();
            if run >= width && ok(i) {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// `[label](destination)` at `start`, returning the label and the index past it.
fn link(chars: &[char], start: usize) -> Option<(String, usize)> {
    if chars.get(start) != Some(&'[') {
        return None;
    }
    let close = chars[start + 1..].iter().position(|c| *c == ']')? + start + 1;
    let after = close + 1;
    if chars.get(after) != Some(&'(') {
        return None;
    }
    let end = chars[after + 1..].iter().position(|c| *c == ')')? + after + 1;
    Some((chars[start + 1..close].iter().collect(), end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Line;

    fn doc(text: &str) -> Vec<MdLine> {
        let lines: Vec<Line> = text.lines().map(Line::plain).collect();
        render(&lines)
    }

    fn kinds(text: &str) -> Vec<Kind> {
        doc(text).iter().map(|l| l.kind).collect()
    }

    #[test]
    fn front_matter_is_metadata_and_is_not_drawn() {
        let out = doc("---\ntitle: \"A post\"\ntags: [\"x\"]\n---\n\nThe text.\n");
        assert_eq!(out[0].kind, Kind::Meta, "the opening delimiter");
        assert_eq!(out[1].kind, Kind::Meta);
        assert_eq!(out[2].kind, Kind::Meta);
        assert_eq!(out[3].kind, Kind::Meta, "the closing delimiter");
        assert_eq!(out[4].kind, Kind::Blank);
        assert_eq!(out[5].kind, Kind::Body);
        assert_eq!(out[5].text(), "The text.");
        // TOML front matter too.
        assert_eq!(doc("+++\ntitle = \"x\"\n+++\nbody\n")[0].kind, Kind::Meta);
    }

    #[test]
    fn a_rule_at_the_top_is_still_a_rule() {
        // One `---` and no second: not metadata, and not to be swallowed.
        let out = doc("---\n\nA paragraph after a rule.\n");
        assert_eq!(out[0].kind, Kind::Rule);
        assert_eq!(out[2].kind, Kind::Body);
        assert_eq!(out[2].text(), "A paragraph after a rule.");
    }

    #[test]
    fn headings_lose_their_hashes() {
        let out = doc("## Notes\n");
        assert_eq!(out[0].kind, Kind::Heading(2));
        assert_eq!(out[0].text(), "Notes");
        // A `#` with no space is a hashtag, not a heading.
        assert_eq!(kinds("#hashtag\n"), vec![Kind::Body]);
        // A closing run of hashes is decoration.
        assert_eq!(doc("## Notes ##")[0].text(), "Notes");
        assert_eq!(kinds("#######\n"), vec![Kind::Body]);
    }

    #[test]
    fn emphasis_markers_are_removed_and_their_words_keep_a_role() {
        let out = doc("a **bold** word and *slanted* and `code`\n");
        let spans = &out[0].spans;
        assert_eq!(out[0].text(), "a bold word and slanted and code");
        assert!(spans.iter().any(|s| s.role == Role::Strong && s.text == "bold"));
        assert!(spans.iter().any(|s| s.role == Role::Emphasis && s.text == "slanted"));
        assert!(spans.iter().any(|s| s.role == Role::Code && s.text == "code"));
    }

    #[test]
    fn underscores_in_identifiers_are_not_emphasis() {
        // A document that mentions snake_case_name should not italicise "case".
        let out = doc("call snake_case_name here\n");
        assert_eq!(out[0].text(), "call snake_case_name here");
        assert!(out[0].spans.iter().all(|s| s.role == Role::Body));
    }

    #[test]
    fn emphasis_inside_code_is_text() {
        let out = doc("use `*args` in python\n");
        assert_eq!(out[0].text(), "use *args in python");
        assert!(out[0].spans.iter().any(|s| s.role == Role::Code && s.text == "*args"));
    }

    #[test]
    fn links_keep_words_and_drop_destinations() {
        let out = doc("see [the docs](https://example.com/x) now\n");
        assert_eq!(out[0].text(), "see the docs now");
        assert!(out[0].spans.iter().any(|s| s.role == Role::Link && s.text == "the docs"));
        // An image becomes its alt text, bracketed — in brackets every face has.
        let img = doc("![a picture](shot.png)\n");
        assert_eq!(img[0].text(), "[a picture]");
    }

    #[test]
    fn bullets_become_bullets_and_numbers_keep_their_number() {
        let out = doc("- one\n- two\n  1. nested\n");
        assert_eq!(out[0].kind, Kind::Item(0));
        assert_eq!(out[0].spans[0].role, Role::Marker);
        assert_eq!(out[0].text(), "• one");
        assert_eq!(out[1].text(), "• two");
        assert_eq!(out[2].kind, Kind::Item(1));
        assert_eq!(out[2].text(), "1. nested");
        // `*` as a bullet is a bullet, not emphasis.
        assert_eq!(doc("* item\n")[0].kind, Kind::Item(0));
    }

    #[test]
    fn quotes_lose_their_arrows_however_deep() {
        let out = doc("> quoted\n>> deeply\n");
        assert_eq!(out[0].kind, Kind::Quote);
        assert_eq!(out[0].text(), "quoted");
        assert_eq!(out[1].kind, Kind::Quote);
        assert_eq!(out[1].text(), "deeply");
    }

    #[test]
    fn fenced_code_is_left_exactly_alone() {
        let out = doc("text\n```rust\nlet x = *p;  // not emphasis\n# not a heading\n```\nmore\n");
        assert_eq!(out[0].kind, Kind::Body);
        assert_eq!(out[1].kind, Kind::Fence);
        assert_eq!(out[2].kind, Kind::CodeLine);
        assert_eq!(out[2].text(), "let x = *p;  // not emphasis");
        assert_eq!(out[3].kind, Kind::CodeLine);
        assert_eq!(out[3].text(), "# not a heading");
        assert_eq!(out[4].kind, Kind::Fence);
        assert_eq!(out[5].kind, Kind::Body);
        // A tilde fence works too.
        let t = doc("~~~\nx\n~~~\n");
        assert_eq!(t[0].kind, Kind::Fence);
        assert_eq!(t[1].kind, Kind::CodeLine);
    }

    #[test]
    fn rules_are_rules_and_not_list_items() {
        assert_eq!(kinds("---\n"), vec![Kind::Rule]);
        assert_eq!(kinds("***\n"), vec![Kind::Rule]);
        assert_eq!(kinds("___\n"), vec![Kind::Rule]);
        // Two marks are not a rule.
        assert_eq!(kinds("--\n"), vec![Kind::Body]);
    }

    #[test]
    fn an_unclosed_marker_does_not_eat_the_line() {
        // A lone asterisk must survive as text rather than swallowing the rest.
        let out = doc("2 * 3 = 6\n");
        assert_eq!(out[0].text(), "2 * 3 = 6");
    }
}
