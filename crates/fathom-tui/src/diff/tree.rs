//! `fathom_tui::diff::tree` — the file list, grouped by taxonomy and then by directory.
//!
//! The grouping is the reviewer's actual question — "9 source, 2 test, 1 lockfile" — and not the
//! file system's, which is why ADR-006 §5.3 says the tree is grouped by taxonomy class rather than by
//! raw path. Where the class is not known (a patch file with no `X-Fathom-Class` header, which is
//! every patch until `e2.11` publishes the delta model) the group says `unknown` and the strip says
//! why: a class is a stored fact, never re-derived here (ADR-004 §6.6 R3), and a guess printed in the
//! same font as a fact is exactly the hidden-suppression ADR-001 §7.2 forbids.
//!
//! Every row is selectable, including the group headers: a header means "the thing under it", so `y`
//! on `▾ source` copies the directory and `Enter` opens the first file in it. That is cheaper to
//! explain than a cursor that silently skips rows, and it is what [`Tree::current`] does.
//!
//! Order is decreasing importance (ADR-006 §1 R3): the counts first, then source, then tests, then
//! everything else, then the group whose class is unknown, and the noise note last.
//!
//! # Virtualised (`e1.8`)
//!
//! Two things, and they are different things. The *structure* — which rows exist, what each points at,
//! and what each totals — is one pass over the index and is built once per `(index, noise)` pair and
//! shared ([`TreeShape`], an `Arc` the view caches). The *text* of a row is built when a pane asks for
//! that row, so a 5 000-file episode's tree pane costs 24 rows per frame instead of 5 001: a frame's
//! cost is the pane's height, which is the claim [`crate::materialize`] makes about hunks and now
//! makes about this list too.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use crate::style::{Face, StyledLine};

use super::model::FileEntry;
use super::scan::Scan;

/// What a row points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The counts row.
    Aggregate,
    /// A taxonomy class group: `(class index)`.
    Group(usize),
    /// A directory group, numbered inside its class: `(directory index)`.
    Dir(usize),
    /// A file: `(index into the scan's files)`.
    File(usize),
    /// The noise note.
    Noise,
}

/// One drawn row.
#[derive(Debug, Clone)]
pub struct TreeRow {
    /// What selecting it means.
    pub target: Target,
    /// The row itself, padded to the pane.
    pub line: StyledLine,
}

/// A directory, with its own totals.
#[derive(Debug, Clone)]
pub struct Directory {
    /// Path as printed, with a trailing `/`.
    pub path: String,
    /// File indices.
    pub files: Vec<usize>,
    /// Added lines under it.
    pub adds: u32,
    /// Removed lines under it.
    pub dels: u32,
}

/// A taxonomy class, with its directories.
#[derive(Debug, Clone)]
pub struct Group {
    /// The class as printed, `unknown` included.
    pub class: String,
    /// Directories, sorted.
    pub dirs: Vec<Directory>,
    /// Added lines under it.
    pub adds: u32,
    /// Removed lines under it.
    pub dels: u32,
}

/// Which row a slot draws, with no text attached.
#[derive(Debug, Clone, Copy)]
enum Slot {
    Aggregate,
    Group(usize),
    Dir { group: usize, dir: usize },
    File(usize),
    Noise,
}

/// The tree's structure: every row that exists, and nothing that has been drawn.
///
/// One pass over the index builds it — the sort is the cost, and it is the same sort the old
/// whole-tree build paid on every call — and the result is shared by every pane, strip and clamp that
/// needs the shape without needing the pixels.
#[derive(Debug, Clone, Default)]
pub struct TreeShape {
    slots: Vec<Slot>,
    groups: Vec<Group>,
    /// Files the index holds, shown or not.
    files: usize,
    /// Added lines across the index.
    adds: u32,
    /// Removed lines across the index.
    dels: u32,
    /// Files hidden by the noise rule.
    hidden: usize,
    /// Files the index marks noise, whether or not they are hidden right now.
    shown_noise: usize,
    /// Whether noise is being shown.
    show_noise: bool,
    /// The rule that hides them.
    noise_rule: Option<String>,
}

