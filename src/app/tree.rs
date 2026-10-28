//! The folder tree and the contents drawer.
//!
//! Both are indexes of the same reading: one is the shape of the folder, the other the
//! shape of the file. The tree keeps its selection by identity rather than by row
//! number, so expanding a folder cannot quietly move the reader to another file.

use super::widgets::*;
use super::*;

/// The tree's own state: the rows on screen, the keyboard focus, and the selection held
/// by identity so expanding a folder cannot move it to another file.
#[derive(Default)]
pub(super) struct TreeState {
    pub(super) rows: Vec<TreeRow>,
    pub(super) focus: bool,
    pub(super) selected: Option<TreeKey>,
    pub(super) moved: bool,
    pub(super) hook_done: bool,
    pub(super) hook_step: u8,
}

impl Zenith {
    /// The index of the selected row, falling back to the file being read.
    ///
    /// Opening the tree should put you where you already are, not at the top of a list of
    /// twenty folders.
    pub(super) fn tree_index(&self) -> Option<usize> {
        let rows = &self.tree.rows;
        if rows.is_empty() {
            return None;
        }
        if let Some(key) = &self.tree.selected {
            if let Some(i) = rows.iter().position(|r| &r.key() == key) {
                return Some(i);
            }
        }
        let here = self.open.as_ref().map(|o| o.path.clone());
        here.and_then(|p| {
            rows.iter()
                .position(|r| matches!(r.key(), TreeKey::File(q) if q == p))
        })
    }

    pub(super) fn tree_move(&mut self, delta: i32) -> bool {
        let len = self.tree.rows.len();
        if len == 0 {
            return false;
        }
        let idx = walk_index(len, self.tree_index(), delta);
        let key = self.tree.rows[idx].key();
        let changed = self.tree.selected.as_ref() != Some(&key);
        self.tree.selected = Some(key);
        changed
    }

    /// `l`, Right or Enter: go in, or open.
    pub(super) fn tree_open(&mut self) {
        let Some(idx) = self.tree_index() else { return };
        match self.tree.rows[idx].clone() {
            TreeRow::Dir { rel, open, .. } => {
                if open {
                    // Already open: step into it, the way a file manager does.
                    self.tree_move(1);
                } else {
                    self.expanded.insert(rel);
                }
            }
            TreeRow::File { path, .. } => {
                // The page follows the tree, but the keyboard stays here: browsing a
                // folder should not throw you back to the text on every file.
                self.open_file(&path);
            }
        }
    }

    /// `h` or Left: out of a folder, or up to whatever this row belongs to.
    pub(super) fn tree_back(&mut self) {
        let Some(idx) = self.tree_index() else { return };
        if let TreeRow::Dir {
            rel, open: true, ..
        } = &self.tree.rows[idx]
        {
            let rel = rel.clone();
            self.expanded.remove(&rel);
            return;
        }
        if let Some(parent) = parent_index(&self.tree.rows, idx) {
            let key = self.tree.rows[parent].key();
            self.tree.selected = Some(key);
            self.tree.moved = true;
        }
    }

    /// The keyboard while the tree has focus. Every key is accounted for, so the page
    /// never moves behind the reader's back.
    pub(super) fn tree_key(&mut self, key: Key, shift: bool, ctrl: bool) {
        let page = ((self.view_h / 21.0) as i32).max(4);
        let moved = match key {
            Key::J | Key::ArrowDown => self.tree_move(1),
            Key::K | Key::ArrowUp => self.tree_move(-1),
            Key::PageDown | Key::Space if !shift => self.tree_move(page),
            Key::PageUp | Key::Space => self.tree_move(-page),
            Key::D if ctrl => self.tree_move(page / 2),
            Key::U if ctrl => self.tree_move(-page / 2),
            Key::G if shift => {
                let last = self.tree.rows.len().saturating_sub(1);
                let key = self.tree.rows.get(last).map(|r| r.key());
                let changed = key.is_some() && self.tree.selected != key;
                self.tree.selected = key;
                changed
            }
            Key::L | Key::ArrowRight | Key::Enter => {
                self.tree_open();
                false
            }
            Key::H | Key::ArrowLeft => {
                self.tree_back();
                false
            }
            Key::Tab => {
                self.tree.focus = false;
                false
            }
            // `/` is how a reader looks for a file, and this is the file list.
            Key::Slash => {
                self.open_palette_files();
                false
            }
            _ => false,
        };
        if moved {
            self.tree.moved = true;
        }
    }

    /// The folder panel's rows, in the order they appear on screen.
    ///
    /// Built before the keyboard is read each frame, because both the navigation and the
    /// drawing have to mean the same thing by "the row below this one".
    pub(super) fn build_tree_rows(&self) -> Vec<TreeRow> {
        let mut out: Vec<TreeRow> = Vec::new();
        if let Some(folder) = &self.folder {
            let mut acc = String::new();
            flatten(&folder.tree, &self.expanded, 0, &mut acc, &mut out);
        }
        out
    }

