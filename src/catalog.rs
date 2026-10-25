//! Configuration, the reading memory, and where both live on disk.
//!
//! Two files, both outside the projects being read:
//!   * `config.toml`  — taste: theme, measure, leading, typography.
//!   * `session.toml` — memory: how far into each file you had read, favourites.
//!
//! Neither is ever written into a repository. Read-only means read-only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Name of the current theme (see theme::THEMES).
    pub theme: String,
    /// Multiplier on every type size. 1.0 is the tuned default.
    pub font_scale: f32,
    /// Reading measure in characters — the line length that stays comfortable.
    pub measure: usize,
    /// Line height, as a multiple of the font size. Generous by default: this is
    /// a reading room, not a terminal.
    pub leading: f32,
    /// Soft-wrap long lines instead of scrolling sideways. Zenith's core bet.
    pub wrap: bool,
    /// Collapse runs of blank lines to a rarer rhythm.
    pub compress_blank: bool,
    /// Draw indentation as fading hairlines instead of empty space.
    pub guides: bool,
    /// Off by default: line numbers are an editor's affordance, not a reader's.
    pub line_numbers: bool,
    /// Set comments in the serif face, as prose.
    pub serif_comments: bool,
    /// Only show files that look like source.
    pub code_only: bool,
    /// Include dotfiles and dot-directories.
    pub show_hidden: bool,
    /// Dim everything except the block you are reading.
    pub focus_lens: bool,
    /// Recede the parts of a strongly typed language a human is not reading:
    /// type annotations, modifiers, and the punctuation that only the compiler needs.
    pub distilled: bool,
    /// Chrome-free reading.
    pub zen: bool,
    /// Draw the window's own bar instead of the system's, so the strip along the top of
    /// the window is the theme's colour rather than the desktop's.
    pub frameless: bool,
    /// Render Markdown as a document rather than setting its source.
    pub markdown: bool,
    /// Language servers: which folders may run one, and any the reader pointed at
    /// themselves.
    pub lsp: Lsp,
}

