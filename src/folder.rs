//! Opening a folder for reading.
//!
//! Nothing here ever writes, creates, or touches a file's mtime. We walk with the
//! `ignore` crate so a repository's own .gitignore decides what is source and what is
//! noise — the same judgment the project already made.

use std::path::{Path, PathBuf};

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
    "rs",
    "py",
    "pyi",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "mts",
    "go",
    "java",
    "kt",
    "kts",
    "swift",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "hh",
    "cs",
    "rb",
    "rake",
    "php",
    "sh",
    "bash",
    "zsh",
    "fish",
    "sql",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "json",
    "jsonc",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "xml",
    "md",
    "markdown",
    "mdx",
    "txt",
    "tex",
    "lua",
    "r",
    "jl",
    "hs",
    "ex",
    "exs",
    "erl",
    "clj",
    "cljs",
    "scala",
    "dart",
    "vue",
    "svelte",
    "astro",
    "zig",
    "nim",
    "v",
    "elm",
    "ml",
    "mli",
    "fs",
    "fsx",
    "pl",
    "pm",
    "tcl",
    "awk",
    "bat",
    "ps1",
    "gradle",
    "proto",
    "graphql",
    "gql",
    "tf",
    "hcl",
    "nix",
    "cmake",
    "csv",
    "tsv",
    "dot",
    "asm",
    "s",
    "wat",
    "rst",
    "adoc",
    "org",
    "http",
    "env",
    "properties",
    "editorconfig",
    "gitignore",
    "dockerfile",
    "makefile",
    "justfile",
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

pub struct Folder {
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
}

impl Folder {
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

/// Is any component of a repository-relative path a directory that a review should
/// never walk? `node_modules` is the reason this is public: the review includes
/// untracked source, but never dependency trees.
pub fn is_noise_rel(rel: &str) -> bool {
    rel.split('/').any(is_noise_dir)
}

/// Open `root` for reading. `code_only` hides things like images, archives, and
/// lockfiles; `show_hidden` lets dotfiles in.
pub fn open(
    root: &Path,
    name: &str,
    code_only: bool,
    show_hidden: bool,
) -> std::io::Result<Folder> {
    let mut files: Vec<FileEntry> = Vec::new();
    let mut filtered = 0usize;
    let mut unreadable = 0usize;

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
        // Links are not followed. A link to a directory would otherwise enter the index
        // as a file and fail when opened; a link to a file still reads as that file.
        if entry.file_type().is_some_and(|t| t.is_symlink()) && !path.is_file() {
            filtered += 1;
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

    Ok(Folder {
        name: name.to_string(),
        root: root.to_path_buf(),
        tree,
        files,
        filtered,
        unreadable,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn folder_at(dir: &TempDir, code_only: bool, show_hidden: bool) -> Folder {
        open(dir.path(), "fixture", code_only, show_hidden).expect("open the fixture")
    }

    fn rels(folder: &Folder) -> Vec<&str> {
        folder.files.iter().map(|f| f.rel.as_str()).collect()
    }

    #[test]
    fn dependency_and_build_trees_are_pruned_before_the_walk() {
        let dir = TempDir::new("folder-noise");
        dir.write("src/main.rs", "fn main() {}\n");
        dir.write("src/target.rs", "// a file, not a build folder\n");
        dir.write("node_modules/pkg/index.js", "module.exports = 1;\n");
        dir.write("target/debug/app.rs", "fn junk() {}\n");
        dir.write("dist/app.js", "console.log(1);\n");
        dir.write(".git/config", "[core]\n");
        dir.write("vendor/lib.go", "package lib\n");

        let folder = folder_at(&dir, false, true);
        assert_eq!(rels(&folder), ["src/main.rs", "src/target.rs"]);
    }

    #[test]
    fn noise_is_decided_per_path_component() {
        assert!(is_noise_rel("node_modules/react/index.js"));
        assert!(is_noise_rel("apps/web/node_modules/x.js"));
        assert!(is_noise_rel("target/debug/zenith"));
        assert!(is_noise_rel(".git/objects/ab/cdef"));
        assert!(!is_noise_rel("src/nodes.rs"));
        assert!(!is_noise_rel("src/target.rs"));
        assert!(!is_noise_rel("docs/node_modules.md"));
    }

    #[test]
    fn code_only_keeps_source_and_reports_what_it_hid() {
        let dir = TempDir::new("folder-code-only");
        dir.write("src/main.rs", "fn main() {}\n");
        dir.write("assets/logo.png", b"\x89PNG\r\n\x1a\n");
        dir.write("LICENSE", "MIT\n");
        dir.write("empty", "");

        let folder = folder_at(&dir, true, false);
        assert_eq!(rels(&folder), ["src/main.rs"]);
        assert_eq!(folder.filtered, 3, "png, LICENSE, and the placeholder");
    }

    #[test]
    fn hidden_files_need_the_setting() {
        let dir = TempDir::new("folder-hidden");
        dir.write(".hidden.rs", "fn hidden() {}\n");
        dir.write("visible.rs", "fn visible() {}\n");

        assert_eq!(rels(&folder_at(&dir, true, false)), ["visible.rs"]);
        assert_eq!(
            rels(&folder_at(&dir, true, true)),
            [".hidden.rs", "visible.rs"],
            "show-hidden includes dotfiles without including .git"
        );
    }

    #[test]
    fn an_empty_source_file_is_still_source() {
        let dir = TempDir::new("folder-empty");
        dir.write("empty.rs", "");
        assert_eq!(rels(&folder_at(&dir, true, false)), ["empty.rs"]);
    }

    #[test]
    fn doorways_float_above_ordinary_files_and_folders_come_first() {
        let dir = TempDir::new("folder-order");
        dir.write("src/a.rs", "fn a() {}\n");
        dir.write("README.md", "# readme\n");
        dir.write("Cargo.toml", "[package]\n");
        dir.write("z.rs", "fn z() {}\n");

        let folder = folder_at(&dir, false, false);
        let root: Vec<&str> = folder
            .tree
            .iter()
            .map(|entry| match entry {
                Entry::Dir(d) => d.name.as_str(),
                Entry::File(f) => f.name.as_str(),
            })
            .collect();
        assert_eq!(root, ["src", "Cargo.toml", "README.md", "z.rs"]);
        assert!(folder
            .files
            .iter()
            .find(|f| f.rel == "README.md")
            .is_some_and(|f| f.doorway));
    }

    #[test]
    fn a_repositorys_gitignore_decides_what_is_noise() {
        let dir = TempDir::new("folder-ignore");
        // `ignore` only honours .gitignore inside a repository.
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        dir.write(".gitignore", "secret.rs\n");
        dir.write("keep.rs", "fn keep() {}\n");
        dir.write("secret.rs", "fn secret() {}\n");

        let folder = folder_at(&dir, false, false);
        assert_eq!(rels(&folder), ["keep.rs"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_loop_is_not_followed() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new("folder-symlink");
        dir.write("src/main.rs", "fn main() {}\n");
        symlink(dir.path(), dir.join("loop")).unwrap();

        let folder = folder_at(&dir, false, false);
        assert_eq!(rels(&folder), ["src/main.rs"]);
    }

    #[test]
    fn sizes_are_written_for_a_reader() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }
}
