//! Full-text search, powered by the same engine as ripgrep.
//!
//! There is no index: every query walks the files the reader's folder already indexed,
//! applies the same ignore rules, skips binaries and oversized files, and scans. The
//! engine is the ripgrep crates — the `rg` binary is never needed.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use grep::matcher::Matcher;
use grep::regex::{RegexMatcher, RegexMatcherBuilder};
use grep::searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};

use crate::folder::FileEntry;

/// Enough to fill a sheet several times over without letting one broad query stall it.
pub const MAX_HITS: usize = 300;
/// A line is not useful to read if the file is a recording; skip the very large.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// One matching line, ready for the window to draw without re-reading the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// Path relative to the folder, for grouping in the sheet.
    pub rel: String,
    /// One-based line number.
    pub line: usize,
    /// The line without its terminator.
    pub text: String,
    /// Byte offsets of the first match within `text`.
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub hits: Vec<Hit>,
    /// How many files were opened in this pass.
    pub files: usize,
    /// True when the result cap stopped the search.
    pub truncated: bool,
}

/// Search the already-indexed files. This is the non-streaming form for tests and
/// callers that want the whole result at once.
pub fn search(files: &[FileEntry], pattern: &str, cancel: &AtomicBool) -> Result<Outcome, String> {
    search_with(files, pattern, cancel, &mut |_| {})
}

/// Search, handing each hit to `on_hit` as its file finishes.
///
/// The callback is where a UI streams results into a sheet. Returning from the whole
/// search is still the authority on whether it completed or was truncated.
pub fn search_with(
    files: &[FileEntry],
    pattern: &str,
    cancel: &AtomicBool,
    on_hit: &mut dyn FnMut(Hit),
) -> Result<Outcome, String> {
    let mut outcome = Outcome::default();
    if pattern.is_empty() {
        return Ok(outcome);
    }
    let matcher = RegexMatcherBuilder::new()
        .case_smart(true)
        .build(pattern)
        .map_err(|e| format!("invalid pattern · {}", regex_reason(&e)))?;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\0'))
        .build();

    for file in files {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if file.size > MAX_FILE_BYTES {
            continue;
        }
        outcome.files += 1;
        let mut sink = HitSink {
            path: file.path.clone(),
            rel: file.rel.clone(),
            matcher: &matcher,
            hits: Vec::new(),
            cancel,
        };
        // An unreadable file is not a failed search: the reader asked for text, and this
        // file has none to give. Binary detection stops a file without reporting an error.
        let _ = searcher.search_path(&matcher, &file.path, &mut sink);
        for hit in sink.hits {
            if outcome.hits.len() >= MAX_HITS {
                outcome.truncated = true;
                break;
            }
            on_hit(hit.clone());
            outcome.hits.push(hit);
        }
        if outcome.hits.len() >= MAX_HITS {
            outcome.truncated = true;
            break;
        }
    }
    Ok(outcome)
}

/// The regex crate's Display is a three-line diagram. The last line is the part worth
/// showing in a 780-point sheet.
fn regex_reason(error: &grep::regex::Error) -> String {
    let text = error.to_string();
    text.lines()
        .rev()
        .find_map(|line| line.strip_prefix("error: "))
        .unwrap_or_else(|| text.lines().next().unwrap_or("bad pattern"))
        .trim()
        .to_string()
}

struct HitSink<'a> {
    path: PathBuf,
    rel: String,
    matcher: &'a RegexMatcher,
    hits: Vec<Hit>,
    cancel: &'a AtomicBool,
}

