//! `fathom_tui::diff::scan` — read a patch once, keep an index, keep nothing else.
//!
//! The output is an *index*: one [`FileEntry`] per file with its kind, its counts and its annotations,
//! and one [`Hunk`] record per hunk with its byte range and its row count in each of the four shapes
//! the pane can take (unified/split × whitespace-on/off). No line text is retained — the pane re-reads
//! the range it needs through [`super::lines`] — so retained cost is O(files + hunks) rather than
//! O(lines), which is the whole reason a 50 000-line diff is affordable.
//!
//! Row counts come from [`super::plan::Planner`] — the same state machine the pane uses to draw — run
//! once per shape. That is four cheap passes over the same chunked line stream and it is why an anchor
//! cannot drift from a drawing.
//!
//! # The dialect
//!
//! Anything `git diff -pu` produces, plus five optional header lines of our own, each of which the
//! store can emit when it knows something the patch format cannot say. `docs/concepts/diff-viewer.md`
//! §3 documents them with their grammar:
//!
//! ```text
//! X-Fathom-Class: source                     the taxonomy decision for this path
//! X-Fathom-Noise: lockfile churn rule=…      this path is noise, and the rule that made it so
//! X-Fathom-State: read | unread | dirty | clean
//! X-Fathom-Evidence: ev-1 epoch.rs:1-10
//! X-Fathom-Skip: oversize bytes=… cap=… | unavailable reason=… hint=… | notext
//! ```
//!
//! Unknown `X-Fathom-*` lines are ignored and noted, never fatal: the store may grow more of them and
//! an old Fathom must still render the patch. A header before the first `diff --git` applies to the
//! whole patch, which is how "this episode's diff is not available offline" arrives as one line.

use std::collections::BTreeMap;

use super::budget::Budget;
use super::lines::{LineReader, RawLine};
use super::model::{Annotation, FileEntry, FileKind, Hunk, Layout, Sign, Skip};
use super::plan::Planner;
use super::source::PatchReader;

/// What one pass over a patch produced.
#[derive(Debug, Clone)]
pub struct Scan {
    /// Where the bytes came from, copied in so the index can outlive its source.
    pub origin: String,
    /// The files, in patch order.
    pub files: Vec<FileEntry>,
    /// Anything the scan wants the reviewer to know: caps reached, clipped lines, odd headers.
    pub notes: Vec<String>,
    /// Why scanning stopped early. `None` means it read everything.
    pub stopped: Option<String>,
    /// Lines read.
    pub lines: u64,
    /// Bytes read.
    pub bytes: u64,
    /// Bytes of index retained — the number the memory cap is asserted against.
    pub retained_bytes: usize,
    /// Lines cut by the line cap.
    pub clipped_lines: u64,
    /// A patch with file headers and no `@@` anywhere: an empty diff, or not a diff.
    pub no_hunks: bool,
    /// Did anything in here look like a file header at all? Distinguishes "an empty diff" from "somebody
    /// piped a README into `fathom tui --patch`", which deserve different sentences.
    pub saw_headers: bool,
    /// The whole patch is unavailable, stated by the source or by a global `X-Fathom-Skip`.
    pub patch_skip: Option<Skip>,
    /// Annotations stated before the first file.
    pub global: Annotation,
    /// The size the source said it had, when it said.
    pub source_bytes: Option<u64>,
}

impl Scan {
    /// An index of nothing, with a reason.
    #[must_use]
    pub fn empty(origin: impl Into<String>) -> Self {
        Scan {
            origin: origin.into(),
            files: Vec::new(),
            notes: Vec::new(),
            stopped: None,
            lines: 0,
            bytes: 0,
            retained_bytes: 0,
            clipped_lines: 0,
            no_hunks: false,
            saw_headers: false,
            patch_skip: None,
            global: Annotation::default(),
            source_bytes: None,
        }
    }

    /// Total added and removed across the index, as the tree's first row prints them.
    #[must_use]
    pub fn totals(&self) -> (u32, u32) {
        self.files.iter().fold((0, 0), |(adds, dels), file| {
            (adds + file.adds, dels + file.dels)
        })
    }

