//! Which server serves which file, and where it might be on this machine.
//!
//! Zenith does not download, bundle or install language servers, and this module is the
//! whole of that decision:
//!
//! * A downloaded server is usually the *wrong* server. rust-analyzer has to match the
//!   project's toolchain, `tsserver` its TypeScript, gopls the `go` line in its go.mod.
//!   The right copy is nearly always the one the project already builds against.
//! * Installing one would make a reader into a package manager, with a network client
//!   and an installer, in an application whose only promise is that it reads.
//! * Whoever is reading Rust has rustup. The servers are, in practice, already there.
//!
//! What "already there" needs is *finding*: a table, a search, and one honest report of
//! what was found. Nothing here is a verdict — see [`probe`].

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Kind {
    /// Stable key. Names the server in `config.toml` and dedupes running servers.
    pub id: &'static str,
    /// What a reader calls the language.
    pub title: &'static str,
    /// Binaries, in the order worth trying, with the arguments that make each of them
    /// speak the protocol on stdin. Some servers only do when asked.
    pub candidates: &'static [(&'static str, &'static [&'static str])],
    /// Extensions, each with the LSP `languageId` for that extension — which is not
    /// always the language of the server: TypeScript's server wants `javascript` for a
    /// `.js` file, and `typescriptreact` for a `.tsx` one.
    pub exts: &'static [(&'static str, &'static str)],
}

pub const KINDS: &[Kind] = &[
    Kind {
        id: "rust",
        title: "Rust",
        candidates: &[("rust-analyzer", &[])],
        exts: &[("rs", "rust")],
    },
    Kind {
        id: "go",
        title: "Go",
        candidates: &[("gopls", &[])],
        exts: &[("go", "go")],
    },
    Kind {
        id: "typescript",
        title: "TypeScript",
        candidates: &[("typescript-language-server", &["--stdio"])],
        exts: &[
            ("ts", "typescript"),
            ("mts", "typescript"),
            ("cts", "typescript"),
            ("tsx", "typescriptreact"),
            ("js", "javascript"),
            ("mjs", "javascript"),
            ("cjs", "javascript"),
            ("jsx", "javascriptreact"),
        ],
    },
    Kind {
        id: "python",
        title: "Python",
        candidates: &[("pyright-langserver", &["--stdio"]), ("pylsp", &[])],
        exts: &[("py", "python"), ("pyi", "python")],
    },
    Kind {
        id: "c",
        title: "C",
        candidates: &[("clangd", &[])],
        exts: &[("c", "c"), ("h", "c")],
    },
    Kind {
        id: "cpp",
        title: "C++",
        candidates: &[("clangd", &[])],
        exts: &[
            ("cc", "cpp"),
            ("cpp", "cpp"),
            ("cxx", "cpp"),
            ("hpp", "cpp"),
            ("hh", "cpp"),
            ("hxx", "cpp"),
        ],
    },
    Kind {
        id: "java",
        title: "Java",
        candidates: &[("jdtls", &[])],
        exts: &[("java", "java")],
    },
    Kind {
        id: "kotlin",
        title: "Kotlin",
        candidates: &[("kotlin-language-server", &[])],
        exts: &[("kt", "kotlin"), ("kts", "kotlin")],
    },
    Kind {
        id: "swift",
        title: "Swift",
        candidates: &[("sourcekit-lsp", &[])],
        exts: &[("swift", "swift")],
    },
    Kind {
        id: "ruby",
        title: "Ruby",
        candidates: &[("ruby-lsp", &[]), ("solargraph", &["stdio"])],
        exts: &[("rb", "ruby")],
    },
    Kind {
        id: "lua",
        title: "Lua",
        candidates: &[("lua-language-server", &[])],
        exts: &[("lua", "lua")],
    },
    Kind {
        id: "zig",
        title: "Zig",
        candidates: &[("zls", &[])],
        exts: &[("zig", "zig")],
    },
    Kind {
        id: "shell",
        title: "Shell",
        candidates: &[("bash-language-server", &["start"])],
        exts: &[("sh", "shellscript"), ("bash", "shellscript"), ("zsh", "shellscript")],
    },
    Kind {
        id: "haskell",
        title: "Haskell",
        candidates: &[("haskell-language-server-wrapper", &["--lsp"])],
        exts: &[("hs", "haskell"), ("lhs", "haskell")],
    },
    Kind {
        id: "csharp",
        title: "C#",
        candidates: &[("omnisharp", &["-lsp"])],
        exts: &[("cs", "csharp")],
    },
    Kind {
        id: "dart",
        title: "Dart",
        candidates: &[("dart", &["language-server", "--protocol=lsp"])],
        exts: &[("dart", "dart")],
    },
    Kind {
        id: "nix",
        title: "Nix",
        candidates: &[("nixd", &[]), ("nil", &[])],
        exts: &[("nix", "nix")],
    },
    Kind {
        id: "terraform",
        title: "Terraform",
        candidates: &[("terraform-ls", &["serve"])],
        exts: &[("tf", "terraform"), ("tfvars", "terraform")],
    },
    Kind {
        id: "php",
        title: "PHP",
        candidates: &[("intelephense", &["--stdio"])],
        exts: &[("php", "php")],
    },
];

