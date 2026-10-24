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
//! Tables are the exception to "one line, one row", and they are handled by looking at the
//! block rather than the line: a header row, a delimiter row, then body rows, with the
//! column widths measured across all of them and every cell padded to its column. The page
//! is drawn in the code face, so the padding actually aligns; a table too wide for the
//! measure wraps like any other text, which is the price of not owning the font metrics
//! here, and turning wrapping off (`w`) shows it whole.
//!
//! What it still does not do: footnotes, reference links, setext headings (`===` under a
//! line), indented code blocks, and nesting inside list items beyond their indentation.

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
    /// `~~struck through~~`
    Strike,
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
    /// A table's header row: the cells, aligned, in the stronger ink, with a hairline
    /// under them.
    TableHead,
    /// The `|---|---|` line itself: it decides the columns and is not drawn. The hairline
    /// the reader sees is this row's.
    TableRule,
    /// A table's body row.
    TableRow,
    /// The front matter at the top of the file: metadata for a build, not prose for a
    /// reader. The lines are still there — searching and every motion still see the file
    /// as it is written — but nothing of them is drawn.
    Meta,
}

/// How a table column is aligned, read from its delimiter cell: `:--`, `--:`, `:-:`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Right,
    Centre,
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
    // A table is the one thing here that is read as a block; this is the line after it.
    let mut consumed = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if index < consumed {
            continue;
        }
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

        // A table: a row of cells, then a delimiter row with the same number of columns.
        // Every cell is padded to its column, so the code face lines the grid up.
        if let Some(align) = table_at(lines, index) {
            let (rows, end) = table_block(lines, index);
            let widths = column_widths(&rows);
            for (row_index, row) in rows.iter().enumerate() {
                let kind = match row_index {
                    0 => Kind::TableHead,
                    1 => Kind::TableRule,
                    _ => Kind::TableRow,
                };
                // The dashes are how the grid is described, not something to read: the
                // reader draws their hairline, and this line says nothing.
                let spans = if kind == Kind::TableRule {
                    Vec::new()
                } else {
                    table_spans(row, &align, &widths, row_index == 0)
                };
                out.push(MdLine { kind, spans });
            }
            consumed = end;
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


/// The cells of a table row, or `None` if the line is not one.
///
/// A row needs a pipe somewhere. One leading and one trailing pipe are dropped, then the
/// rest is split on the pipes that are not escaped, and every cell is trimmed.
fn table_row(source: &str) -> Option<Vec<String>> {
    let t = source.trim();
    if !t.contains('|') {
        return None;
    }
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                chars.next();
                cell.push('|');
            }
            '|' => {
                cells.push(std::mem::take(&mut cell).trim().to_string());
            }
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_string());
    Some(cells)
}

/// The alignment row: `|---|---|`, `|:--|--:|`, `|:-:|`.
fn table_delimiter(source: &str) -> Option<Vec<Align>> {
    let cells = table_row(source)?;
    let mut out = Vec::with_capacity(cells.len());
    for cell in &cells {
        let t = cell.trim();
        if t.is_empty() || !t.chars().all(|c| c == '-' || c == ':') {
            return None;
        }
        let left = t.starts_with(':');
        let right = t.ends_with(':');
        let dashes = t.trim_start_matches(':').trim_end_matches(':').chars().count();
        if dashes == 0 {
            return None;
        }
        out.push(match (left, right) {
            (true, true) => Align::Centre,
            (false, true) => Align::Right,
            _ => Align::Left,
        });
    }
    Some(out)
}

/// Is a table here? The header must be a row, the line under it a delimiter row of the
/// same width, and there must be at least one column.
fn table_at(lines: &[Line], index: usize) -> Option<Vec<Align>> {
    let head = table_row(&lines.get(index)?.text())?;
    let align = table_delimiter(&lines.get(index + 1)?.text())?;
    if head.is_empty() || head.len() != align.len() {
        return None;
    }
    Some(align)
}

/// Every row of the table, and the index of the line after it. The delimiter row is
/// included: it is what tells the renderer to stop drawing text for that line.
fn table_block(lines: &[Line], start: usize) -> (Vec<Vec<String>>, usize) {
    let mut rows = Vec::new();
    let mut i = start;
    while let Some(row) = lines.get(i).and_then(|l| table_row(&l.text())) {
        if row.is_empty() {
            break;
        }
        rows.push(row);
        i += 1;
    }
    (rows, i)
}

/// How wide each column is, in characters, across the whole table. The delimiter row is
/// not measured: its dashes say how to align a column, not how wide it is.
fn column_widths(rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if i == 1 {
            continue;
        }
        for (c, cell) in row.iter().enumerate() {
            let w = cell.chars().count();
            match widths.get_mut(c) {
                Some(slot) => *slot = (*slot).max(w),
                None => widths.push(w),
            }
        }
    }
    widths
}