    /// The file indices the tree shows: everything, or everything that is not noise.
    #[must_use]
    pub fn visible(&self, show_noise: bool) -> Vec<usize> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_index, file)| show_noise || !file.annotation.is_noise())
            .map(|(index, _file)| index)
            .collect()
    }

    /// Hunks in the index, across every file it kept.
    #[must_use]
    pub fn hunk_count(&self) -> usize {
        self.files.iter().map(|file| file.hunks.len()).sum()
    }

    /// How many files the noise rule hides.
    #[must_use]
    pub fn hidden_noise(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.annotation.is_noise())
            .count()
    }

    /// The rule behind the first hidden file, which is the one the strip prints.
    #[must_use]
    pub fn noise_rule(&self) -> Option<String> {
        self.files
            .iter()
            .find(|file| file.annotation.is_noise())
            .and_then(|file| {
                file.annotation
                    .noise_rule
                    .clone()
                    .or_else(|| file.annotation.noise.clone())
            })
    }
}

/// One pass over one patch.
///
/// There is no error to name: a read failure is recorded as a note and the index is whatever was
/// reachable before it, because a half-read patch with the reason attached beats a blank pane.
#[must_use]
pub fn scan(origin: &str, reader: &mut dyn PatchReader, budget: &Budget) -> Scan {
    let source_bytes = reader.len();
    let stop = source_bytes.unwrap_or(u64::MAX);
    let mut lines = LineReader::new(reader, 0, stop, budget);
    let mut builder = Builder::new(budget);
    let mut notes: Vec<String> = Vec::new();
    let mut read_error: Option<String> = None;

    loop {
        let line = match lines.next_line() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(err) => {
                read_error = Some(format!("the patch could not be read: {err}"));
                break;
            }
        };
        if line.oversized {
            builder.clipped += 1;
        }
        if builder.stopped.is_some() {
            continue;
        }
        if builder.retained() > budget.index_bytes {
            builder.stop(format!(
                "index stopped at the {}-byte index budget after {} of {} file(s) — the rest of \
                 the patch was not read",
                crate::app::format_thousands(budget.index_bytes as u64),
                crate::app::format_thousands(builder.files.len() as u64),
                crate::app::format_thousands(builder.seen_files as u64)
            ));
            continue;
        }
        builder.line(&line, &mut notes);
    }

    builder.finish(&mut notes);
    let mut out = Scan {
        origin: origin.to_owned(),
        files: std::mem::take(&mut builder.files),
        notes,
        stopped: builder.stopped.take(),
        lines: lines.lines,
        bytes: lines.bytes,
        retained_bytes: builder.retained,
        clipped_lines: builder.clipped,
        no_hunks: builder.saw_file && builder.hunks_seen == 0,
        saw_headers: builder.saw_file,
        patch_skip: builder.patch_skip.take(),
        global: builder.global.clone(),
        source_bytes,
    };
    if let Some(err) = read_error {
        out.notes.push(err);
    }
    if out.clipped_lines > 0 {
        out.notes.push(format!(
            "{} line(s) longer than {} bytes were cut",
            crate::app::format_thousands(out.clipped_lines),
            crate::app::format_thousands(budget.max_line_bytes as u64)
        ));
    }
    if out.files.is_empty() {
        out.notes.push(
            "no `diff --git` and no `--- `/`+++ ` pair: this is not a unified diff \
             (`git diff -u` is what Fathom reads)"
                .to_owned(),
        );
    }
    out
}

/// The index-building half of [`scan`].
struct Builder<'b> {
    budget: &'b Budget,
    files: Vec<FileEntry>,
    current: Option<File>,
    stopped: Option<String>,
    retained: usize,
    seen_files: usize,
    hunks_seen: usize,
    clipped: u64,
    saw_file: bool,
    patch_skip: Option<Skip>,
    global: Annotation,
    hunk: Option<Pending>,
    /// Lines the current hunk's header promised but has not delivered, so a hunk ends where the
    /// header says it ends and a malformed one cannot swallow the rest of the patch.
    left_old: u32,
    left_new: u32,
    in_hunk: bool,
    /// Which side the last body line was on, which is what `\ No newline at end of file` is about.
    last_sign: Sign,
}

/// The hunk being read.
struct Pending {
    header: String,
    /// The file's whitespace-hidden count when the hunk opened, so the hunk can report its own.
    hidden_before: u32,
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    byte_start: u64,
    byte_end: u64,
    /// Row index each shape's hunk header landed on, before the header was counted.
    anchors: [usize; 4],
    adds: u32,
    dels: u32,
    context: u32,
}

