//! The launcher's vocabulary: what can be done, how it is named, and what it is called
//! by. The sheet that shows these lives in `sheets`; this module only knows the list.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Launch {
    Contents,
    Search,
    FindAll,
    CopyBlock,
    CloseFile,
    Bookmark,
    Reload,
    Index,
    OpenFolder,
    Review,
    ReviewBase,
    FocusLens,
    SoftWrap,
    LineNumbers,
    Distilled,
    Markdown,
    NextTheme,
    PrevTheme,
    Larger,
    Smaller,
    Settings,
    Keys,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LaunchGroup {
    Here,
    Folder,
    Zenith,
}

impl LaunchGroup {
    pub(super) fn rank(self) -> u8 {
        match self {
            LaunchGroup::Here => 0,
            LaunchGroup::Folder => 1,
            LaunchGroup::Zenith => 2,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            LaunchGroup::Here => "HERE",
            LaunchGroup::Folder => "FOLDER",
            LaunchGroup::Zenith => "ZENITH",
        }
    }
}

pub(super) struct CommandSpec {
    pub(super) action: Launch,
    pub(super) label: &'static str,
    pub(super) group: LaunchGroup,
    pub(super) shortcut: Option<&'static str>,
    pub(super) keywords: &'static str,
}

pub(super) const LAUNCH_COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        action: Launch::Contents,
        label: "Contents",
        group: LaunchGroup::Here,
        shortcut: Some("o"),
        keywords: "outline symbols headings document",
    },
    CommandSpec {
        action: Launch::Search,
        label: "Search this file",
        group: LaunchGroup::Here,
        shortcut: Some("/"),
        keywords: "find in file grep search",
    },
    CommandSpec {
        action: Launch::FindAll,
        label: "Search every file",
        group: LaunchGroup::Folder,
        shortcut: Some("⌘⇧F"),
        keywords: "project full text content grep ripgrep find across files",
    },
    CommandSpec {
        action: Launch::CopyBlock,
        label: "Copy block",
        group: LaunchGroup::Here,
        shortcut: Some("y"),
        keywords: "clipboard yank copy",
    },
    CommandSpec {
        action: Launch::CloseFile,
        label: "Close file",
        group: LaunchGroup::Here,
        shortcut: Some("q"),
        keywords: "close put down file",
    },
    CommandSpec {
        action: Launch::Bookmark,
        label: "Bookmark",
        group: LaunchGroup::Here,
        shortcut: Some("M"),
        keywords: "favourite favorite star mark",
    },
    CommandSpec {
        action: Launch::Reload,
        label: "Reload from disk",
        group: LaunchGroup::Here,
        shortcut: Some("r"),
        keywords: "reload refresh disk reread index tree folders modules",
    },
    CommandSpec {
        action: Launch::Index,
        label: "Index",
        group: LaunchGroup::Folder,
        shortcut: Some("tab"),
        keywords: "tree files folder index sidebar",
    },
    CommandSpec {
        action: Launch::OpenFolder,
        label: "Open folder…",
        group: LaunchGroup::Folder,
        shortcut: Some("⌘O"),
        keywords: "open project directory folder",
    },
    CommandSpec {
        action: Launch::Review,
        label: "Review changes",
        group: LaunchGroup::Folder,
        shortcut: Some("g r"),
        keywords: "review git diff changes branch pr compare",
    },
    CommandSpec {
        action: Launch::ReviewBase,
        label: "Review against…",
        group: LaunchGroup::Folder,
        shortcut: Some("g b"),
        keywords: "review base branch commit compare git",
    },
    CommandSpec {
        action: Launch::FocusLens,
        label: "Focus lens",
        group: LaunchGroup::Zenith,
        shortcut: Some("f"),
        keywords: "dim attention focus zen lens",
    },
    CommandSpec {
        action: Launch::SoftWrap,
        label: "Soft wrap",
        group: LaunchGroup::Zenith,
        shortcut: None,
        keywords: "wrap lines soft",
    },
    CommandSpec {
        action: Launch::LineNumbers,
        label: "Line numbers",
        group: LaunchGroup::Zenith,
        shortcut: Some("#"),
        keywords: "numbers gutter lines",
    },
    CommandSpec {
        action: Launch::Distilled,
        label: "Distilled",
        group: LaunchGroup::Zenith,
        shortcut: Some("d"),
        keywords: "types punctuation simplify distill",
    },
    CommandSpec {
        action: Launch::Markdown,
        label: "Render markdown",
        group: LaunchGroup::Zenith,
        shortcut: Some("x"),
        keywords: "markdown render preview source",
    },
    CommandSpec {
        action: Launch::NextTheme,
        label: "Next theme",
        group: LaunchGroup::Zenith,
        shortcut: Some("t"),
        keywords: "theme palette colour color atmosphere next",
    },
    CommandSpec {
        action: Launch::PrevTheme,
        label: "Previous theme",
        group: LaunchGroup::Zenith,
        shortcut: Some("T"),
        keywords: "theme palette colour color atmosphere previous",
    },
    CommandSpec {
        action: Launch::Larger,
        label: "Larger type",
        group: LaunchGroup::Zenith,
        shortcut: Some("="),
        keywords: "type font size larger zoom",
    },
    CommandSpec {
        action: Launch::Smaller,
        label: "Smaller type",
        group: LaunchGroup::Zenith,
        shortcut: Some("-"),
        keywords: "type font size smaller zoom",
    },
    CommandSpec {
        action: Launch::Settings,
        label: "Settings",
        group: LaunchGroup::Zenith,
        shortcut: Some("s"),
        keywords: "preferences options settings",
    },
    CommandSpec {
        action: Launch::Keys,
        label: "Keys",
        group: LaunchGroup::Zenith,
        shortcut: Some("?"),
        keywords: "shortcuts keymap keyboard help keys",
    },
];