/// Language servers, as configuration.
///
/// The list of trusted folders is here, in the reader's own settings, and not in a file
/// inside the project being read. That is forced by the same promise as everything else
/// in this program: zenith writes nothing into a repository, so a per-project setting
/// has to live outside the project.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Lsp {
    /// Folders a server has been allowed to run in.
    pub trusted: Vec<PathBuf>,
    /// Servers at a path of the reader's choosing, named by language:
    ///
    /// ```toml
    /// [lsp.servers.rust]
    /// command = "/opt/homebrew/bin/rust-analyzer"
    /// ```
    pub servers: BTreeMap<String, Server>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Server {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "Washi".into(),
            font_scale: 1.0,
            measure: 86,
            leading: 1.62,
            wrap: true,
            compress_blank: true,
            guides: true,
            line_numbers: false,
            serif_comments: true,
            code_only: true,
            show_hidden: false,
            focus_lens: false,
            distilled: false,
            zen: false,
            frameless: true,
            markdown: true,
            lsp: Lsp::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Session {
    pub last_root: Option<PathBuf>,
    pub last_file: Option<PathBuf>,
    /// Reading position per file, as a fraction of the scroll height.
    pub positions: BTreeMap<String, f32>,
    pub favorites: Vec<PathBuf>,
    pub recent: Vec<PathBuf>,
}

pub struct Store {
    pub dir: PathBuf,
    pub config: Config,
    pub session: Session,
    /// Set when the directory could not be written to; the app then runs in memory.
    pub ephemeral: bool,
}

impl Store {
    pub fn load() -> Self {
        let dir = config_dir();
        let mut ephemeral = false;
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("zenith: config dir {dir:?} is not writable ({e}); running in memory");
            ephemeral = true;
        }
        let config = match std::fs::read_to_string(dir.join("config.toml")) {
            Ok(s) => match toml::from_str::<Config>(&s) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("zenith: config.toml unreadable ({e}); using defaults");
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        };
        let session = match std::fs::read_to_string(dir.join("session.toml")) {
            Ok(s) => toml::from_str::<Session>(&s).unwrap_or_default(),
            Err(_) => Session::default(),
        };
        Store {
            dir,
            config,
            session,
            ephemeral,
        }
    }

    pub fn save_config(&self) {
        if self.ephemeral {
            return;
        }
        if let Ok(s) = toml::to_string_pretty(&self.config) {
            let _ = std::fs::write(self.dir.join("config.toml"), s);
        }
    }

    pub fn save_session(&self) {
        if self.ephemeral {
            return;
        }
        if let Ok(s) = toml::to_string_pretty(&self.session) {
            let _ = std::fs::write(self.dir.join("session.toml"), s);
        }
    }

    pub fn remember_position(&mut self, file: &Path, frac: f32) {
        let key = file.to_string_lossy().to_string();
        self.session.positions.insert(key, frac.clamp(0.0, 1.0));
    }

    pub fn position_of(&self, file: &Path) -> f32 {
        self.session
            .positions
            .get(&file.to_string_lossy().to_string())
            .copied()
            .unwrap_or(0.0)
    }

    pub fn touch_recent(&mut self, file: &Path) {
        self.session.recent.retain(|p| p != file);
        self.session.recent.insert(0, file.to_path_buf());
        self.session.recent.truncate(40);
        self.session.last_file = Some(file.to_path_buf());
    }

    pub fn toggle_favorite(&mut self, file: &Path) -> bool {
        if let Some(pos) = self.session.favorites.iter().position(|p| p == file) {
            self.session.favorites.remove(pos);
            false
        } else {
            self.session.favorites.push(file.to_path_buf());
            true
        }
    }

    pub fn is_favorite(&self, file: &Path) -> bool {
        self.session.favorites.iter().any(|p| p == file)
    }

    /// Has this folder been allowed to run a language server?
    ///
    /// Starting one is not free: a server analyses the project, which means running its
    /// build scripts. It is a small, mundane, and very real kind of consent, and it is
    /// asked once per folder rather than once per session.
    pub fn trusts(&self, root: &Path) -> bool {
        self.config.lsp.trusted.iter().any(|p| p == root)
    }

    pub fn trust(&mut self, root: &Path) {
        if !self.trusts(root) {
            self.config.lsp.trusted.push(root.to_path_buf());
            self.save_config();
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ZENITH_CONFIG_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    home_dir().join(".config").join("zenith")
}

pub fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn display_path(p: &Path) -> String {
    let home = home_dir();
    match p.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => p.to_string_lossy().to_string(),
    }
}

/// The folder a file belongs to, by looking for the marks of a project above it.
///
/// This is what a language server should be rooted at: a crate, a module, a repository —
/// something with a manifest, not a bare directory of loose files.
pub fn project_root(file: &Path) -> PathBuf {
    // Absolute first: walking up from a relative path ends at `""`, and `""` is a path
    // that exists (it joins to this directory), so the walk would stop there and hand a
    // language server an empty root — which it cannot even start in.
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let file = file.as_path();
    let parent = file.parent().map(|p| p.to_path_buf());
    let markers = [
        ".git",
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "pom.xml",
        "Gemfile",
    ];
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dir = parent.clone();
    let mut hops = 0;
    while let Some(d) = dir {
        if hops > 8 {
            break;
        }
        if markers.iter().any(|m| d.join(m).exists()) {
            return d;
        }
        // Stop at the home directory or the root of the filesystem. A "project" is
        // something a reader would recognise; climbing past home is how opening a lone
        // file in /tmp used to hand the walker the whole disk.
        if home.as_deref() == Some(d.as_path()) || d.parent().is_none() {
            break;
        }
        dir = d.parent().map(|p| p.to_path_buf());
        hops += 1;
    }
    // No marker anywhere above it: the file's own folder is the folder.
    parent.unwrap_or_else(|| PathBuf::from("."))
}