/// The file being read.
struct File {
    path: String,
    old_path: Option<String>,
    kind: FileKind,
    mode_old: Option<String>,
    mode_new: Option<String>,
    similarity: Option<u8>,
    adds: u32,
    dels: u32,
    hunks: Vec<Hunk>,
    skip: Option<Skip>,
    bytes: Option<u64>,
    rows: [usize; 4],
    planners: Vec<Planner>,
    annotation: Annotation,
}

/// The four shapes, in slot order (see [`super::model::slot`]).
fn planners_for(pair_run: usize) -> Vec<Planner> {
    (0..4)
        .map(|slot| {
            Planner::new(
                if slot & 2 != 0 {
                    Layout::Split
                } else {
                    Layout::Unified
                },
                slot & 1 != 0,
                pair_run,
            )
        })
        .collect()
}

impl File {
    fn new(path: String, pair_run: usize) -> Self {
        File {
            path,
            old_path: None,
            kind: FileKind::Unknown,
            mode_old: None,
            mode_new: None,
            similarity: None,
            adds: 0,
            dels: 0,
            hunks: Vec::new(),
            skip: None,
            bytes: None,
            // Row 0 of every file's doc is the file header line.
            rows: [1; 4],
            planners: planners_for(pair_run),
            annotation: Annotation::default(),
        }
    }

    /// Feed one hunk body line to every shape, and charge each shape for the rows it emitted.
    fn body(&mut self, mut op: impl FnMut(&mut Planner, &mut dyn FnMut(&super::model::Row))) {
        for planner in &mut self.planners {
            let mut counted = 0_usize;
            op(planner, &mut |_row| counted += 1);
            let slot = planner.slot();
            self.rows[slot] += counted;
        }
    }
}

impl<'b> Builder<'b> {
    fn new(budget: &'b Budget) -> Self {
        Builder {
            budget,
            files: Vec::new(),
            current: None,
            stopped: None,
            retained: 0,
            seen_files: 0,
            hunks_seen: 0,
            clipped: 0,
            saw_file: false,
            patch_skip: None,
            global: Annotation::default(),
            hunk: None,
            left_old: 0,
            left_new: 0,
            in_hunk: false,
            last_sign: Sign::Context,
        }
    }

    fn stop(&mut self, why: String) {
        if self.stopped.is_none() {
            self.stopped = Some(why);
        }
    }

    /// Rough bytes the index holds: published plus in progress.
    fn retained(&self) -> usize {
        self.retained
            + self
                .current
                .as_ref()
                .map_or(0, |file| entry_cost(file, file.hunks.len()))
    }

    fn finish(&mut self, notes: &mut Vec<String>) {
        self.close_hunk();
        self.close_file(notes);
    }

    fn close_file(&mut self, notes: &mut Vec<String>) {
        let Some(file) = self.current.take() else {
            return;
        };
        if file.hunks.is_empty()
            && file.kind == FileKind::Unknown
            && file.skip.is_none()
            && file.annotation.is_empty()
        {
            // A preamble that named no change at all: `git diff` between identical trees says nothing,
            // and a row claiming `unknown` for it would be the tool confusing itself, not the reviewer.
            // A file the store said something about is different: "oversize", "not available offline"
            // and "this is the noise file" are all facts with no hunk text behind them, and dropping
            // the row would be the viewer silently losing a change.
            notes.push(format!(
                "`{}` carried no change header and was skipped",
                file.path
            ));
            return;
        }
        let retained = entry_cost(&file, file.hunks.len());
        self.retained += retained;
        self.files.push(FileEntry {
            path: file.path,
            old_path: file.old_path,
            kind: file.kind,
            mode_old: file.mode_old,
            mode_new: file.mode_new,
            similarity: file.similarity,
            adds: file.adds,
            dels: file.dels,
            hunks: file.hunks,
            skip: file.skip,
            bytes: file.bytes,
            rows: file.rows,
            annotation: file.annotation,
            retained_bytes: retained,
        });
        if self.files.len() >= self.budget.max_files {
            self.stop(format!(
                "index stopped at {} files — this patch touches more than that",
                crate::app::format_thousands(self.budget.max_files as u64)
            ));
        }
    }

