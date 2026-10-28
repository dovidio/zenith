//! Where three coordinate systems meet.
//!
//! * A **caret** is a line and a *character* column — and zenith's column excludes the
//!   indentation, because indentation is drawn in the gutter and never set as text.
//! * A **language server** counts `(line, character)` in *UTF-16 code units from the
//!   first byte of the line*, indentation included, with end-exclusive ranges.
//! * A **file** is bytes, and its indentation may be tabs.
//!
//! Each of those mismatches is a silent bug — a jump that lands one word over, a link
//! that opens nothing, a definition that works everywhere except in code that uses
//! tabs. So the arithmetic lives here, it is pure, and it is tested; nothing else in
//! the program does arithmetic on columns.

use std::path::{Path, PathBuf};

use crate::syntax::Line;

/// The characters of a line as the *file* wrote them: tokens, with their tabs intact.
fn chars(line: &Line) -> impl Iterator<Item = char> + '_ {
    line.tokens.iter().flat_map(|t| t.text.chars())
}

/// A caret column → the position a language server expects.
pub fn position(line: &Line, col: usize) -> u32 {
    // Indentation is spaces and tabs, which are ASCII: one byte, one UTF-16 unit.
    let mut units = line.indent_bytes as u32;
    let mut left = col;
    for ch in chars(line) {
        if left == 0 {
            break;
        }
        units += ch.len_utf16() as u32;
        left -= 1;
    }
    units
}

/// A position from a language server → a caret column (indentation excluded).
///
/// A position that lands inside an indentation, or past the end of the line, is not an
/// error: it clamps. A definition whose range starts at the indentation is a definition
/// of the line, and a reader should still be taken to it.
pub fn column(line: &Line, unit: u32) -> usize {
    let mut left = match unit.checked_sub(line.indent_bytes as u32) {
        Some(n) => n,
        None => return 0,
    };
    let mut cols = 0usize;
    for ch in chars(line) {
        let width = ch.len_utf16() as u32;
        if width > left {
            // Inside a surrogate pair: the half is not a character, so take the whole
            // one. Landing before a lone emoji is better than landing past it.
            break;
        }
        left -= width;
        cols += 1;
        if left == 0 {
            break;
        }
    }
    cols
}

/// A path as a `file:` URI. Percent-encoding is not decoration: the paths people read
/// code in have spaces in them ("Application Support", "My Projects"), and a URI with a
/// raw space in it is one no server will parse.
pub fn to_uri(path: &Path) -> String {
    // A `file:` URI is absolute or it is not a URI. A relative path here becomes
    // `file://src/main.rs`, and a server answers that with "url is not a file" — the
    // least useful sentence in the protocol.
    let absolute = path
        .canonicalize()
        .or_else(|_| {
            if path.is_absolute() {
                Ok(path.to_path_buf())
            } else {
                std::env::current_dir().map(|dir| dir.join(path))
            }
        })
        .unwrap_or_else(|_| path.to_path_buf());
    let mut out = String::from("file://");
    for b in absolute.to_string_lossy().bytes() {
        match b {
            b'/' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A `file:` URI back to a path on this machine.
///
/// Returns `None` for a URI that is not a local file — `untitled:`, a `jdt:` archive, a
/// server's own virtual document. Navigation cannot follow those, and pretending
/// otherwise would put a path on screen that does not exist.
pub fn from_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://localhost/x` — the authority is allowed to be empty or the local host.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    Some(PathBuf::from(percent_decode(rest)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Line;

    fn line(src: &str) -> Line {
        Line::plain(src)
    }

    #[test]
    fn a_caret_column_is_measured_from_the_end_of_the_indentation() {
        let l = line("    let x = 1;");
        assert_eq!(l.indent, 4);
        assert_eq!(position(&l, 0), 4, "the first character is the fifth byte");
        assert_eq!(position(&l, 4), 8, "caret on `x`");
    }

    #[test]
    fn a_tab_indents_four_columns_and_one_byte() {
        // Go is indented with tabs, and the two counts disagree. The server means bytes.
        let l = line("\tfoo");
        assert_eq!(l.indent, 4);
        assert_eq!(l.indent_bytes, 1);
        assert_eq!(position(&l, 0), 1);
        assert_eq!(column(&l, 1), 0);
        assert_eq!(column(&l, 4), 3, "the end of a three-letter line");
    }

    #[test]
    fn a_position_is_counted_in_utf16_units_not_characters() {
        let l = line("    let s = \"🙂\"; odd");
        let before = l.text().chars().take_while(|c| *c != '🙂').count();
        // `🙂` is one character and two UTF-16 units, so the character after it sits
        // one unit further along than its character index suggests.
        let after = before + 1;
        assert_eq!(
            position(&l, after),
            l.indent_bytes as u32 + before as u32 + 2
        );
        assert_eq!(column(&l, position(&l, after)), after);
    }

    #[test]
    fn the_round_trip_holds_for_every_column() {
        for src in [
            "x",
            "    let x = 1;",
            "\t\tif err != nil {",
            "    let s = \"héllo wörld\";",
            "  🙂 then 🙂",
        ] {
            let l = line(src);
            let width = l.text().chars().count();
            for col in 0..=width {
                let unit = position(&l, col);
                assert_eq!(column(&l, unit), col, "{src:?} column {col}");
            }
        }
    }

    #[test]
    fn a_position_before_the_line_belongs_to_its_indentation() {
        let l = line("        deep");
        assert_eq!(column(&l, 0), 0);
        assert_eq!(column(&l, 3), 0);
        assert_eq!(column(&l, 8), 0);
        assert_eq!(column(&l, 9), 1);
    }

    #[test]
    fn a_uri_survives_the_paths_people_actually_read_code_in() {
        let p = Path::new("/Users/umberto/My Projects/ünïcode/lib.rs");
        let uri = to_uri(p);
        assert_eq!(
            uri,
            "file:///Users/umberto/My%20Projects/%C3%BCn%C3%AFcode/lib.rs"
        );
        assert_eq!(from_uri(&uri).as_deref(), Some(p));
    }

    #[test]
    fn a_relative_path_still_becomes_an_absolute_uri() {
        let uri = to_uri(Path::new("src/main.rs"));
        assert!(uri.starts_with("file:///"), "{uri}");
        assert!(!uri.contains("file://src"), "{uri}");
        assert!(uri.ends_with("/src/main.rs"), "{uri}");
    }

    #[test]
    fn only_local_uris_become_paths() {
        assert!(from_uri("untitled:Untitled-1").is_none());
        assert!(from_uri("jdt://contents/x.class").is_none());
        assert!(from_uri("file:///tmp/a.rs").is_some());
    }

    #[test]
    fn a_fragment_does_not_become_part_of_the_name() {
        assert_eq!(
            from_uri("file:///tmp/a.rs#L12").as_deref(),
            Some(Path::new("/tmp/a.rs"))
        );
    }
}
