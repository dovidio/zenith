//! Opening a folder for reading.
//!
//! Nothing here ever writes, creates, or touches a file's mtime. We walk with the
//! `ignore` crate so a repository's own .gitignore decides what is source and what
//! is noise — the same judgment the team already made.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignore::WalkBuilder;

/// Directories that are never source, no matter what .gitignore says.
const NOISE_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".hg",
    ".svn",
    "dist",
    "build",
    "out",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".cache",
    ".gradle",
    ".idea",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "Pods",
    "DerivedData",
    "coverage",
    ".terraform",
    "bin",
    "obj",
    ".dart_tool",
    ".angular",
    "tmp",
    "logs",
    ".zenith",
];

const CODE_EXTS: &[&str] = &[
    "rs", "py", "pyi", "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "go", "java", "kt", "kts",
    "swift", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "cs", "rb", "rake", "php", "sh", "bash",
    "zsh", "fish", "sql", "html", "htm", "css", "scss", "sass", "less", "json", "jsonc", "yaml",
    "yml", "toml", "ini", "cfg", "conf", "xml", "md", "markdown", "mdx", "txt", "tex", "lua", "r",
    "jl", "hs", "ex", "exs", "erl", "clj", "cljs", "scala", "dart", "vue", "svelte", "astro", "zig",
    "nim", "v", "elm", "ml", "mli", "fs", "fsx", "pl", "pm", "tcl", "awk", "bat", "ps1", "gradle",
    "proto", "graphql", "gql", "tf", "hcl", "nix", "cmake", "csv", "tsv", "dot", "asm", "s", "wat",
    "rst", "adoc", "org", "http", "env", "properties", "editorconfig", "gitignore", "dockerfile",
    "makefile", "justfile",
];

/// Files worth walking to first when a reader opens a repository.
const ENTRY_NAMES: &[&str] = &[
    "main.rs",
    "lib.rs",
    "main.go",
    "main.py",
    "app.py",
    "manage.py",
    "index.js",
    "index.ts",
    "index.tsx",
    "main.js",
    "main.ts",
    "app.js",
    "app.ts",
    "server.js",
    "server.ts",
    "mod.rs",
    "Application.java",
    "main.c",
    "main.cpp",
    "main.swift",
];

const MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "composer.json",
    "CMakeLists.txt",
    "Makefile",
    "mix.exs",
    "Package.swift",
    "pubspec.yaml",
    "requirements.txt",
];

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub rel: String,
    pub name: String,
    pub size: u64,
    /// True for README/index/main-style doorways into the repository.
    pub doorway: bool,
}

#[derive(Clone, Debug)]
pub enum Entry {
    Dir(Dir),
    File(FileEntry),
}

#[derive(Clone, Debug)]
pub struct Dir {
    pub name: String,
    pub children: Vec<Entry>,
}

pub struct Shelf {
    pub name: String,
    pub root: PathBuf,
    pub tree: Vec<Entry>,
    /// Every readable file, sorted by path — the fuzzy finder's index.
    pub files: Vec<FileEntry>,
    /// Files left out on purpose: not source (when "source files only" is on), hidden,
    /// inside build output, or empty placeholders. This is the setting doing its job, so
    /// it is reported as a filter and not as a problem.
    pub filtered: usize,
    /// Entries the walker could not read at all — the only number here that is a fault.
    pub unreadable: usize,
    /// Set when the walk stopped at [`MAX_FILES`].
    pub truncated: bool,
}

impl Shelf {
    pub fn file(&self, path: &Path) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }
}

pub fn is_code_ext(ext: &str) -> bool {
    let e = ext.to_ascii_lowercase();
    CODE_EXTS.contains(&e.as_str())
}

fn is_noise_dir(name: &str) -> bool {
    NOISE_DIRS.contains(&name)
}