    fn close_hunk(&mut self) {
        let Some(pending) = self.hunk.take() else {
            return;
        };
        self.in_hunk = false;
        let Some(file) = self.current.as_mut() else {
            return;
        };
        // A change block that runs to the end of the hunk is flushed by `end_hunk`, and those rows
        // have to be counted here or the anchor table and the drawing disagree about where the next
        // hunk starts — the one drift this index is explicitly built to make impossible.
        let mut flushed = [0_usize; 4];
        for (slot, planner) in file.planners.iter_mut().enumerate() {
            let mut counted = 0_usize;
            planner.end_hunk(&mut |_row| counted += 1);
            flushed[slot] = counted;
        }
        // The whitespace counter is per file, because the pairing state is; a hunk's own number is the
        // delta across it, which is what the footer adds up.
        let ws_hidden = file.planners[1]
            .hidden
            .saturating_sub(pending.hidden_before);
        let mut rows = [0_usize; 4];
        for slot in 0..4 {
            file.rows[slot] = file.rows[slot].saturating_add(flushed[slot]);
            rows[slot] = file.rows[slot].saturating_sub(pending.anchors[slot]);
        }
        file.hunks.push(Hunk {
            header: pending.header,
            old_start: pending.old_start,
            old_count: pending.old_count,
            new_start: pending.new_start,
            new_count: pending.new_count,
            byte_start: pending.byte_start,
            byte_end: pending.byte_end,
            rows,
            anchors: pending.anchors,
            adds: pending.adds,
            dels: pending.dels,
            context: pending.context,
            ws_hidden,
        });
        self.hunks_seen += 1;
        // `>=`, checked after the push: the cap is the number of hunks retained, so the stop fires on
        // the last one allowed and the index never holds one more than it says it does.
        if self.hunks_seen >= self.budget.max_hunks {
            self.stop(format!(
                "index stopped at {} hunks",
                crate::app::format_thousands(self.budget.max_hunks as u64)
            ));
        }
    }

    fn line(&mut self, line: &RawLine, notes: &mut Vec<String>) {
        let text = line.text.as_str();
        if self.in_hunk {
            let first = text.as_bytes().first().copied();
            let consumed = match first {
                Some(b'+') => {
                    self.body_add(text.get(1..).unwrap_or(""), line);
                    true
                }
                Some(b'-') => {
                    self.body_del(text.get(1..).unwrap_or(""), line);
                    true
                }
                Some(b' ') => {
                    self.body_context(text.get(1..).unwrap_or(""), line);
                    true
                }
                Some(b'\\') => {
                    self.body_no_newline();
                    true
                }
                _ => false,
            };
            if consumed {
                if let Some(pending) = self.hunk.as_mut() {
                    pending.byte_end = line.byte_start + line.byte_len;
                }
                if self.left_old == 0 && self.left_new == 0 {
                    self.close_hunk();
                }
                return;
            }
            self.close_hunk();
        }
        self.preamble(text, line, notes);
    }