/// The server for a file, and the `languageId` to tell it about that file.
///
/// Notably this is *not* the syntax highlighter's answer: zenith sets `.ts` in the
/// JavaScript grammar, because there is no TypeScript grammar to set it in, but a
/// language server has to be told the truth.
pub fn kind_for(path: &Path) -> Option<(&'static Kind, &'static str)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    for kind in KINDS {
        for (e, id) in kind.exts {
            if *e == ext {
                return Some((kind, id));
            }
        }
    }
    None
}

/// The languages present in a set of files, each once, in table order.
pub fn kinds_among<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<&'static Kind> {
    let mut found: Vec<&'static Kind> = Vec::new();
    for path in paths {
        if let Some((kind, _)) = kind_for(path) {
            if !found.iter().any(|k| k.id == kind.id) {
                found.push(kind);
            }
        }
    }
    found.sort_by_key(|k| KINDS.iter().position(|t| t.id == k.id).unwrap_or(usize::MAX));
    found
}

/// A server the reader pointed at themselves, from `config.toml`.
#[derive(Clone)]
pub struct Override {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone)]
pub struct Available {
    pub kind: &'static Kind,
    pub binary: PathBuf,
    pub args: Vec<String>,
    /// What the binary said when asked its version. Information, never a verdict.
    pub note: String,
}

/// Look for a server on this machine. **Spawns processes**: call it off the UI thread.
pub fn find(
    kind: &'static Kind,
    cwd: &Path,
    over: Option<&Override>,
) -> Option<Available> {
    if let Some(o) = over {
        let binary = PathBuf::from(&o.command);
        if is_executable(&binary) {
            let args = if o.args.is_empty() {
                default_args(kind, &binary)
            } else {
                o.args.clone()
            };
            return Some(Available {
                kind,
                note: probe(&binary),
                binary,
                args,
            });
        }
    }
    for dir in search_dirs(cwd) {
        for (bin, args) in kind.candidates {
            let binary = dir.join(bin);
            if is_executable(&binary) {
                return Some(Available {
                    kind,
                    note: probe(&binary),
                    binary,
                    args: args.iter().map(|a| a.to_string()).collect(),
                });
            }
        }
    }
    None
}

fn default_args(kind: &Kind, binary: &Path) -> Vec<String> {
    let name = binary.file_name().and_then(|n| n.to_str()).unwrap_or("");
    kind.candidates
        .iter()
        .find(|(bin, _)| *bin == name)
        .map(|(_, args)| args.iter().map(|a| a.to_string()).collect())
        .unwrap_or_default()
}