/// Open `root` for reading. `code_only` hides things like images, archives, and
/// lockfiles; `show_hidden` lets dotfiles in.
pub fn open(root: &Path, name: &str, code_only: bool, show_hidden: bool) -> std::io::Result<Shelf> {
    let mut files: Vec<FileEntry> = Vec::new();
    let mut filtered = 0usize;
    let mut unreadable = 0usize;
    let mut truncated = false;
    const MAX_FILES: usize = 40_000;

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!show_hidden)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .follow_links(false)
        .same_file_system(true)
        // Prune build output and dependency trees outright. Counting them as
        // "skipped" was not enough: the walker still descended into them.
        .filter_entry(|entry| {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                !is_noise_dir(&entry.file_name().to_string_lossy())
            } else {
                true
            }
        });

    let walker = builder.build();
    for result in walker {
        let Ok(entry) = result else {
            unreadable += 1;
            continue;
        };
        let path = entry.path();
        if path == root {
            continue;
        }
        let Some(name_os) = path.file_name() else {
            continue;
        };
        let fname = name_os.to_string_lossy().to_string();

        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if is_noise_dir(&fname) {
                filtered += 1;
            }
            // Directories are reconstructed from file paths below; nothing to do.
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if code_only && !is_code_ext(&ext) {
            filtered += 1;
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size == 0 && ext.is_empty() && !is_known_name(&fname) {
            // Empty, extensionless, unknown: almost certainly a placeholder.
            filtered += 1;
            continue;
        }
        if files.len() >= MAX_FILES {
            truncated = true;
            break;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let lower = fname.to_ascii_lowercase();
        let doorway = ENTRY_NAMES.contains(&fname.as_str())
            || lower.starts_with("readme")
            || lower.starts_with("index.")
            || MANIFESTS.contains(&fname.as_str());
        files.push(FileEntry {
            path: path.to_path_buf(),
            rel,
            name: fname,
            size,
            doorway,
        });
    }

    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let tree = build_tree(&files);

    Ok(Shelf {
        name: name.to_string(),
        root: root.to_path_buf(),
        tree,
        files,
        filtered,
        unreadable,
        truncated,
    })
}

fn is_known_name(name: &str) -> bool {
    MANIFESTS.contains(&name) || ENTRY_NAMES.contains(&name)
}

/// Rebuild a directory hierarchy from a flat, sorted file list.
fn build_tree(files: &[FileEntry]) -> Vec<Entry> {
    let mut roots: Vec<Entry> = Vec::new();
    // component -> index path into the tree
    for f in files {
        let comps: Vec<&str> = f.rel.split('/').collect();
        insert(&mut roots, &comps, f);
    }
    sort_entries(&mut roots);
    roots
}

fn insert(level: &mut Vec<Entry>, comps: &[&str], file: &FileEntry) {
    if comps.len() <= 1 {
        level.push(Entry::File(file.clone()));
        return;
    }
    let dir_name = comps[0];
    let pos = level.iter().position(|e| match e {
        Entry::Dir(d) => d.name == dir_name,
        Entry::File(_) => false,
    });
    let idx = match pos {
        Some(i) => i,
        None => {
            level.push(Entry::Dir(Dir {
                name: dir_name.to_string(),
                children: Vec::new(),
            }));
            level.len() - 1
        }
    };
    if let Entry::Dir(d) = &mut level[idx] {
        insert(&mut d.children, &comps[1..], file);
    }
}

fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| match (a, b) {
        (Entry::Dir(x), Entry::Dir(y)) => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
        (Entry::Dir(_), Entry::File(_)) => std::cmp::Ordering::Less,
        (Entry::File(_), Entry::Dir(_)) => std::cmp::Ordering::Greater,
        (Entry::File(x), Entry::File(y)) => {
            // Doorways (README, manifest, entry points) float to the top of a folder.
            match (x.doorway, y.doorway) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
            }
        }
    });
    for e in entries.iter_mut() {
        if let Entry::Dir(d) = e {
            sort_entries(&mut d.children);
        }
    }
}

// ---------------------------------------------------------------------------
// Team discovery
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Repo {
    pub name: String,
    pub path: PathBuf,
    pub touched: Option<SystemTime>,
    /// Language hint for the shelf entry, derived from manifests present.
    pub kind: String,
}

/// Find the team's repositories: every directory under `workspace` that looks like
/// a project. Recency of modification is the ordering, because a team's attention
/// is what a reader usually wants to follow.
pub fn discover_repos(workspace: &Path, max: usize) -> Vec<Repo> {
    let mut out: Vec<Repo> = Vec::new();
    let Ok(rd) = std::fs::read_dir(workspace) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || is_noise_dir(&name) {
            continue;
        }
        let mut kinds: Vec<&str> = Vec::new();
        for m in MANIFESTS {
            if path.join(m).exists() {
                kinds.push(match *m {
                    "Cargo.toml" => "rust",
                    "package.json" => "js",
                    "go.mod" => "go",
                    "pyproject.toml" | "setup.py" | "requirements.txt" => "python",
                    "pom.xml" | "build.gradle" | "build.gradle.kts" => "java",
                    "Gemfile" => "ruby",
                    "composer.json" => "php",
                    "CMakeLists.txt" => "c/c++",
                    "Package.swift" => "swift",
                    "pubspec.yaml" => "dart",
                    "mix.exs" => "elixir",
                    _ => "project",
                });
            }
        }
        let is_repo = path.join(".git").exists();
        if kinds.is_empty() && !is_repo {
            continue;
        }
        let touched = dir_touched(&path);
        out.push(Repo {
            name,
            path,
            touched,
            kind: dedup_kinds(&kinds, is_repo),
        });
        if out.len() >= max {
            break;
        }
    }
    out.sort_by(|a, b| b.touched.cmp(&a.touched).then(a.name.cmp(&b.name)));
    out
}

fn dedup_kinds(kinds: &[&str], is_repo: bool) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for k in kinds {
        if !seen.contains(k) {
            seen.push(k);
        }
    }
    if seen.is_empty() {
        return if is_repo { "repository".into() } else { "folder".into() };
    }
    seen.join(" · ")
}

fn dir_touched(path: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for probe in [path.join(".git"), path.to_path_buf()] {
        if let Ok(md) = std::fs::metadata(&probe) {
            if let Ok(t) = md.modified() {
                newest = Some(match newest {
                    Some(n) if n > t => n,
                    _ => t,
                });
            }
        }
    }
    newest
}

#[allow(dead_code)]
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{} B", bytes)
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.2} GB", b / (KB * KB * KB))
    }
}