    #[allow(clippy::too_many_lines)]
    fn preamble(&mut self, text: &str, line: &RawLine, notes: &mut Vec<String>) {
        if let Some(rest) = text.strip_prefix("diff --git ") {
            self.close_hunk();
            self.close_file(notes);
            self.seen_files += 1;
            self.saw_file = true;
            let (a, b) = split_git_paths(rest);
            let path = b.or(a.clone()).unwrap_or_else(|| "unknown".to_owned());
            let mut file = File::new(path, self.budget.pair_run);
            if let Some(old) = a
                && old != file.path
            {
                // `diff --git a/x b/y` with different halves is how git spells a rename on its own;
                // the `rename from`/`rename to` pair confirms it and adds the similarity.
                file.old_path = Some(old);
                file.kind = FileKind::Renamed;
            }
            self.current = Some(file);
            return;
        }
        if let Some(rest) = text.strip_prefix("--- ") {
            if self.current.is_none() {
                self.seen_files += 1;
                self.saw_file = true;
                let path = path_of(rest, "a/");
                let added = path == "/dev/null";
                let mut file = File::new(path, self.budget.pair_run);
                if added {
                    file.kind = FileKind::Added;
                }
                self.current = Some(file);
            }
            return;
        }
        if let Some(rest) = text.strip_prefix("+++ ") {
            let path = path_of(rest, "b/");
            match self.current.as_mut() {
                Some(file) => {
                    if path == "/dev/null" {
                        if file.kind != FileKind::Added {
                            file.kind = FileKind::Deleted;
                        }
                    } else if file.kind != FileKind::Added {
                        file.path = path;
                    }
                }
                None => {
                    self.seen_files += 1;
                    self.saw_file = true;
                    self.current = Some(File::new(path, self.budget.pair_run));
                }
            }
            return;
        }
        if let Some(rest) = text.strip_prefix("@@ ") {
            self.start_hunk(rest, line);
            return;
        }
        if let Some(rest) = text.strip_prefix("X-Fathom-") {
            self.extension(rest, notes);
            return;
        }
        if let Some(rest) = text.strip_prefix("old mode ") {
            self.set(|file| {
                file.mode_old = Some(rest.trim().to_owned());
                if file.kind == FileKind::Unknown {
                    file.kind = FileKind::ModeOnly;
                }
            });
            return;
        }
        if let Some(rest) = text.strip_prefix("new mode ") {
            self.set(|file| {
                file.mode_new = Some(rest.trim().to_owned());
                if file.kind == FileKind::Unknown {
                    file.kind = FileKind::ModeOnly;
                }
            });
            return;
        }
        if let Some(rest) = text.strip_prefix("new file mode ") {
            self.set(|file| {
                file.kind = FileKind::Added;
                file.mode_new = Some(rest.trim().to_owned());
            });
            return;
        }
        if let Some(rest) = text.strip_prefix("deleted file mode ") {
            self.set(|file| {
                file.kind = FileKind::Deleted;
                file.mode_old = Some(rest.trim().to_owned());
            });
            return;
        }
        if let Some(rest) = text.strip_prefix("similarity index ") {
            let percent = rest.trim_end_matches('%').trim().parse().unwrap_or(0);
            self.set(|file| file.similarity = Some(percent.min(100)));
            return;
        }
        if let Some(rest) = text.strip_prefix("rename from ") {
            self.set(|file| {
                file.old_path = Some(path_of(rest, "a/"));
                file.kind = FileKind::Renamed;
            });
            return;
        }
        if let Some(rest) = text.strip_prefix("rename to ") {
            self.set(|file| {
                file.path = path_of(rest, "b/");
                file.kind = FileKind::Renamed;
            });
            return;
        }
        if let Some(rest) = text.strip_prefix("copy from ") {
            self.set(|file| {
                file.old_path = Some(path_of(rest, "a/"));
                file.kind = FileKind::Copied;
            });
            return;
        }
        if let Some(rest) = text.strip_prefix("copy to ") {
            self.set(|file| {
                file.path = path_of(rest, "b/");
                file.kind = FileKind::Copied;
            });
            return;
        }
        if text.starts_with("Binary files ") {
            self.set(|file| {
                file.kind = FileKind::Binary;
                file.skip = Some(Skip::NoText);
            });
            return;
        }
        if text.starts_with("GIT binary patch") {
            // The literal/delta base-64 body is not line text; decoding it is `e1.8`'s, and until
            // then the pane says the bytes are there and not shown, which is the truth.
            self.set(|file| {
                file.kind = FileKind::Binary;
                file.skip = Some(Skip::NoText);
            });
            return;
        }
        if text.starts_with("Subproject ") || text.starts_with("Submodule ") {
            self.set(|file| {
                file.kind = FileKind::Submodule;
                file.skip = Some(Skip::NoText);
            });
            return;
        }
        if text.starts_with("index ") || text.is_empty() {
            return;
        }
        if self.current.is_some() {
            notes.push(format!("ignored unknown patch header: {text}"));
        }
    }

    fn set(&mut self, edit: impl FnOnce(&mut File)) {
        if let Some(file) = self.current.as_mut() {
            edit(file);
        }
    }

    fn annotation_mut(&mut self) -> &mut Annotation {
        match self.current.as_mut() {
            Some(file) => &mut file.annotation,
            None => &mut self.global,
        }
    }