/// Where a server may live, in the order it is worth looking.
///
/// `PATH` first — it is the reader's own answer. But a window opened from the Finder
/// inherits almost nothing: the desktop hands a GUI app `/usr/bin:/bin:/usr/sbin:/sbin`,
/// so the servers people actually install, in `~/.cargo/bin`, `~/go/bin` or Homebrew,
/// are invisible. Reading `path_helper`'s own sources puts them back without running a
/// login shell (which would execute the reader's startup files to answer a question
/// about a directory listing).
fn search_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |d: PathBuf| {
        if !d.as_os_str().is_empty() && !dirs.contains(&d) {
            dirs.push(d);
        }
    };

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            push(dir);
        }
    }
    for dir in listed("/etc/paths") {
        push(dir);
    }
    if let Ok(entries) = std::fs::read_dir("/etc/paths.d") {
        let mut extra: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .flat_map(|e| listed(&e.path().to_string_lossy()))
            .collect();
        extra.sort();
        for dir in extra {
            push(dir);
        }
    }
    let home = crate::catalog::home_dir();
    for dir in ["~/.cargo/bin", "~/.local/bin", "~/go/bin", "~/bin", "~/.bun/bin"] {
        push(home.join(dir.trim_start_matches("~/")));
    }
    for dir in ["/opt/homebrew/bin", "/opt/local/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        push(PathBuf::from(dir));
    }
    // The project's own toolchain beats the machine's: a repository with TypeScript in
    // its node_modules means the server that matches *this* codebase is right there.
    push(cwd.join("node_modules/.bin"));
    dirs
}

fn listed(path: &str) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(PathBuf::from)
        .collect()
}

/// Ask a binary its version, and give it two seconds to answer.
///
/// The answer is *information*: `gopls` refuses `--version` outright ("flag provided but
/// not defined"), and `rustup`'s rust-analyzer shim answers with its own name instead of
/// a version. Using that as a pass/fail gate would refuse to start servers that work and
/// would accept the one that does not. The gate is the handshake, which happens after
/// the reader says yes; this is just what to put next to the path.
pub fn probe(binary: &Path) -> String {
    let mut child = match Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return format!("could not run it: {e}"),
    };
    let deadline = Instant::now() + Duration::from_millis(2_000);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let text = read_pipes(&mut child);
                let first = text
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("")
                    .to_string();
                return if first.is_empty() {
                    format!("exit {}", status.code().unwrap_or(-1))
                } else {
                    first
                };
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return "no answer to --version".into();
            }
        }
    }
}

fn read_pipes(child: &mut std::process::Child) -> String {
    use std::io::Read;
    let mut out = String::new();
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_string(&mut out);
    }
    if let Some(mut e) = child.stderr.take() {
        let mut tail = String::new();
        let _ = e.read_to_string(&mut tail);
        if !tail.trim().is_empty() {
            out.push_str(&tail);
        }
    }
    out
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_servers_language_is_not_the_highlighters_language() {
        // The app sets `.ts` in the JavaScript grammar; the server must be told the truth.
        let (kind, id) = kind_for(Path::new("/x/a.ts")).unwrap();
        assert_eq!(kind.id, "typescript");
        assert_eq!(id, "typescript");
        let (_, id) = kind_for(Path::new("/x/a.js")).unwrap();
        assert_eq!(id, "javascript");
        let (_, id) = kind_for(Path::new("/x/a.tsx")).unwrap();
        assert_eq!(id, "typescriptreact");
    }

    #[test]
    fn prose_is_not_a_language_with_a_server() {
        assert!(kind_for(Path::new("/x/README.md")).is_none());
        assert!(kind_for(Path::new("/x/LICENSE")).is_none());
    }

    #[test]
    fn an_extension_is_matched_regardless_of_case() {
        assert_eq!(kind_for(Path::new("/x/A.RS")).map(|(k, _)| k.id), Some("rust"));
    }

    #[test]
    fn a_folder_reports_each_language_once_and_in_table_order() {
        let files = [
            Path::new("/x/b.go"),
            Path::new("/x/a.rs"),
            Path::new("/x/c.rs"),
            Path::new("/x/d.md"),
        ];
        let kinds = kinds_among(files);
        let ids: Vec<&str> = kinds.iter().map(|k| k.id).collect();
        assert_eq!(ids, vec!["rust", "go"]);
    }

    #[test]
    fn the_search_reaches_past_the_desktop_apps_path() {
        let dirs = search_dirs(Path::new("/x"));
        let home = crate::catalog::home_dir();
        assert!(dirs.contains(&home.join(".cargo/bin")), "cargo's bins");
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")), "homebrew");
        assert!(dirs.contains(&PathBuf::from("/x/node_modules/.bin")), "the project's own");
        let mut sorted = dirs.clone();
        sorted.dedup();
        assert_eq!(dirs.len(), sorted.len(), "no directory is searched twice");
    }
}