/// One row, every cell padded to its column so the code face aligns the grid.
///
/// Cells are separated by two spaces rather than by a drawn bar: a bar is a glyph some
/// font somewhere will not have, and the padding already does the work.
fn table_spans(row: &[String], align: &[Align], widths: &[usize], head: bool) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::with_capacity(widths.len() * 3);
    for (c, width) in widths.iter().enumerate() {
        if c > 0 {
            spans.push(Span {
                text: "  ".to_string(),
                role: Role::Body,
            });
        }
        // A cell is a line of prose in miniature: `` `code` `` and `**strong**` work
        // inside it, because a table about a program is mostly made of both.
        let mut cell = inline(row.get(c).map(String::as_str).unwrap_or(""));
        if head {
            for span in &mut cell {
                if span.role == Role::Body {
                    span.role = Role::Strong;
                }
            }
        }
        // Padding is measured on what the cell will draw, not on its source: `**bold**`
        // is four characters narrower than it looks.
        let drawn: usize = cell.iter().map(|s| s.text.chars().count()).sum();
        let pad = width.saturating_sub(drawn);
        let (before, after) = match align.get(c).copied().unwrap_or(Align::Left) {
            Align::Left => (0, pad),
            Align::Right => (pad, 0),
            Align::Centre => (pad / 2, pad - pad / 2),
        };
        if before > 0 {
            spans.push(Span {
                text: " ".repeat(before),
                role: Role::Body,
            });
        }
        spans.extend(cell);
        if after > 0 {
            spans.push(Span {
                text: " ".repeat(after),
                role: Role::Body,
            });
        }
    }
    // The last column's padding is there to align nothing; in the text it is noise.
    while let Some(last) = spans.last_mut() {
        let trimmed = last.text.trim_end().to_string();
        if trimmed.is_empty() && !last.text.is_empty() {
            spans.pop();
            continue;
        }
        last.text = trimmed;
        break;
    }
    spans
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
            // A task list keeps its box visible: an item that says it is done should say
            // so, and `[ ]` is a glyph every face has.
            for (box_, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                if let Some(rest) = rest.strip_prefix(box_) {
                    let _ = done;
                    let mark = if done { "[x] " } else { "[ ] " };
                    return Some((depth, mark.to_string(), rest));
                }
            }
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
            ('~', true, Role::Strike),
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
    fn a_table_lines_its_columns_up() {
        let out = doc("| Name | Value |\n|------|-------|\n| a | 12 |\n| longer | 7 |\n");
        assert_eq!(out[0].kind, Kind::TableHead);
        assert_eq!(out[1].kind, Kind::TableRule, "the dashes are not text");
        assert_eq!(out[2].kind, Kind::TableRow);
        assert_eq!(out[0].text(), "Name    Value");
        // Every cell starts where it started in the header.
        assert_eq!(out[2].text(), "a       12");
        assert_eq!(out[3].text(), "longer  7");
        // The point of the padding: the second column begins in the same place in every
        // row, or it is not a table, it is a paragraph with pipes in it.
        let col_of = |line: &MdLine, cell: &str| line.text().find(cell);
        assert_eq!(col_of(&out[0], "Value"), col_of(&out[2], "12"));
        assert_eq!(col_of(&out[0], "Value"), col_of(&out[3], "7"));
    }

    #[test]
    fn a_table_aligns_by_its_delimiters() {
        let out = doc("| left | right | middle |\n|:--|--:|:-:|\n| a | 1 | b |\n");
        let row = out[2].text();
        assert!(row.starts_with("a "), "left column starts at the edge: {row:?}");
        assert!(row.contains("   1"), "the right column is pushed right: {row:?}");
    }

    #[test]
    fn a_ragged_row_is_padded_not_dropped() {
        let out = doc("| a | b | c |\n|---|---|---|\n| only |\n");
        assert_eq!(out[2].text().trim_end(), "only");
    }

    #[test]
    fn a_line_with_pipes_is_not_a_table_without_its_dashes() {
        let out = doc("| not | a table |\n\njust prose | with a pipe\n");
        assert_eq!(out[0].kind, Kind::Body);
        assert_eq!(out[2].kind, Kind::Body);
        // A delimiter row of the wrong width is not a table either.
        let out = doc("| a | b |\n|---|\n");
        assert_eq!(out[0].kind, Kind::Body);
    }

    #[test]
    fn a_table_ends_at_the_first_line_that_is_not_a_row() {
        let out = doc("| a |\n|---|\n| 1 |\n\nAfter.\n");
        assert_eq!(out[2].kind, Kind::TableRow);
        assert_eq!(out[3].kind, Kind::Blank);
        assert_eq!(out[4].text(), "After.");
    }

    #[test]
    fn task_lists_keep_their_boxes() {
        let out = doc("- [ ] not yet\n- [x] done\n- plain\n");
        assert_eq!(out[0].text(), "[ ] not yet");
        assert_eq!(out[1].text(), "[x] done");
        assert_eq!(out[2].text(), "• plain");
    }

    #[test]
    fn strikethrough_is_a_role_not_literal_tildes() {
        let out = doc("this is ~~gone~~ and this ~is not~\n");
        assert_eq!(out[0].text(), "this is gone and this ~is not~");
        assert!(out[0].spans.iter().any(|s| s.role == Role::Strike && s.text == "gone"));
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