    fn extension(&mut self, rest: &str, notes: &mut Vec<String>) {
        let (key, value) = match rest.split_once(':') {
            Some((key, value)) => (key.trim(), value.trim()),
            None => {
                notes.push(format!("ignored `X-Fathom-{rest}`: no `:`"));
                return;
            }
        };
        let global = self.current.is_none();
        match key {
            "Class" => self.annotation_mut().class = Some(value.to_owned()),
            "State" => {
                let annotation = self.annotation_mut();
                for word in value.split([' ', ',']) {
                    match word {
                        "read" => annotation.read = Some(true),
                        "unread" => annotation.read = Some(false),
                        "dirty" => annotation.dirty = Some(true),
                        "clean" => annotation.dirty = Some(false),
                        _ => notes.push(format!("ignored `X-Fathom-State: {word}`")),
                    }
                }
            }
            "Noise" => {
                let (rule, why) = match value.split_once(" rule=") {
                    Some((rule, why)) => (rule.trim(), why.trim().to_owned()),
                    None => (
                        value,
                        format!(
                            "noise rule `{}`",
                            value.split_whitespace().next().unwrap_or("")
                        ),
                    ),
                };
                let annotation = self.annotation_mut();
                annotation.noise = Some(rule.to_owned());
                annotation.noise_rule = Some(why);
            }
            "Evidence" => {
                let annotation = self.annotation_mut();
                for id in value.split_whitespace() {
                    annotation.evidence.push(id.to_owned());
                }
            }
            "Binary-Size" => match value.parse::<u64>() {
                Ok(bytes) => self.set(|file| file.bytes = Some(bytes)),
                Err(_) => notes.push(format!(
                    "ignored `X-Fathom-Binary-Size: {value}`: not a size"
                )),
            },
            "Skip" => {
                let skip = parse_skip(value);
                if global {
                    self.patch_skip = Some(skip);
                } else {
                    self.set(|file| file.skip = Some(skip));
                }
            }
            other => notes.push(format!("ignored unknown header `X-Fathom-{other}`")),
        }
    }

    fn start_hunk(&mut self, rest: &str, line: &RawLine) {
        let Some(file) = self.current.as_mut() else {
            return;
        };
        let header = format!("@@ {rest}");
        let (old_start, old_count, new_start, new_count) = parse_ranges(rest);
        let index = file.hunks.len();
        let mut anchors = [0_usize; 4];
        for planner in &mut file.planners {
            let slot = planner.slot();
            anchors[slot] = file.rows[slot];
            let mut counted = 0_usize;
            {
                let sink: &mut dyn FnMut(&super::model::Row) = &mut |_row| counted += 1;
                planner.begin_hunk(index, &header, old_start, new_start, sink);
            }
            file.rows[slot] += counted;
        }
        self.hunk = Some(Pending {
            hidden_before: file.planners[1].hidden,
            header,
            old_start,
            old_count,
            new_start,
            new_count,
            byte_start: line.byte_start + line.byte_len,
            byte_end: line.byte_start + line.byte_len,
            anchors,
            adds: 0,
            dels: 0,
            context: 0,
        });
        self.left_old = old_count;
        self.left_new = new_count;
        self.in_hunk = true;
        self.last_sign = Sign::Context;
        if file.kind == FileKind::Unknown {
            file.kind = FileKind::Modified;
        }
    }

    fn body_context(&mut self, text: &str, line: &RawLine) {
        let (body, crlf) = strip_cr(text);
        if let Some(file) = self.current.as_mut() {
            file.body(|planner, sink| planner.context(body.to_owned(), line.oversized, crlf, sink));
        }
        if let Some(pending) = self.hunk.as_mut() {
            pending.context += 1;
        }
        self.left_old = self.left_old.saturating_sub(1);
        self.left_new = self.left_new.saturating_sub(1);
        self.last_sign = Sign::Context;
    }

    fn body_del(&mut self, text: &str, line: &RawLine) {
        let (body, crlf) = strip_cr(text);
        if let Some(file) = self.current.as_mut() {
            file.body(|planner, sink| planner.removed(body.to_owned(), line.oversized, crlf, sink));
        }
        if let Some(pending) = self.hunk.as_mut() {
            pending.dels += 1;
        }
        self.left_old = self.left_old.saturating_sub(1);
        self.last_sign = Sign::Removed;
        if let Some(file) = self.current.as_mut() {
            file.dels += 1;
        }
    }

    fn body_add(&mut self, text: &str, line: &RawLine) {
        let (body, crlf) = strip_cr(text);
        if let Some(file) = self.current.as_mut() {
            file.body(|planner, sink| planner.added(body.to_owned(), line.oversized, crlf, sink));
        }
        if let Some(pending) = self.hunk.as_mut() {
            pending.adds += 1;
        }
        self.left_new = self.left_new.saturating_sub(1);
        self.last_sign = Sign::Added;
        if let Some(file) = self.current.as_mut() {
            file.adds += 1;
        }
    }