impl TreeShape {
    /// Group, sort and total one index. The only O(files) step there is, and it happens once per
    /// `(load, noise)` pair rather than once per pane per frame.
    #[must_use]
    pub fn build(scan: &Scan, show_noise: bool) -> Self {
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut hidden = 0_usize;
        for (index, file) in scan.files.iter().enumerate() {
            if !show_noise && file.annotation.is_noise() {
                hidden += 1;
                continue;
            }
            groups
                .entry(file.annotation.class_label().to_owned())
                .or_default()
                .push(index);
        }
        // The class order is the reviewer's reading order, not alphabetical.
        fn rank(class: &str) -> u8 {
            match class {
                "source" => 0,
                "test" | "tests" => 1,
                "docs" | "doc" => 2,
                "config" => 3,
                "build" => 4,
                "migration" => 5,
                "generated" => 6,
                "lockfile" => 7,
                "vendor" | "vendored" => 8,
                "unknown" => 10,
                _ => 9,
            }
        }
        let mut order: Vec<(String, Vec<usize>)> = groups.into_iter().collect();
        order.sort_by_key(|(class, _)| rank(class.as_str()));

        let mut slots: Vec<Slot> = vec![Slot::Aggregate];
        let mut built: Vec<Group> = Vec::new();
        for (class, indices) in order {
            let mut dirs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for &index in &indices {
                dirs.entry(directory_of(&scan.files[index].path))
                    .or_default()
                    .push(index);
            }
            let mut group = Group {
                class,
                dirs: Vec::new(),
                adds: 0,
                dels: 0,
            };
            for (path, mut files) in dirs {
                files.sort_by_key(|&index| scan.files[index].path.clone());
                let mut node = Directory {
                    path,
                    files,
                    adds: 0,
                    dels: 0,
                };
                for &index in &node.files {
                    node.adds += scan.files[index].adds;
                    node.dels += scan.files[index].dels;
                }
                group.adds += node.adds;
                group.dels += node.dels;
                group.dirs.push(node);
            }
            let group_index = built.len();
            slots.push(Slot::Group(group_index));
            for (dir_index, dir) in group.dirs.iter().enumerate() {
                slots.push(Slot::Dir {
                    group: group_index,
                    dir: dir_index,
                });
                for &file_index in &dir.files {
                    slots.push(Slot::File(file_index));
                }
            }
            built.push(group);
        }
        let hidden_noise = scan.hidden_noise();
        if hidden > 0 || (show_noise && hidden_noise > 0) {
            slots.push(Slot::Noise);
        }
        let (adds, dels) = scan.totals();
        TreeShape {
            slots,
            groups: built,
            files: scan.files.len(),
            adds,
            dels,
            hidden,
            shown_noise: hidden_noise,
            show_noise,
            noise_rule: scan.noise_rule(),
        }
    }

    /// An empty shape: no index, so no rows, stated as such by whoever draws it.
    #[must_use]
    pub fn empty() -> Self {
        TreeShape::default()
    }

    /// How many rows there are, which is what the shell clamps a selection against.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Is there no row at all?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The target of one row, without drawing it.
    #[must_use]
    pub fn target(&self, selection: usize) -> Option<Target> {
        Some(match *self.slots.get(selection)? {
            Slot::Aggregate => Target::Aggregate,
            Slot::Group(group) => Target::Group(group),
            Slot::Dir { dir, .. } => Target::Dir(dir),
            Slot::File(file) => Target::File(file),
            Slot::Noise => Target::Noise,
        })
    }

    /// The file the selection means: a file row means itself, a header row means the first file under
    /// it, and nothing after it means the first file in the tree.
    #[must_use]
    pub fn current(&self, selection: usize) -> Option<usize> {
        match *self.slots.get(selection)? {
            Slot::File(index) => Some(index),
            _ => self
                .slots
                .iter()
                .skip(selection + 1)
                .find_map(|slot| match slot {
                    Slot::File(index) => Some(*index),
                    _ => None,
                })
                .or_else(|| {
                    self.slots.iter().find_map(|slot| match slot {
                        Slot::File(index) => Some(*index),
                        _ => None,
                    })
                }),
        }
    }

