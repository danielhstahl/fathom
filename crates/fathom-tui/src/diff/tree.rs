//! `fathom_tui::diff::tree` — the file list, grouped by taxonomy and then by directory.
//!
//! The grouping is the reviewer's actual question — "9 source, 2 test, 1 lockfile" — and not the file
//! system's, which is why ADR-006 §5.3 says the tree is grouped by taxonomy class rather than by raw
//! path. Where the class is not known (a patch file with no `X-Fathom-Class` header, which is every
//! patch until `e2.11` publishes the delta model) the group says `unknown` and the strip says why:
//! a class is a stored fact, never re-derived here (ADR-004 §6.6 R3), and a guess printed in the same
//! font as a fact is exactly the hidden-suppression ADR-001 §7.2 forbids.
//!
//! Every row is selectable, including the group headers: a header means "the thing under it", so
//! `y` on `▾ source` copies the directory and `Enter` opens the first file in it. That is cheaper to
//! explain than a cursor that silently skips rows, and it is what [`Tree::current`] does.
//!
//! Order is decreasing importance (ADR-006 §1 R3): the counts first, then source, then tests, then
//! everything else, then the group whose class is unknown, and the noise note last.

use std::collections::BTreeMap;

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
    /// A directory group: `(directory index)`.
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

/// The tree, in the order it is drawn and with the structure kept so a header row can be resolved.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    /// The rows.
    pub rows: Vec<TreeRow>,
    /// The groups, for header resolution.
    pub groups: Vec<Group>,
    /// The pane width the rows were padded to.
    pub width: usize,
    /// Files hidden by the noise rule.
    pub hidden: usize,
    /// Whether noise is being shown.
    pub show_noise: bool,
    /// The rule that hides them.
    pub noise_rule: Option<String>,
}

impl Tree {
    /// Build the tree for a scan, at a given pane width.
    #[must_use]
    pub fn build(scan: &Scan, show_noise: bool, width: usize) -> Tree {
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
        // The class order is the reviewer's reading order, not alphabetical: what they came to read
        // first is the first thing they see, and `unknown` — which is a missing fact, not a class —
        // goes last so it is never mistaken for the pile it sits beside.
        /// The class order is the reviewer's reading order, not alphabetical.
        fn rank(class: &str) -> (u8, &str) {
            let bucket = match class {
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
            };
            (bucket, class)
        }
        let mut order: Vec<(String, Vec<usize>)> = groups.into_iter().collect();
        order.sort_by_key(|(class, _)| rank(class.as_str()).0);

        let mut rows: Vec<TreeRow> = Vec::new();
        rows.push(TreeRow {
            target: Target::Aggregate,
            line: Self::aggregate(scan, show_noise, hidden, width),
        });

        let mut built: Vec<Group> = Vec::new();
        for (class, indices) in order {
            let mut dirs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for &index in &indices {
                dirs.entry(directory_of(&scan.files[index].path))
                    .or_default()
                    .push(index);
            }
            let mut group = Group {
                class: class.clone(),
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
            built.push(group);
            rows.push(TreeRow {
                target: Target::Group(group_index),
                line: Self::group_line(&built[group_index], width),
            });
            for (dir_index, dir) in built[group_index].dirs.iter().enumerate() {
                rows.push(TreeRow {
                    target: Target::Dir(dir_index),
                    line: Self::dir_line(dir, width),
                });
                for &file_index in &dir.files {
                    rows.push(TreeRow {
                        target: Target::File(file_index),
                        line: file_line(&scan.files[file_index], width),
                    });
                }
            }
        }
        if hidden > 0 {
            rows.push(TreeRow {
                target: Target::Noise,
                line: StyledLine::with_face(
                    format!(
                        "noise: {hidden} hidden by `{}` (b)",
                        scan.noise_rule().unwrap_or_else(|| "noise".to_owned())
                    ),
                    Face::Warning,
                )
                .padded(width),
            });
        } else if show_noise && scan.hidden_noise() > 0 {
            rows.push(TreeRow {
                target: Target::Noise,
                line: StyledLine::with_face(
                    format!(
                        "noise shown: {} file(s) marked `[n]` (b hides them again)",
                        scan.hidden_noise()
                    ),
                    Face::Muted,
                )
                .padded(width),
            });
        }
        Tree {
            rows,
            groups: built,
            width,
            hidden,
            show_noise,
            noise_rule: scan.noise_rule(),
        }
    }

    /// How many rows there are, which is what the shell clamps a selection against.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Is the tree empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The row at a selection, if the selection is inside the tree.
    #[must_use]
    pub fn row(&self, selection: usize) -> Option<&TreeRow> {
        self.rows.get(selection)
    }

    /// The file the selection means: a file row means itself, a header row means the first file under
    /// it, and nothing after it means the first file in the tree.
    #[must_use]
    pub fn current(&self, selection: usize) -> Option<usize> {
        match self.row(selection).map(|row| row.target)? {
            Target::File(index) => Some(index),
            _ => self
                .rows
                .iter()
                .skip(selection + 1)
                .find_map(|row| match row.target {
                    Target::File(index) => Some(index),
                    _ => None,
                })
                .or_else(|| {
                    self.rows.iter().find_map(|row| match row.target {
                        Target::File(index) => Some(index),
                        _ => None,
                    })
                }),
        }
    }

    /// Which file a selection row names, following the row down to the first file under it the way
    /// [`Tree::current`] does — a directory row names the first file inside it.
    #[must_use]
    pub fn current_at(&self, _scan: &Scan, selection: usize) -> Option<usize> {
        self.current(selection)
    }

    /// The selection a given file would be at, for a jump that keeps the cursor honest.
    #[must_use]
    pub fn row_of(&self, file: usize) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| row.target == Target::File(file))
    }

    /// The path a selection means: a file's path, or a directory's with a trailing slash.
    #[must_use]
    pub fn path_of(&self, scan: &Scan, selection: usize) -> Option<String> {
        match self.row(selection)?.target {
            Target::File(index) => Some(scan.files[index].path.clone()),
            Target::Dir(index) => self
                .groups
                .iter()
                .flat_map(|group| group.dirs.iter())
                .nth(index)
                .map(|dir| dir.path.clone()),
            Target::Aggregate | Target::Noise => None,
            Target::Group(group) => self.groups.get(group).and_then(|group| {
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

    /// The counts row, which is the strip's answer and the tree's first line.
    fn aggregate(scan: &Scan, show_noise: bool, hidden: usize, width: usize) -> StyledLine {
        let (adds, dels) = scan.totals();
        let mut line = StyledLine::new();
        line.push(
            Face::Emphasis,
            &format!(
                "{} files",
                crate::app::format_thousands(scan.files.len() as u64)
            ),
        );
        line.push_str(" · ");
        line.push(Face::Added, &format!("+{adds}"));
        line.push_str(" ");
        line.push(Face::Removed, &format!("−{dels}"));
        if hidden > 0 {
            line.push_str(" · noise ");
            line.push(Face::Warning, &hidden.to_string());
            line.push_str(" (b)");
        } else if show_noise && scan.hidden_noise() > 0 {
            line.push_str(" · noise shown");
        }
        line.padded(width)
    }

    fn group_line(group: &Group, width: usize) -> StyledLine {
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

    fn dir_line(dir: &Directory, width: usize) -> StyledLine {
        let mut line = StyledLine::new();
        line.push_str("  ");
        line.push(Face::Path, &dir.path);
        line.push_str("  ");
        line.push(Face::Added, &format!("+{}", dir.adds));
        line.push_str(" ");
        line.push(Face::Removed, &format!("−{}", dir.dels));
        line.padded(width)
    }
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
