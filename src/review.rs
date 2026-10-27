//! Reviewing a branch without turning the page into a patch.
//!
//! Git is the source of truth; zenith only reads it. Every command here is a diff or a
//! show, with optional locks disabled, so opening a review never touches the index or
//! the working tree.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::folder::{is_noise_rel, FileEntry};

/// What is the current file being compared against?
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Base {
    #[default]
    Working,
    Staged,
    LastCommit,
    Ref(String),
}

impl Base {
    pub fn label(&self) -> String {
        match self {
            Self::Working => "working tree".into(),
            Self::Staged => "staged".into(),
            Self::LastCommit => "last commit".into(),
            Self::Ref(reference) => reference.clone(),
        }
    }

    pub fn is_working(&self) -> bool {
        matches!(self, Self::Working)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl Status {
    pub fn mark(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "changed",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileChange {
    pub path: PathBuf,
    pub rel: String,
    pub status: Status,
    /// The path in the base revision, when the file moved.
    pub old_rel: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Review {
    pub root: PathBuf,
    pub base: Base,
    pub branch: String,
    /// The revision the old side comes from.
    pub old_ref: Option<String>,
    /// The revision the new side comes from. `None` means the working tree.
    pub new_ref: Option<String>,
    /// True when the new side is the index.
    pub staged: bool,
    pub files: Vec<FileChange>,
    /// Changed paths that were deliberately left out (noise, or unreadable files).
    pub skipped: usize,
}

impl Review {
    pub fn file_text(&self, file: &FileChange) -> Result<String, String> {
        let rel = file.rel.as_str();
        let bytes = if file.status == Status::Deleted {
            let old = self
                .old_ref
                .as_deref()
                .ok_or("this review has no base revision")?;
            git_bytes(&self.root, &["show", &format!("{old}:{rel}")])?
        } else if self.staged {
            git_bytes(&self.root, &["show", &format!(":{rel}")])?
        } else if let Some(new_ref) = &self.new_ref {
            git_bytes(&self.root, &["show", &format!("{new_ref}:{rel}")])?
        } else {
            std::fs::read(self.root.join(&file.rel)).map_err(|e| e.to_string())?
        };

        if bytes.contains(&0) {
            return Err("binary file — zenith only reviews text".into());
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        if text.len() > 8 * 1024 * 1024 {
            return Err("file is too large to review in one page".into());
        }
        Ok(text)
    }

    pub fn diff_for(&self, file: &FileChange) -> Result<Diff, String> {
        let text = self.file_text(file)?;
        let lines = text.lines().count().max(1);
        match file.status {
            Status::Added => Ok(Diff::added(lines)),
            Status::Deleted => Ok(Diff::deleted(text.lines().map(str::to_string).collect())),
            Status::Modified | Status::Renamed => {
                let old = self
                    .old_ref
                    .as_deref()
                    .ok_or("this review has no base revision")?;
                let mut args = vec!["diff", "--no-ext-diff", "--no-color", "--unified=0"];
                if self.staged {
                    args.extend(["--cached", old, "--", file.rel.as_str()]);
                } else if let Some(new_ref) = &self.new_ref {
                    args.extend([old, new_ref.as_str(), "--"]);
                    if let Some(old_rel) = &file.old_rel {
                        args.push(old_rel.as_str());
                    }
                    args.push(file.rel.as_str());
                } else {
                    args.extend([old, "--", file.rel.as_str()]);
                }
                let out = git_bytes(&self.root, &args)?;
                Ok(parse_unified(&String::from_utf8_lossy(&out)))
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Diff {
    /// New-side line indices that changed, sorted.
    pub changed: Vec<usize>,
    /// Deletions that have no line of their own in the new file.
    pub deletions: Vec<Deletion>,
    /// The old lines to lift when a changed line (or a deletion) is asked about.
    pub old: BTreeMap<usize, Vec<String>>,
    pub deleted_file: bool,
    pub added_file: bool,
}

impl Diff {
    pub fn added(_lines: usize) -> Self {
        Self {
            changed: (0.._lines).collect(),
            added_file: true,
            ..Default::default()
        }
    }

    pub fn deleted(lines: Vec<String>) -> Self {
        let mut old = BTreeMap::new();
        for (i, line) in lines.iter().enumerate() {
            old.insert(i, vec![line.clone()]);
        }
        Self {
            old,
            deleted_file: true,
            ..Default::default()
        }
    }

    pub fn changed_at(&self, line: usize) -> bool {
        self.changed.binary_search(&line).is_ok()
    }

    pub fn deletion_at(&self, line: usize) -> Option<&Deletion> {
        self.deletions.iter().find(|d| d.at == line)
    }

    pub fn old_for(&self, line: usize) -> Option<&[String]> {
        self.old.get(&line).map(Vec::as_slice)
    }

    /// The next marked line to visit, wrapping at the ends.
    ///
    /// A review is a loop: `]` after the last change returns to the first, so the reader
    /// can lean on the key. Deletions count as marks, though they sit *between* lines
    /// rather than on one.
    pub fn change_step(&self, current: usize, forward: bool) -> Option<usize> {
        let mut at: Vec<usize> = self
            .changed
            .iter()
            .copied()
            .chain(self.deletions.iter().map(|d| d.at))
            .collect();
        at.sort_unstable();
        at.dedup();
        if forward {
            at.iter()
                .copied()
                .find(|&line| line > current)
                .or_else(|| at.first().copied())
        } else {
            at.iter()
                .rev()
                .copied()
                .find(|&line| line < current)
                .or_else(|| at.last().copied())
        }
    }
}

#[derive(Clone, Debug)]
pub struct Deletion {
    /// New-side line index before which the removal sits.
    pub at: usize,
    pub old: Vec<String>,
}

/// Read the changed files for a base, without walking ignored dependency trees.
pub fn collect(root: &Path, base: &Base, folder_files: &[FileEntry]) -> Result<Review, String> {
    let repo = repo_root(root)?;
    let branch = git_text(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();
    let branch = if branch.is_empty() || branch == "HEAD" {
        "detached HEAD".to_string()
    } else {
        branch
    };

    let (old_ref, new_ref, staged) = match base {
        Base::Working => {
            ensure_head(&repo)?;
            (Some("HEAD".to_string()), None, false)
        }
        Base::Staged => {
            ensure_head(&repo)?;
            (Some("HEAD".to_string()), None, true)
        }
        Base::LastCommit => {
            ensure_parent(&repo)?;
            (Some("HEAD^".to_string()), Some("HEAD".to_string()), false)
        }
        Base::Ref(reference) => {
            let resolved = resolve_ref(&repo, reference)?;
            let merge = git_text(&repo, &["merge-base", &resolved, "HEAD"])?
                .trim()
                .to_string();
            if merge.is_empty() {
                return Err(format!("{reference} and HEAD have no common ancestor"));
            }
            (Some(merge), Some("HEAD".to_string()), false)
        }
    };

    let old = old_ref.as_deref().unwrap_or("HEAD");
    let mut raw: Vec<RawChange> = match base {
        Base::Working => parse_name_status(&git_bytes(
            &repo,
            &["diff", "--name-status", "-z", "--find-renames", old],
        )?),
        Base::Staged => parse_name_status(&git_bytes(
            &repo,
            &[
                "diff",
                "--cached",
                "--name-status",
                "-z",
                "--find-renames",
                old,
            ],
        )?),
        Base::LastCommit => parse_name_status(&git_bytes(
            &repo,
            &[
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                "HEAD^",
                "HEAD",
            ],
        )?),
        Base::Ref(_) => parse_name_status(&git_bytes(
            &repo,
            &["diff", "--name-status", "-z", "--find-renames", old, "HEAD"],
        )?),
    };

    if base.is_working() {
        let known: BTreeSet<&str> = folder_files.iter().map(|f| f.rel.as_str()).collect();
        let untracked = git_bytes(&repo, &["ls-files", "--others", "--exclude-standard", "-z"])?;
        for rel in untracked.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            let rel = String::from_utf8_lossy(rel).replace('\\', "/");
            if is_noise_rel(&rel) || (!known.is_empty() && !known.contains(rel.as_str())) {
                continue;
            }
            raw.push(RawChange {
                status: Status::Added,
                rel,
                old_rel: None,
            });
        }
    }

    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    let mut skipped = 0;
    for change in raw {
        let rel = change.rel.replace('\\', "/");
        let old_rel = change.old_rel.map(|s| s.replace('\\', "/"));
        if rel.is_empty() || is_noise_rel(&rel) || old_rel.as_deref().is_some_and(is_noise_rel) {
            skipped += 1;
            continue;
        }
        if !seen.insert(rel.clone()) {
            continue;
        }
        files.push(FileChange {
            path: repo.join(&rel),
            rel,
            status: change.status,
            old_rel,
        });
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));

    Ok(Review {
        root: repo,
        base: base.clone(),
        branch,
        old_ref,
        new_ref,
        staged,
        files,
        skipped,
    })
}

fn ensure_head(repo: &Path) -> Result<(), String> {
    if git_text(repo, &["rev-parse", "--verify", "HEAD"]).is_err() {
        return Err("this repository has no commits yet".into());
    }
    Ok(())
}

fn ensure_parent(repo: &Path) -> Result<(), String> {
    ensure_head(repo)?;
    if git_text(repo, &["rev-parse", "--verify", "HEAD^"]).is_err() {
        return Err("the first commit has no parent; review the working tree instead".into());
    }
    Ok(())
}

fn resolve_ref(repo: &Path, reference: &str) -> Result<String, String> {
    let spec = format!("{reference}^{{commit}}");
    let out = git_text(repo, &["rev-parse", "--verify", &spec])?;
    let out = out.trim().to_string();
    if out.is_empty() {
        Err(format!("unknown branch or commit: {reference}"))
    } else {
        Ok(out)
    }
}

pub fn repo_root(root: &Path) -> Result<PathBuf, String> {
    let out = git_text(root, &["rev-parse", "--show-toplevel"])?;
    let path = PathBuf::from(out.trim());
    path.canonicalize().map_err(|e| e.to_string())
}

pub fn branches(root: &Path) -> Vec<String> {
    let Ok(out) = git_text(
        root,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    ) else {
        return Vec::new();
    };
    let mut out: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "HEAD" && !s.ends_with("/HEAD"))
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

struct RawChange {
    status: Status,
    rel: String,
    old_rel: Option<String>,
}

fn parse_name_status(bytes: &[u8]) -> Vec<RawChange> {
    let mut parts = bytes.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut out = Vec::new();
    while let Some(status) = parts.next() {
        let status = String::from_utf8_lossy(status).to_string();
        let Some(rel) = parts.next().map(|s| String::from_utf8_lossy(s).to_string()) else {
            break;
        };
        let kind = status.chars().next().unwrap_or('M');
        if matches!(kind, 'R' | 'C') {
            let Some(new_rel) = parts.next().map(|s| String::from_utf8_lossy(s).to_string()) else {
                break;
            };
            out.push(RawChange {
                status: if kind == 'R' {
                    Status::Renamed
                } else {
                    Status::Added
                },
                rel: new_rel,
                old_rel: Some(rel),
            });
        } else {
            let status = match kind {
                'A' => Status::Added,
                'D' => Status::Deleted,
                'R' => Status::Renamed,
                _ => Status::Modified,
            };
            out.push(RawChange {
                status,
                rel,
                old_rel: None,
            });
        }
    }
    out
}

/// Parse a zero-context unified diff. The new side is the page; the old side is lifted
/// only when the reader asks for it.
fn parse_unified(text: &str) -> Diff {
    let mut diff = Diff::default();
    let mut in_hunk = false;
    let mut new_start = 0usize;
    let mut new_count = 0usize;
    let mut old_lines: Vec<String> = Vec::new();

    let finish =
        |new_start: usize, new_count: usize, old_lines: &mut Vec<String>, diff: &mut Diff| {
            if new_count == 0 {
                if !old_lines.is_empty() {
                    diff.deletions.push(Deletion {
                        at: new_start,
                        old: std::mem::take(old_lines),
                    });
                }
            } else {
                for i in 0..new_count {
                    let line = new_start.saturating_sub(1) + i;
                    diff.changed.push(line);
                    if !old_lines.is_empty() {
                        diff.old.insert(line, old_lines.clone());
                    }
                }
                old_lines.clear();
            }
        };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            finish(new_start, new_count, &mut old_lines, &mut diff);
            let spec = rest.split(" @@").next().unwrap_or("");
            let mut fields = spec.split_whitespace();
            let old = fields
                .next()
                .and_then(|s| parse_range(s.trim_start_matches('-')));
            let new = fields
                .next()
                .and_then(|s| parse_range(s.trim_start_matches('+')));
            if let (Some(_old), Some(new)) = (old, new) {
                new_start = new.0;
                new_count = new.1;
                in_hunk = true;
            } else {
                in_hunk = false;
            }
            continue;
        }
        if !in_hunk {
            continue;
        }
        // Within a hunk every line is content, including one that happens to begin with
        // `--`. A deleted SQL comment (`-- note`) arrives as `--- note`; treating that
        // as a file header loses the old code. Headers only appear before the first
        // hunk, where `in_hunk` is false.
        if let Some(rest) = line.strip_prefix('-') {
            old_lines.push(rest.to_string());
        } else if line.starts_with("\\ No newline") {
            // Not a content line.
        }
    }
    finish(new_start, new_count, &mut old_lines, &mut diff);
    diff.changed.sort_unstable();
    diff.changed.dedup();
    diff
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(',').unwrap_or((s, "1"));
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("--no-pager")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "git is not installed".to_string()
            } else {
                format!("could not run git: {e}")
            }
        })?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if why.is_empty() {
            "git could not read this repository".to_string()
        } else {
            why
        });
    }
    Ok(output.stdout)
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&git_bytes(root, args)?).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn run_git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository with one commit, or `None` when git is not installed. Every test
    /// that needs history shares this setup so the fixture cannot drift.
    fn git_repo(label: &str) -> Option<TempDir> {
        let probe = Command::new("git").arg("--version").output().ok()?;
        if !probe.status.success() {
            return None;
        }
        let dir = TempDir::new(label);
        run_git(dir.path(), &["init", "-q"]);
        run_git(
            dir.path(),
            &["config", "user.email", "zenith@example.invalid"],
        );
        run_git(dir.path(), &["config", "user.name", "zenith"]);
        run_git(dir.path(), &["config", "commit.gpgsign", "false"]);
        Some(dir)
    }

    #[test]
    fn parses_a_replacement_and_a_deletion() {
        let diff = parse_unified(
            "@@ -3 +3 @@\n-let a = 1;\n+let a = 2;\n@@ -7,2 +7,0 @@\n-let b = 1;\n-let b = 2;\n",
        );
        assert_eq!(diff.changed, vec![2]);
        assert_eq!(diff.old_for(2).unwrap(), ["let a = 1;"]);
        assert_eq!(diff.deletions.len(), 1);
        assert_eq!(diff.deletions[0].at, 7);
        assert_eq!(diff.deletions[0].old, ["let b = 1;", "let b = 2;"]);
    }

    #[test]
    fn a_hunk_that_only_adds_marks_every_new_line() {
        let diff = parse_unified("@@ -4,0 +5,3 @@\n+let a = 1;\n+let b = 2;\n+let c = 3;\n");
        assert_eq!(diff.changed, vec![4, 5, 6]);
        assert!(diff.old.is_empty());
        assert!(diff.deletions.is_empty());
    }

    #[test]
    fn a_deletion_sits_before_the_line_that_follows_it() {
        let diff = parse_unified("@@ -2,2 +1,0 @@\n-old a\n-old b\n");
        assert!(diff.changed.is_empty());
        assert_eq!(diff.deletions.len(), 1);
        assert_eq!(diff.deletions[0].at, 1);
        assert_eq!(diff.deletions[0].old, ["old a", "old b"]);
    }

    #[test]
    fn old_lines_that_begin_with_dashes_are_still_code() {
        // A deleted SQL comment arrives as `--- note`, which looks like a file header
        // unless the parser remembers it is inside a hunk.
        let diff = parse_unified("@@ -1,2 +1,0 @@\n--- a SQL comment\n--- and another\n");
        assert_eq!(diff.deletions.len(), 1);
        assert_eq!(
            diff.deletions[0].old,
            ["-- a SQL comment", "-- and another"]
        );
    }

    #[test]
    fn the_no_newline_marker_is_not_old_code() {
        let diff = parse_unified("@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n");
        assert_eq!(diff.changed, vec![0]);
        assert_eq!(diff.old_for(0).unwrap(), ["old"]);
    }

    #[test]
    fn paths_with_spaces_survive_name_status() {
        let raw = parse_name_status(b"M\0src/a b.rs\0A\0docs/new file.md\0");
        assert_eq!(raw.len(), 2);
        assert_eq!(raw[0].rel, "src/a b.rs");
        assert_eq!(raw[0].status, Status::Modified);
        assert_eq!(raw[1].rel, "docs/new file.md");
        assert_eq!(raw[1].status, Status::Added);
    }

    #[test]
    fn reads_renames_from_name_status() {
        let raw = parse_name_status(b"M\0a.rs\0R100\0old.rs\0new.rs\0A\0n.rs\0");
        assert_eq!(raw.len(), 3);
        assert_eq!(raw[0].status, Status::Modified);
        assert_eq!(raw[1].status, Status::Renamed);
        assert_eq!(raw[1].old_rel.as_deref(), Some("old.rs"));
        assert_eq!(raw[2].status, Status::Added);
    }

    #[test]
    fn change_steps_wrap_both_ways() {
        // Changed line 1, then a deletion before line 7. A review is a loop in both
        // directions; falling off either end is the bug this prevents.
        let diff = parse_unified("@@ -2 +2 @@\n-a\n+b\n@@ -7,2 +7,0 @@\n-c\n-d\n");
        assert_eq!(diff.change_step(0, true), Some(1));
        assert_eq!(diff.change_step(1, true), Some(7));
        assert_eq!(diff.change_step(7, true), Some(1));
        assert_eq!(diff.change_step(0, false), Some(7));
        assert_eq!(diff.change_step(7, false), Some(1));
        assert_eq!(diff.change_step(1, false), Some(7));
        assert_eq!(Diff::default().change_step(3, true), None);
    }

    #[test]
    fn a_feature_branch_is_compared_from_its_merge_base() {
        let Some(repo) = git_repo("review-merge") else {
            return;
        };
        let root = repo.path();
        repo.write("src/app.rs", "fn main() {}\n");
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-qm", "base"]);
        run_git(root, &["branch", "-M", "main"]);
        run_git(root, &["checkout", "-qb", "feature"]);
        repo.write("src/app.rs", "fn main() { println!(\"hi\"); }\n");
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-qm", "feature"]);

        let review = collect(root, &Base::Ref("main".into()), &[]).unwrap();
        assert_eq!(review.files.len(), 1);
        assert_eq!(review.files[0].rel, "src/app.rs");
        let diff = review.diff_for(&review.files[0]).unwrap();
        assert_eq!(diff.changed, vec![0]);
        assert!(diff.old_for(0).is_some());
    }

    #[test]
    fn untracked_dependency_trees_never_enter_the_review() {
        let Some(repo) = git_repo("review-untracked") else {
            return;
        };
        let root = repo.path();
        repo.write("src/base.rs", "fn base() {}\n");
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-qm", "base"]);

        repo.write("src/base.rs", "fn base() { changed(); }\n");
        repo.write("src/new.rs", "fn new() {}\n");
        repo.write("node_modules/pkg/index.js", "module.exports = 1;\n");
        repo.write("target/debug/junk.rs", "fn junk() {}\n");

        let review = collect(root, &Base::Working, &[]).unwrap();
        let rels: Vec<&str> = review.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, ["src/base.rs", "src/new.rs"]);
        assert_eq!(review.files[0].status, Status::Modified);
        assert_eq!(review.files[1].status, Status::Added);
        assert!(review.diff_for(&review.files[0]).is_ok());
    }

    #[test]
    fn base_labels_are_reader_language() {
        assert_eq!(Base::Working.label(), "working tree");
        assert_eq!(Base::Ref("main".into()).label(), "main");
    }
}