    /// The selection a given file would be at, for a jump that keeps the cursor honest.
    #[must_use]
    pub fn row_of(&self, file: usize) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| matches!(slot, Slot::File(index) if *index == file))
    }

    /// The first file row at or after a row, which is what `<Enter>` on a header opens.
    #[must_use]
    pub fn first_file_at_or_after(&self, selection: usize) -> Option<usize> {
        self.current(selection)
    }

    /// The path a selection means: a file's path, or a directory's with a trailing slash.
    ///
    /// A directory row copies its own directory — the one under the group it sits in, found by
    /// walking back to that group, which is the only way a numbered directory means one thing.
    #[must_use]
    pub fn path_of(&self, scan: &Scan, selection: usize) -> Option<String> {
        match *self.slots.get(selection)? {
            Slot::File(index) => Some(scan.files[index].path.clone()),
            Slot::Dir { group, dir } => self
                .groups
                .get(group)?
                .dirs
                .get(dir)
                .map(|dir| dir.path.clone()),
            Slot::Aggregate | Slot::Noise => None,
            Slot::Group(group) => self.groups.get(group).and_then(|group| {
                group.dirs.first().map(|dir| {
                    if dir.path == "./" {
                        String::new()
                    } else {
                        dir.path.clone()
                    }
                })
            }),
        }
    }

    /// The group a row belongs to, for a fold that has to know which group to fold.
    #[must_use]
    pub fn group_of(&self, selection: usize) -> Option<usize> {
        match *self.slots.get(selection)? {
            Slot::Group(group) => Some(group),
            Slot::Dir { group, .. } => Some(group),
            Slot::File(file) => (0..self.groups.len()).find(|group| {
                self.groups[*group]
                    .dirs
                    .iter()
                    .any(|dir| dir.files.contains(&file))
            }),
            Slot::Aggregate | Slot::Noise => None,
        }
    }

    /// The files hidden by the noise rule.
    #[must_use]
    pub const fn hidden(&self) -> usize {
        self.hidden
    }

    /// The groups, for header resolution and for a fold.
    #[must_use]
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }
}

/// The tree, in the order it is drawn, with the structure kept so a header row can be resolved and
/// with rows drawn only when a pane asks for them.
#[derive(Debug, Clone)]
pub struct Tree {
    shape: Arc<TreeShape>,
    /// The index the rows name, so drawing a file row is one lookup and building a tree is one
    /// reference count.
    scan: Arc<Scan>,
    width: usize,
}

impl Default for Tree {
    fn default() -> Self {
        Tree {
            shape: Arc::new(TreeShape::default()),
            scan: Arc::new(Scan::empty("no index")),
            width: 0,
        }
    }
}

impl Tree {
    /// Build the tree for a scan, at a given pane width.
    ///
    /// A view that draws this more than once per load should hold a [`TreeShape`] and build its trees
    /// from it ([`Tree::from_shape`]) — that is what [`super::DiffView::tree`] does.
    #[must_use]
    pub fn build(scan: &Scan, show_noise: bool, width: usize) -> Tree {
        // A view that builds a tree more than once per load shares the index `Arc` instead (see
        // [`Tree::from_shape`]): this copy is a full index clone, which is exactly the cost the
        // virtualised tree was built to stop paying.
        Tree::from_shape(
            Arc::new(TreeShape::build(scan, show_noise)),
            Arc::new(scan.clone()),
            width,
        )
    }

    /// The same tree over an index the caller already shares: no copy, no pass over the files.
    #[must_use]
    pub fn build_shared(scan: &Arc<Scan>, show_noise: bool, width: usize) -> Tree {
        Tree::from_shape(
            Arc::new(TreeShape::build(scan, show_noise)),
            Arc::clone(scan),
            width,
        )
    }

    /// A tree over a shape and an index someone already holds: one `Arc` clone, no pass over the
    /// files. This is what a frame builds per pane.
    #[must_use]
    pub fn from_shape(shape: Arc<TreeShape>, scan: Arc<Scan>, width: usize) -> Tree {
        Tree { shape, scan, width }
    }

    /// The shape this tree draws rows from, for a caller that wants structure without text.
    #[must_use]
    pub fn shape(&self) -> &TreeShape {
        &self.shape
    }

    /// How many rows there are, which is what the shell clamps a selection against.
    #[must_use]
    pub fn len(&self) -> usize {
        self.shape.len()
    }