    fn body_no_newline(&mut self) {
        let sign = self.last_sign;
        if let Some(file) = self.current.as_mut() {
            file.body(|planner, sink| {
                planner.no_newline(
                    if sign == Sign::Context {
                        Sign::Added
                    } else {
                        sign
                    },
                    sink,
                )
            });
        }
    }
}

/// Rough index cost of one file: the struct, its strings and its hunks.
fn entry_cost(file: &File, hunks: usize) -> usize {
    let hunk_bytes = file
        .hunks
        .iter()
        .take(hunks)
        .map(|hunk| std::mem::size_of::<Hunk>() + hunk.header.len())
        .sum::<usize>();
    std::mem::size_of::<FileEntry>()
        + file.path.len()
        + file.old_path.as_deref().map_or(0, str::len)
        + file
            .annotation
            .evidence
            .iter()
            .map(String::len)
            .sum::<usize>()
        + hunk_bytes
}

/// `\r` is content, not spacing: keep the fact and hand out the text without it.
fn strip_cr(text: &str) -> (&str, bool) {
    match text.strip_suffix('\r') {
        Some(head) => (head, true),
        None => (text, false),
    }
}

/// `diff --git a/old b/new`, with the quoting git uses for odd paths.
fn split_git_paths(rest: &str) -> (Option<String>, Option<String>) {
    let body = rest.trim_end();
    match body.split_once(" b/") {
        Some((a, b)) => (
            Some(unquote(a.strip_prefix("a/").unwrap_or(a))),
            Some(unquote(b)),
        ),
        None => (Some(unquote(body.strip_prefix("a/").unwrap_or(body))), None),
    }
}

/// `--- a/path` / `+++ b/path`, minus the prefix and the tab git appends for timestamps.
fn path_of(rest: &str, prefix: &str) -> String {
    let rest = rest.split('\t').next().unwrap_or(rest);
    let rest = unquote(rest);
    rest.strip_prefix(prefix)
        .map_or_else(|| rest.clone(), str::to_owned)
}

/// Undo git's C-style quoting far enough to be right about a space, which is the case a reviewer
/// actually hits (`"my file.txt"`).
fn unquote(text: &str) -> String {
    let trimmed = text.trim_end();
    let Some(body) = trimmed
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
    else {
        return trimmed.to_owned();
    };
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `@@ -12,7 +12,9 @@ fn thing` → the four numbers.
fn parse_ranges(rest: &str) -> (u32, u32, u32, u32) {
    let body = rest.split(" @@").next().unwrap_or(rest).trim();
    let mut parts = body.split_whitespace();
    let old = parts.next().unwrap_or("-0");
    let new = parts.next().unwrap_or("+0");
    let (old_start, old_count) = range(old, '-');
    let (new_start, new_count) = range(new, '+');
    (old_start, old_count, new_start, new_count)
}

/// One `-a,b` / `+a` side of an `@@` header. A missing count is 1, as in git.
fn range(text: &str, sign: char) -> (u32, u32) {
    let body = text.trim_start_matches(sign);
    match body.split_once(',') {
        Some((start, count)) => (start.parse().unwrap_or(0), count.parse().unwrap_or(0)),
        None => (body.parse().unwrap_or(0), 1),
    }
}

/// Split a header value into words, keeping `"a quoted phrase"` in one piece.
///
/// The store's reasons are sentences (`reason="blob not in the local packfile"`), and splitting on
/// whitespace alone turns a sentence into a field of one word and prose of the rest.
fn quoted_words(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in value.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// `oversize bytes=… cap=…`, `unavailable reason=… hint=…`, `notext`.
fn parse_skip(value: &str) -> Skip {
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut kind = String::new();
    let mut prose: Vec<String> = Vec::new();
    for word in quoted_words(value) {
        if let Some((key, val)) = word.split_once('=') {
            fields.insert(key.to_owned(), val.to_owned());
        } else if kind.is_empty() {
            kind = word;
        } else {
            prose.push(word);
        }
    }
    match kind.as_str() {
        "oversize" => Skip::Oversize {
            bytes: fields
                .get("bytes")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            cap: fields
                .get("cap")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
        },
        "notext" => Skip::NoText,
        _ => Skip::Unavailable {
            reason: fields
                .get("reason")
                .cloned()
                .unwrap_or_else(|| prose.join(" "))
                .trim_matches('"')
                .to_owned(),
            hint: fields
                .get("hint")
                .map(|hint| hint.trim_matches('"').to_owned()),
        },
    }
}