impl Sink for HitSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let line = String::from_utf8_lossy(mat.bytes())
            .trim_end_matches(['\r', '\n'])
            .to_string();
        // The matcher works on the line as the file wrote it; offsets are byte offsets.
        let found = self
            .matcher
            .find(line.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let (start, end) = found
            .map(|m| (m.start(), m.end()))
            .unwrap_or((0, line.len()));
        self.hits.push(Hit {
            path: self.path.clone(),
            rel: self.rel.clone(),
            line: mat.line_number().unwrap_or(0) as usize,
            text: line,
            start,
            end,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder;
    use crate::testutil::TempDir;

    fn indexed(dir: &TempDir) -> Vec<FileEntry> {
        folder::open(dir.path(), "fixture", false, false)
            .expect("fixture walk")
            .files
    }

    #[test]
    fn finds_lines_across_files_with_their_columns() {
        let dir = TempDir::new("find-basic");
        dir.write("src/main.rs", "fn main() {\n    let ledger = 1;\n}\n");
        dir.write("src/review.rs", "pub fn review() {\n    ledger();\n}\n");
        let files = indexed(&dir);

        let out = search(&files, "ledger", &AtomicBool::new(false)).unwrap();
        assert_eq!(out.hits.len(), 2);
        assert_eq!(out.files, 2);
        assert_eq!(out.hits[0].rel, "src/main.rs");
        assert_eq!(out.hits[0].line, 2);
        assert_eq!(
            &out.hits[0].text[out.hits[0].start..out.hits[0].end],
            "ledger"
        );
        assert_eq!(out.hits[1].rel, "src/review.rs");
    }

    #[test]
    fn smart_case_is_lowercase_insensitive_and_uppercase_sensitive() {
        let dir = TempDir::new("find-case");
        dir.write("src/main.rs", "let ledger = 1;\nlet Ledger = 2;\n");
        let files = indexed(&dir);

        assert_eq!(
            search(&files, "ledger", &AtomicBool::new(false))
                .unwrap()
                .hits
                .len(),
            2
        );
        assert_eq!(
            search(&files, "Ledger", &AtomicBool::new(false))
                .unwrap()
                .hits
                .len(),
            1
        );
    }

    #[test]
    fn a_binary_file_is_skipped_without_failing_the_search() {
        let dir = TempDir::new("find-binary");
        dir.write("src/main.rs", "needle in text\n");
        dir.write("src/blob.rs", b"needle\0binary");
        let files = indexed(&dir);

        let out = search(&files, "needle", &AtomicBool::new(false)).unwrap();
        assert_eq!(out.hits.len(), 1);
        assert_eq!(out.hits[0].rel, "src/main.rs");
    }

    #[test]
    fn the_result_cap_is_explicit() {
        let dir = TempDir::new("find-cap");
        let mut text = String::new();
        for i in 0..MAX_HITS + 5 {
            text.push_str(&format!("needle {i}\n"));
        }
        dir.write("src/many.rs", text);
        let files = indexed(&dir);

        let out = search(&files, "needle", &AtomicBool::new(false)).unwrap();
        assert_eq!(out.hits.len(), MAX_HITS);
        assert!(out.truncated);
    }

    #[test]
    fn an_invalid_pattern_is_a_message_not_a_panic() {
        let dir = TempDir::new("find-invalid");
        dir.write("src/main.rs", "fn main() {}\n");
        let files = indexed(&dir);
        let err = search(&files, "[", &AtomicBool::new(false)).unwrap_err();
        assert!(err.contains("invalid pattern"), "{err}");
    }

    #[test]
    fn an_empty_query_does_nothing() {
        let dir = TempDir::new("find-empty");
        dir.write("src/main.rs", "fn main() {}\n");
        let files = indexed(&dir);
        let out = search(&files, "", &AtomicBool::new(false)).unwrap();
        assert!(out.hits.is_empty());
        assert_eq!(out.files, 0);
    }

    #[test]
    fn cancellation_stops_before_any_file() {
        let dir = TempDir::new("find-cancel");
        dir.write("src/main.rs", "needle\n");
        let files = indexed(&dir);
        let cancel = AtomicBool::new(true);
        let out = search(&files, "needle", &cancel).unwrap();
        assert!(out.hits.is_empty());
        assert_eq!(out.files, 0);
    }
}