pub(super) fn launch_command_score(spec: &CommandSpec, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let label = spec.label.to_lowercase();
    if label == q {
        return Some(300);
    }
    if label.starts_with(&q) {
        return Some(240 - (label.len() as i32 - q.len() as i32).min(40));
    }
    if label.contains(&q) {
        return Some(180);
    }
    let mut score = 0;
    for word in spec.keywords.split_whitespace() {
        if word == q {
            score = score.max(160);
        } else if word.starts_with(&q) {
            score = score.max(140);
        }
    }
    if score > 0 {
        return Some(score);
    }
    fuzzy::match_path(spec.label, &q).map(|hit| (hit.score - 20).max(1))
}

#[derive(Clone)]
pub(super) enum LaunchRow {
    Command {
        spec: &'static CommandSpec,
        detail: Option<String>,
        score: i32,
    },
    File {
        rel: String,
        path: PathBuf,
        marked: bool,
        positions: Vec<usize>,
        score: i32,
    },
}

impl LaunchRow {
    pub(super) fn group(&self) -> LaunchGroup {
        match self {
            LaunchRow::Command { spec, .. } => spec.group,
            LaunchRow::File { .. } => LaunchGroup::Folder,
        }
    }

    pub(super) fn score(&self) -> i32 {
        match self {
            LaunchRow::Command { score, .. } | LaunchRow::File { score, .. } => *score,
        }
    }

    pub(super) fn is_file(&self) -> bool {
        matches!(self, LaunchRow::File { .. })
    }
}