    /// Is the tree empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shape.is_empty()
    }

    /// The pane width the rows are padded to.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// The row at a selection, drawn, if the selection is inside the tree.
    #[must_use]
    pub fn row(&self, selection: usize) -> Option<TreeRow> {
        let slot = self.shape.slots.get(selection).copied()?;
        Some(self.shape.draw_row(slot, &self.scan.files, self.width))
    }

    /// The rows in `range`, clipped to the tree. This is the windowed path: a pane asks for the rows
    /// it can see and pays for exactly those.
    #[must_use]
    pub fn window(&self, range: Range<usize>) -> Vec<TreeRow> {
        let start = range.start.min(self.shape.len());
        let end = range.end.min(self.shape.len());
        (start..end).filter_map(|row| self.row(row)).collect()
    }

    /// Every row, drawn. `--plain` and the text paths want this; a pane on screen does not.
    #[must_use]
    pub fn rows(&self) -> Vec<TreeRow> {
        self.window(0..self.shape.len())
    }

    /// The file the selection means, following a header row down to the first file under it.
    #[must_use]
    pub fn current(&self, selection: usize) -> Option<usize> {
        self.shape.current(selection)
    }

    /// Which file a selection row names, whatever the index it was built from.
    #[must_use]
    pub fn current_at(&self, _scan: &Scan, selection: usize) -> Option<usize> {
        self.shape.current(selection)
    }

    /// The selection a given file would be at, for a jump that keeps the cursor honest.
    #[must_use]
    pub fn row_of(&self, file: usize) -> Option<usize> {
        self.shape.row_of(file)
    }

    /// The path a selection means: a file's path, or a directory's with a trailing slash.
    #[must_use]
    pub fn path_of(&self, selection: usize) -> Option<String> {
        self.shape.path_of(&self.scan, selection)
    }

    /// The target of a row, without drawing it.
    #[must_use]
    pub fn target(&self, selection: usize) -> Option<Target> {
        self.shape.target(selection)
    }

    /// The groups, for header resolution.
    #[must_use]
    pub fn groups(&self) -> &[Group] {
        self.shape.groups()
    }

    /// Files hidden by the noise rule.
    #[must_use]
    pub fn hidden(&self) -> usize {
        self.shape.hidden()
    }

    /// Whether noise is being shown.
    #[must_use]
    pub fn show_noise(&self) -> bool {
        self.shape.show_noise
    }

    /// The rule that hides them.
    #[must_use]
    pub fn noise_rule(&self) -> Option<&str> {
        self.shape.noise_rule.as_deref()
    }
}

/// The class header row: what it groups, what is under it, and how much.
fn group_line(group: Option<&Group>, width: usize) -> StyledLine {
    let Some(group) = group else {
        return StyledLine::with_face("group is not in the index", Face::Warning).padded(width);
    };
    let mut line = StyledLine::new();
    line.push(Face::Dir, "▾ ");
    line.push(
        if group.class == "unknown" {
            Face::Warning
        } else {
            Face::Dir
        },
        &group.class,
    );
    line.push_str("  ");
    line.push(Face::Added, &format!("+{}", group.adds));
    line.push_str(" ");
    line.push(Face::Removed, &format!("−{}", group.dels));
    line.push_str(&format!("  ({} f)", total_files(group)));
    line.padded(width)
}

/// The directory row.
fn dir_line(dir: Option<&Directory>, width: usize) -> StyledLine {
    let Some(dir) = dir else {
        return StyledLine::with_face("directory is not in the index", Face::Warning).padded(width);
    };
    let mut line = StyledLine::new();
    line.push_str("  ");
    line.push(Face::Path, &dir.path);
    line.push_str("  ");
    line.push(Face::Added, &format!("+{}", dir.adds));
    line.push_str(" ");
    line.push(Face::Removed, &format!("−{}", dir.dels));
    line.padded(width)
}

/// How many files a group covers.
fn total_files(group: &Group) -> usize {
    group.dirs.iter().map(|dir| dir.files.len()).sum()
}

/// The directory a path sits in, as the tree prints it.
fn directory_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((head, _)) => format!("{head}/"),
        None => "./".to_owned(),
    }
}