    pub(super) fn folder_panel(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let font = self.typo.code_id(PANEL_ROW_FONT * self.scale);
        let label_font = self.typo.ui_id(PANEL_LABEL_FONT * self.scale);

        // Collect what we need before mutating. The rows come from the one list the
        // keyboard walks, so a row and its key action can never disagree about which row
        // is which.
        let rows: Vec<TreeRow> = self.tree.rows.clone();
        let tree_files: Vec<TreeRow> = rows.clone();
        let folder_name = self
            .folder
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let current = self.open.as_ref().map(|o| o.path.clone());

        let mut clicked_file: Option<PathBuf> = None;
        let mut toggled_dir: Option<String> = None;

        let panel_rect = egui::Panel::left("zenith-folder")
            .resizable(true)
            .default_size(252.0)
            .min_size(200.0)
            .max_size(340.0)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(PANEL_MARGIN_X, PANEL_MARGIN_Y)),
            )
            .show(ui, |ui| {
                // With no folder open the panel stays empty: the page already offers to
                // open one, and being told twice is noise.
                if !folder_name.is_empty() {
                    ui.add_space(6.0);
                    ui.painter().hline(
                        ui.min_rect().left()..=ui.min_rect().right(),
                        ui.cursor().top(),
                        Stroke::new(1.0, pal.edge),
                    );
                    ui.add_space(6.0);
                    paint_tracked(
                        ui.painter(),
                        Pos2::new(
                            ui.cursor().left() + 1.0,
                            ui.cursor().top() + PANEL_LABEL_DROP,
                        ),
                        Align2::LEFT_CENTER,
                        &format!("FOLDER · {folder_name}"),
                        label_font.clone(),
                        pal.ink_soft,
                        LABEL_TRACKING * PANEL_LABEL_FONT * self.scale,
                    );
                }
                if let Some(s) = &self.folder {
                    // No file count, no folder count, no total size: none of it is what
                    // a reader came for. Only a fault gets a line of its own.
                    let mut warn = String::new();
                    if s.unreadable > 0 {
                        warn.push_str(&format!("{} unreadable", s.unreadable));
                    }
                    if !warn.is_empty() {
                        ui.painter().text(
                            Pos2::new(ui.cursor().left() + 6.0, ui.cursor().top() + 22.0),
                            Align2::LEFT_CENTER,
                            warn,
                            label_font.clone(),
                            pal.accent,
                        );
                        ui.add_space(16.0);
                    }
                    // What a filter left out is worth saying once, on hover, because a
                    // reader who cannot see a file should be able to find out why.
                    if s.filtered > 0 {
                        let note = format!(
                            "{} file{} not shown: not source, hidden, or build output \
                             (settings › page)",
                            s.filtered,
                            if s.filtered == 1 { "" } else { "s" }
                        );
                        ui.interact(
                            Rect::from_min_size(
                                Pos2::new(ui.cursor().left(), ui.cursor().top()),
                                vec2(ui.available_width(), 16.0),
                            ),
                            ui.id().with("folder-filter"),
                            Sense::hover(),
                        )
                        .on_hover_text(RichText::new(note).font(label_font.clone()));
                    }
                }
                ui.add_space(PANEL_LABEL_GAP);

                ScrollArea::vertical()
                    .id_salt("zenith-tree")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in &tree_files {
                            let key = item.key();
                            let picked =
                                self.tree.focus && self.tree.selected.as_ref() == Some(&key);
                            let (name, depth, doorway, empty_file, path, rel, is_open) = match item
                            {
                                TreeRow::Dir {
                                    name,
                                    depth,
                                    rel,
                                    open,
                                } => (name, *depth, false, false, None, Some(rel.clone()), *open),
                                TreeRow::File {
                                    name,
                                    path,
                                    depth,
                                    doorway,
                                    empty,
                                } => (
                                    name,
                                    *depth,
                                    *doorway,
                                    *empty,
                                    Some(path.clone()),
                                    None,
                                    false,
                                ),
                            };
                            let on_current =
                                path.as_ref().is_some_and(|p| current.as_ref() == Some(p));
                            let ink = if empty_file {
                                pal.ink_faint
                            } else if picked || on_current || doorway {
                                pal.ink
                            } else {
                                pal.ink_soft
                            };
                            let resp = row(
                                ui,
                                &pal,
                                name,
                                if doorway {
                                    self.typo.marks.pick("◦")
                                } else {
                                    ""
                                },
                                depth,
                                picked,
                                ink,
                                &font,
                                None,
                                if rel.is_some() { Some(is_open) } else { None },
                            );
                            if self.tree.moved && picked {
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                            if resp.clicked() {
                                self.tree.focus = true;
                                self.tree.selected = Some(key);
                                match item {
                                    TreeRow::Dir { .. } => toggled_dir = rel,
                                    TreeRow::File { path, .. } => clicked_file = Some(path.clone()),
                                }
                            }
                        }
                    });
            })
            .response
            .rect;

        if let Some(rel) = toggled_dir {
            if !self.expanded.remove(&rel) {
                self.expanded.insert(rel);
            }
        }
        if let Some(path) = clicked_file {
            self.open_file(&path);
        }
        // One hairline between the desk and the page, not a gap.
        ui.painter().vline(
            panel_rect.right() - 0.5,
            panel_rect.y_range(),
            Stroke::new(1.0, pal.edge),
        );
    }

    pub(super) fn outline_panel(&mut self, ui: &mut Ui) {
        let pal = self.palette;
        let font = self.typo.code_id(PANEL_ROW_FONT * self.scale);
        let label_font = self.typo.ui_id(PANEL_LABEL_FONT * self.scale);
        let glyph_font = self.typo.code_id(11.0 * self.scale);

        let items: Vec<(String, String, usize, usize, String, SymKind)> = self
            .open
            .as_ref()
            .map(|o| {
                o.outline
                    .iter()
                    .map(|s| {
                        (
                            s.name.clone(),
                            s.detail.clone(),
                            s.line,
                            s.depth,
                            self.typo.marks.pick(s.kind.mark()).to_string(),
                            s.kind,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let title = self
            .open
            .as_ref()
            .map(|o| {
                o.path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let cursor = self.caret.line;
        let some_open = self.open.is_some();
        let mut jump: Option<usize> = None;

        let panel_rect = egui::Panel::right("zenith-outline")
            .resizable(true)
            .default_size(240.0)
            .min_size(180.0)
            .max_size(340.0)
            .show_separator_line(false)
            .frame(
                Frame::NONE
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(PANEL_MARGIN_X, PANEL_MARGIN_Y)),
            )
            .show(ui, |ui| {
                // The same rule and the same rhythm as the folder panel, so the two
                // plates sit on one line across the page.
                ui.add_space(6.0);
                ui.painter().hline(
                    ui.min_rect().left()..=ui.min_rect().right(),
                    ui.cursor().top(),
                    Stroke::new(1.0, pal.edge),
                );
                ui.add_space(6.0);
                paint_tracked(
                    ui.painter(),
                    Pos2::new(
                        ui.cursor().left() + 1.0,
                        ui.cursor().top() + PANEL_LABEL_DROP,
                    ),
                    Align2::LEFT_CENTER,
                    "CONTENTS",
                    label_font.clone(),
                    pal.ink_soft,
                    LABEL_TRACKING * PANEL_LABEL_FONT * self.scale,
                );
                ui.add_space(PANEL_LABEL_GAP);
                if !some_open {
                    return;
                }
                ScrollArea::vertical()
                    .id_salt("outline")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // A file with no outline gets an empty panel, not a note about
                        // being empty: the reader can see there is nothing there.
                        if items.is_empty() {
                            return;
                        }
                        // Which symbol is the cursor inside?
                        let mut active = None;
                        for (i, (_, _, line, ..)) in items.iter().enumerate() {
                            if *line <= cursor {
                                active = Some(i);
                            } else {
                                break;
                            }
                        }
                        for (i, (name, detail, _line, depth, glyph, kind)) in
                            items.iter().enumerate()
                        {
                            let selected = active == Some(i);
                            let ink = if selected { pal.ink } else { pal.ink_soft };
                            let resp = row(
                                ui,
                                &pal,
                                name,
                                glyph,
                                (*depth).min(6),
                                selected,
                                ink,
                                &font,
                                None,
                                None,
                            );
                            if resp.clicked() {
                                jump = Some(*_line);
                            }
                            if resp.hovered() && !detail.is_empty() {
                                resp.on_hover_text(
                                    RichText::new(format!("{} · {detail}", kind.label()))
                                        .font(glyph_font.clone()),
                                );
                            }
                        }
                    });
                let _ = title;
            })
            .response
            .rect;

        // The same hairline as the folder panel's, on its own side.
        ui.painter().vline(
            panel_rect.left() + 0.5,
            panel_rect.y_range(),
            Stroke::new(1.0, pal.edge),
        );

        if let Some(line) = jump {
            let caret = self
                .open
                .as_ref()
                .map(|o| vim::landing(&o.doc, line))
                .unwrap_or_default();
            self.jump_to(caret);
        }
    }
}

/// One visible line of the folder tree.
#[derive(Clone)]
pub(super) enum TreeRow {
    Dir {
        name: String,
        rel: String,
        depth: usize,
        open: bool,
    },
    File {
        name: String,
        path: PathBuf,
        depth: usize,
        doorway: bool,
        empty: bool,
    },
}

/// What a row *is*, independent of where it currently sits in the list.
///
/// The selection is kept as one of these rather than as an index, because expanding a
/// folder inserts rows above the selection: an index would slide to another file, which
/// is exactly the kind of quiet misbehaviour a keyboard interface cannot afford.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum TreeKey {
    Dir(String),
    File(PathBuf),
}

impl TreeRow {
    pub(super) fn key(&self) -> TreeKey {
        match self {
            TreeRow::Dir { rel, .. } => TreeKey::Dir(rel.clone()),
            TreeRow::File { path, .. } => TreeKey::File(path.clone()),
        }
    }

    /// For `h`: which row is this row's parent, if it has one on screen?
    fn is_child_of(&self, other: &TreeRow) -> bool {
        match (self, other) {
            (
                TreeRow::Dir { depth, .. } | TreeRow::File { depth, .. },
                TreeRow::Dir {
                    depth: pd, open, ..
                },
            ) => *open && *pd + 1 == *depth,
            // A row at the top has no row above it to go back to.
            _ => false,
        }
    }
}

/// Where does the selection go? Clamped, so the ends of the list are the ends of the list.
fn walk_index(len: usize, current: Option<usize>, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    match current {
        None => 0,
        Some(i) => (i as i32 + delta).clamp(0, len as i32 - 1) as usize,
    }
}

/// The row that the row at `idx` belongs to, for `h`.
///
/// Not simply "the row above": a folder's children are its own, and a repository belongs
/// it belongs to. Walking up means finding the nearest row above that contains it.
fn parent_index(rows: &[TreeRow], idx: usize) -> Option<usize> {
    let row = rows.get(idx)?;
    (0..idx).rev().find(|i| row.is_child_of(&rows[*i]))
}

/// Flatten the parts of the tree the reader has opened, and only those.
fn flatten(
    entries: &[Entry],
    expanded: &BTreeSet<String>,
    depth: usize,
    acc: &mut String,
    out: &mut Vec<TreeRow>,
) {
    for e in entries {
        match e {
            Entry::Dir(d) => {
                let mark = acc.len();
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(&d.name);
                let rel = acc.clone();
                let open = expanded.contains(&rel);
                out.push(TreeRow::Dir {
                    name: d.name.clone(),
                    rel,
                    depth,
                    open,
                });
                if open {
                    flatten(&d.children, expanded, depth + 1, acc, out);
                }
                acc.truncate(mark);
            }
            Entry::File(f) => out.push(TreeRow::File {
                name: f.name.clone(),
                path: f.path.clone(),
                depth,
                doorway: f.doorway,
                empty: f.size == 0,
            }),
        }
    }
}

#[cfg(test)]
mod tree_tests {
    use super::{parent_index, walk_index, TreeRow};

    fn dir(rel: &str, depth: usize, open: bool) -> TreeRow {
        TreeRow::Dir {
            name: rel.rsplit('/').next().unwrap_or(rel).to_string(),
            rel: rel.to_string(),
            depth,
            open,
        }
    }

    fn file(name: &str, depth: usize) -> TreeRow {
        TreeRow::File {
            name: name.to_string(),
            path: std::path::PathBuf::from(name),
            depth,
            doorway: false,
            empty: false,
        }
    }

    #[test]
    fn movement_stops_at_both_ends() {
        // The reader should be able to lean on `j` without the selection escaping.
        assert_eq!(
            walk_index(5, None, 1),
            0,
            "nothing selected starts at the top"
        );
        assert_eq!(walk_index(5, Some(0), -1), 0);
        assert_eq!(walk_index(5, Some(4), 1), 4);
        assert_eq!(walk_index(5, Some(2), 1), 3);
        assert_eq!(walk_index(5, Some(2), -1), 1);
        assert_eq!(walk_index(5, Some(3), 10), 4);
        assert_eq!(walk_index(0, None, 1), 0);
    }

    #[test]
    fn going_back_finds_the_folder_you_came_from() {
        let rows = vec![
            dir("src", 0, true),
            file("src/main.rs", 1),
            dir("src/app", 1, true),
            file("src/app/ui.rs", 2),
            file("src/lib.rs", 1),
            dir("docs", 0, false),
        ];
        // A nested file belongs to its folder, not to the row above it.
        assert_eq!(parent_index(&rows, 3), Some(2));
        // A row at the top of the tree has nothing above it to go back to.
        assert_eq!(parent_index(&rows, 0), None);
    }

    #[test]
    fn a_folder_inside_an_open_folder_belongs_to_it() {
        let rows = vec![dir("src", 0, true), dir("src/app", 1, true)];
        assert_eq!(parent_index(&rows, 1), Some(0));
    }
}
