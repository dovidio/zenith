//! Configuration, the team catalogue, and reading memory.
//!
//! Two files, both outside the projects being read:
//!   * `config.toml`  — taste: theme, measure, leading, the team's workspace root.
//!   * `session.toml` — memory: how far into each file you had read, favourites.
//!
//! Neither is ever written into a repository. Read-only means read-only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::shelf::Repo;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Where the team's projects live. Repositories under it form the "team shelf".
    pub workspace: PathBuf,
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
    /// Extra hand-named shelves.
    pub shelves: Vec<ShelfCfg>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShelfCfg {
    pub name: String,
    pub path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
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
            shelves: Vec::new(),
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

/// Where a team's projects probably live. A reader should open onto a shelf, not onto
/// a home directory, so zenith looks for the usual workshop names first.
pub fn default_workspace() -> PathBuf {
    let home = home_dir();
    for candidate in [
        "hack",
        "projects",
        "code",
        "dev",
        "src",
        "work",
        "Developer",
        "Documents/GitHub",
    ] {
        let path = home.join(candidate);
        if path.is_dir() {
            return path;
        }
    }
    home
}

/// The catalogue a reader chooses from: the team's repositories, plus any shelves
/// they named by hand.
pub struct Catalog {
    pub workspace: PathBuf,
    pub team: Vec<Repo>,
    pub shelves: Vec<ShelfCfg>,
}

impl Catalog {
    pub fn build(config: &Config) -> Self {
        let workspace = if config.workspace.as_os_str().is_empty() {
            home_dir()
        } else {
            config.workspace.clone()
        };
        let team = crate::shelf::discover_repos(&workspace, 120);
        Catalog {
            workspace,
            team,
            shelves: config.shelves.clone(),
        }
    }
}

pub fn display_path(p: &Path) -> String {
    let home = home_dir();
    match p.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => p.to_string_lossy().to_string(),
    }
}