/// One file's row: name, what happened, the counts, then read/dirty/noise markers.
fn file_line(file: &FileEntry, width: usize) -> StyledLine {
    let name = file.path.rsplit('/').next().unwrap_or(&file.path);
    let mut line = StyledLine::new();
    line.push_str("  ");
    line.push(Face::Text, name);
    line.push_str(" ");
    line.push(Face::Marker, file.marker());
    if let Some(modes) = file.mode_change() {
        // The kind marker already says "mode"; the arrow is the new information.
        line.push_str(" ");
        line.push(Face::Marker, &modes);
    }
    line.push_str(" ");
    line.push(Face::Added, &format!("+{}", file.adds));
    line.push_str(" ");
    line.push(Face::Removed, &format!("−{}", file.dels));
    if let Some(bytes) = file.bytes {
        line.push_str(&format!(" {} B", crate::app::format_thousands(bytes)));
    }
    let mut markers = String::new();
    if file.annotation.read == Some(true) {
        markers.push('✓');
    }
    if file.annotation.dirty == Some(true) {
        markers.push('*');
    }
    if file.annotation.is_noise() {
        markers.push_str("[n]");
    }
    if !markers.is_empty() {
        line.push_str(" ");
        line.push(Face::Marker, &markers);
    }
    if line.cells() > width && width > 4 {
        // A name that does not fit keeps its tail: `…mod.rs` is guessable, `src/st…` is not.
        return tail_out(&line, width);
    }
    line.padded(width)
}

/// Clip from the front, which is the only way a long path stays recognisable.
fn tail_out(line: &StyledLine, width: usize) -> StyledLine {
    let text = line.text();
    let mut out = StyledLine::new();
    out.push(Face::Muted, "…");
    let keep = width.saturating_sub(1);
    let mut chars: Vec<char> = text.chars().rev().take(keep.saturating_sub(1)).collect();
    chars.reverse();
    let head: String = chars.iter().collect();
    let head = head.trim_start().to_owned();
    out.push(Face::Text, &head);
    out.padded(width)
}

impl TreeShape {
    /// Draw one slot, given the file entries it names.
    fn draw_row(&self, slot: Slot, files: &[FileEntry], width: usize) -> TreeRow {
        let (target, line) = match slot {
            Slot::Aggregate => {
                let mut line = StyledLine::new();
                line.push(
                    Face::Emphasis,
                    &format!("{} files", crate::app::format_thousands(self.files as u64)),
                );
                line.push_str(" · ");
                line.push(Face::Added, &format!("+{}", self.adds));
                line.push_str(" ");
                line.push(Face::Removed, &format!("−{}", self.dels));
                if self.hidden > 0 {
                    line.push_str(" · noise ");
                    line.push(Face::Warning, &self.hidden.to_string());
                    line.push_str(" (b)");
                } else if self.show_noise && self.shown_noise > 0 {
                    line.push_str(" · noise shown");
                }
                (Target::Aggregate, line.padded(width))
            }
            Slot::Group(group) => (
                Target::Group(group),
                group_line(self.groups.get(group), width),
            ),
            Slot::Dir { group, dir } => (
                Target::Dir(dir),
                dir_line(self.groups.get(group).and_then(|g| g.dirs.get(dir)), width),
            ),
            Slot::File(file) => (
                Target::File(file),
                files
                    .get(file)
                    .map_or_else(
                        || {
                            StyledLine::with_face(
                                format!("file {file} is not in the index"),
                                Face::Warning,
                            )
                        },
                        |entry| file_line(entry, width),
                    )
                    .padded(width),
            ),
            Slot::Noise => {
                let line = if self.hidden > 0 {
                    StyledLine::with_face(
                        format!(
                            "noise: {} hidden by `{}` (b)",
                            self.hidden,
                            self.noise_rule
                                .clone()
                                .unwrap_or_else(|| "noise".to_owned())
                        ),
                        Face::Warning,
                    )
                } else {
                    StyledLine::with_face(
                        format!(
                            "noise shown: {} file(s) marked `[n]` (b hides them again)",
                            self.shown_noise
                        ),
                        Face::Muted,
                    )
                };
                (Target::Noise, line.padded(width))
            }
        };
        TreeRow { target, line }
    }
}