pub(super) const HELP_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "reading",
        &[
            ("j k, ctrl-j ctrl-k, ↑ ↓", "down and up a line"),
            ("w b e", "next word, previous word, end of word"),
            ("0 ^ $", "start of line, first non-blank, end of line"),
            ("{ }", "previous and next paragraph"),
            ("%", "jump to the matching bracket"),
            ("ctrl-d ctrl-u", "half a page down and up"),
            ("space ctrl-f, ⇧space ctrl-b", "a whole page"),
            ("gg / G", "top / bottom of the file"),
            ("g r", "review the branch's changes"),
            ("g b", "choose what the review compares with"),
            ("p", "peek at the code a changed line replaced"),
            ("H M L", "top, middle, bottom of the screen"),
            ("zz zt zb", "centre, or put the line at the top / bottom"),
            ("n N", "next and previous search match"),
            (
                "[ ]",
                "previous / next chapter; the next change in a review",
            ),
            (
                "ctrl-o ctrl-i",
                "back and forward through everywhere you have been",
            ),
            ("gd", "go to the definition of what is under the cursor"),
        ],
    ),
    (
        "files and the index",
        &[
            (
                "J K",
                "next / previous file in the folder; changed file in a review",
            ),
            ("tab", "open the index and move the keyboard into it"),
            ("j k l h", "in the index: move, open, back out"),
            ("enter, G, gg", "in the index: open, last row, first row"),
            ("⌘K", "the launcher: files, views, actions"),
            ("⌘P or ctrl-p", "find a file in the folder"),
            ("⌘⇧F", "search the contents of every file"),
            ("/ or ⌘F", "find inside this file"),
            ("q", "close the file, and go back to the one you came from"),
            ("⌘O", "open a folder to read"),
        ],
    ),
    (
        "views",
        &[
            ("o", "contents"),
            ("f", "focus lens: dim everything but this block"),
            ("d", "distilled: fade types and punctuation away"),
            ("y Y", "copy the block / the line"),
            ("M", "bookmark this file"),
            ("t T", "next / previous theme"),
            ("1-6", "pick a theme"),
            ("- =", "smaller / larger type"),
            ("#", "toggle line numbers"),
            ("x", "render markdown, or its source"),
            ("r", "re-read the page and the file index"),
        ],
    ),
    (
        "settings and the window",
        &[
            ("s or ⌘,", "settings"),
            ("?", "this page"),
            ("escape", "close whatever is open"),
            ("{mod}Q", "quit zenith"),
        ],
    ),
];

#[cfg(test)]
mod page_tests {
    #[test]
    pub(super) fn the_page_ends_are_reachable_by_ordinary_motions() {
        use super::{reveal_pending, Pending};
        // A caret that lands on the first line asks for the top of the page, not just
        // the first line in view. The same at the other end.
        assert!(matches!(reveal_pending(0, 10), Pending::Start));
        assert!(matches!(reveal_pending(10, 10), Pending::End));
        assert!(matches!(reveal_pending(5, 10), Pending::Reveal(5)));
        // A one-line document is both ends; the top wins.
        assert!(matches!(reveal_pending(0, 0), Pending::Start));
    }
}

#[cfg(test)]
mod launcher_tests {
    use super::{launch_command_score, CommandSpec, Launch, LAUNCH_COMMANDS};

    fn spec(action: Launch) -> &'static CommandSpec {
        LAUNCH_COMMANDS
            .iter()
            .find(|s| s.action == action)
            .expect("a command for every action")
    }

    #[test]
    fn line_finds_line_numbers_not_contents() {
        // `Contents` has the keyword "outline"; a keyword may not be found by its
        // middle. Typing "line" means the gutter, not the table of contents.
        assert!(launch_command_score(spec(Launch::Contents), "line").is_none());
        assert!(launch_command_score(spec(Launch::LineNumbers), "line").is_some());
    }

    #[test]
    fn theme_and_focus_find_their_actions() {
        assert!(launch_command_score(spec(Launch::NextTheme), "theme").is_some());
        assert!(launch_command_score(spec(Launch::PrevTheme), "theme").is_some());
        assert!(launch_command_score(spec(Launch::FocusLens), "focus").is_some());
        assert!(launch_command_score(spec(Launch::Keys), "shortcuts").is_some());
    }

    #[test]
    fn the_keycaps_are_the_keys_the_page_uses() {
        assert_eq!(spec(Launch::Bookmark).shortcut, Some("M"));
        assert_eq!(spec(Launch::LineNumbers).shortcut, Some("#"));
        assert_eq!(spec(Launch::NextTheme).shortcut, Some("t"));
        assert_eq!(spec(Launch::PrevTheme).shortcut, Some("T"));
        assert_eq!(spec(Launch::Settings).shortcut, Some("s"));
        assert_eq!(spec(Launch::Keys).shortcut, Some("?"));
        assert_eq!(
            spec(Launch::SoftWrap).shortcut,
            None,
            "soft wrap has no key"
        );
    }
}
