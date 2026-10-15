//! Turning a set of repository files into a [`Surface`] — and the diff form of the same question
//! (e2.5 / reco-j83).
//!
//! # The input is files, not a repository
//!
//! [`scan`] takes an iterator of `(path, bytes)`. That is the seam this whole module is built on: the
//! analysis is pure, so a golden test is a directory of fixtures, an ADR-004 §5 cache key is a function of
//! the inputs, and the git read — which is IO, and belongs to `fathom-ingest` — is one adapter at the end
//! of the file ([`scan_repository`], [`delta_repository`]). A parser that reached for `std::fs` would buy
//! none of those, and lint `W1` would not let it.
//!
//! # What a scan promises
//!
//! * It never fails because a *file* was bad. It fails only when the caller could not supply the file list
//!   at all; every per-file failure is a [`DepFile`] with [`Body::Unparsed`].
//! * It is bounded: [`Options::max_files`] and [`Options::max_blob_bytes`] cap the work, `Surface::complete`
//!   says whether the caps bit, and a capped read produces `Truncated` rather than a confident wrong parse.
//! * It says what it did not look at: `Surface::scanned` counts every path offered, and ignored
//!   directories are summarized rather than silently dropped.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::deps::discover::{self, Recognition};
use crate::deps::model::{
    Body, DepFile, Ecosystem, Format, Lock, Manifest, Role, Status, UnparsedReason,
};
use crate::deps::parse_body;
use crate::deps::text::{self, Notes};
use fathom_ingest::repo::BlobRead;

/// Caps on one scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    /// Read at most this many bytes of any one file. A `package-lock.json` over the cap is reported
    /// [`Truncated`](UnparsedReason::Truncated), which is true, rather than being parsed as if it were
    /// whole, which would not be.
    pub max_blob_bytes: u64,
    /// Recognize at most this many files in one scan, so a pathological tree cannot make a scan unbounded.
    pub max_files: usize,
    /// How many bytes of a file's head to keep with an `Unparsed` result — the "here is the raw diff"
    /// half of the ticket's degradation rule.
    pub preview_bytes: usize,
    /// Whether to keep the full parsed structures. `false` gives a scan of counts and statuses only, which
    /// is what a listing view wants before a row is opened.
    pub keep_bodies: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            // 8 MiB covers the largest lockfile in any corpus we have measured, and a real monorepo's
            // `pnpm-lock.yaml` has been seen past 4 MiB.
            max_blob_bytes: 8 * 1024 * 1024,
            max_files: 4096,
            preview_bytes: text::PREVIEW_BYTES,
            keep_bodies: true,
        }
    }
}

/// One file offered to a scan.
#[derive(Debug, Clone)]
pub struct Input<'a> {
    /// Repository-relative path.
    pub path: &'a str,
    /// Its content, or the reason there is none.
    pub content: Content<'a>,
}

/// What the caller could supply for a path.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    /// The bytes, already capped by the caller if the caller is the one capping them.
    Bytes(&'a [u8]),
    /// The file exists at this revision and was not read; `size` is its blob size if known.
    Unread {
        /// The size, if the tree entry gave it.
        size: Option<u64>,
        /// Whose failure this is, in the reader's words.
        why: &'a str,
    },
    /// The path is gone on this side of a diff.
    Absent,
}

/// One repository's dependency surface at one revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    /// Every recognized file, sorted by path.
    pub files: Vec<DepFile>,
    /// The packages the scan found, one per manifest directory, sorted by directory.
    pub members: Vec<Member>,
    /// The counts, already summed.
    pub totals: Totals,
    /// Scan-level statements: caps that bit, directories that were skipped, manifests no workspace claims.
    pub notes: Vec<String>,
    /// How many paths the scan was offered, recognized or not.
    pub scanned: usize,
    /// Whether the scan looked at everything it was offered.
    pub complete: bool,
}

impl Surface {
    /// The files of one ecosystem, in path order.
    pub fn files_of(&self, ecosystem: Ecosystem) -> Vec<&DepFile> {
        self.files
            .iter()
            .filter(|file| file.ecosystem == ecosystem)
            .collect()
    }

    /// Every file whose content this build could not use — the list a reviewer should see first.
    pub fn unparsed(&self) -> Vec<&DepFile> {
        self.files
            .iter()
            .filter(|file| file.status == Status::Unparsed)
            .collect()
    }

    /// Whether any file at all was recognized.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// One package in the surface: a directory with a manifest, plus the locks that cover it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// The directory, `""` for the repository root.
    pub dir: String,
    /// The ecosystem, taken from its manifest.
    pub ecosystem: Ecosystem,
    /// The package name as the manifest states it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The manifest path.
    pub manifest: String,
    /// The lockfiles that cover it.
    pub locks: Vec<String>,
    /// Declared requirements in the manifest.
    pub direct: usize,
    /// Resolved packages in its lockfiles.
    pub resolved: usize,
    /// The worst status of its manifest and locks — a member is only as good as its worst file.
    pub status: Status,
    /// Whether a workspace glob in a parent manifest or config claims this member.
    pub claimed: bool,
}

/// The sums, kept next to the surface so a view never recomputes them differently.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    /// Recognized files.
    pub files: usize,
    /// Of which manifests.
    pub manifests: usize,
    /// Of which lockfiles.
    pub locks: usize,
    /// Of which configs (`pnpm-workspace.yaml`, `go.work`).
    pub configs: usize,
    /// Fully parsed.
    pub parsed: usize,
    /// Parsed with named skips.
    pub partial: usize,
    /// Not parsed at all.
    pub unparsed: usize,
    /// Declared requirements across every manifest.
    pub direct: usize,
    /// Of which runtime (not dev, not build, not peer, not an extra).
    pub runtime_direct: usize,
    /// Resolved packages across every lockfile, per-file (a package in two locks counts twice).
    pub resolved: usize,
    /// Bytes read.
    pub bytes: u64,
    /// Recognized files per ecosystem.
    pub by_ecosystem: BTreeMap<String, usize>,
}

/// Scan a set of files.
///
/// Order-independent: the output is sorted whatever order the caller offers.
#[must_use]
pub fn scan<'a>(inputs: impl IntoIterator<Item = Input<'a>>, options: &Options) -> Surface {
    let mut files: Vec<DepFile> = Vec::new();
    let mut scanned = 0usize;
    let mut ignored: BTreeMap<(&'static str, &'static str), usize> = BTreeMap::new();
    let mut notes: Vec<String> = Vec::new();
    let mut complete = true;
    for input in inputs {
        scanned += 1;
        if files.len() >= options.max_files {
            complete = false;
            notes.push(format!(
                "the scan stopped at {} recognized files; the rest of the tree was not read",
                options.max_files
            ));
            break;
        }
        match discover::recognize(input.path) {
            Recognition::NotDependency => {}
            Recognition::Ignored(reason) => {
                let directory = discover::ignored_directory(input.path).unwrap_or("this");
                *ignored.entry((directory, reason)).or_insert(0) += 1;
            }
            Recognition::Recognized(format) => files.push(parse_one(input, format, options)),
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let notes_and_members = group(&files, &mut notes);
    for ((directory, reason), count) in ignored {
        notes.push(format!(
            "{count} path(s) under `{directory}/` skipped: {reason}"
        ));
    }
    let totals = totals_of(&files);
    Surface {
        files,
        members: notes_and_members,
        totals,
        notes,
        scanned,
        complete,
    }
}

/// Parse one offered file, including every way it can decline to be parsed.
fn parse_one<'a>(input: Input<'a>, format: Format, options: &Options) -> DepFile {
    let bytes = match input.content {
        Content::Bytes(bytes) => bytes,
        Content::Unread { size, why } => {
            let mut file = DepFile::unparsed(
                format,
                input.path,
                UnparsedReason::ContentUnavailable,
                why,
                String::new(),
            );
            if let Some(size) = size {
                file = file.with_size_on_disk(size);
            }
            return file;
        }
        Content::Absent => {
            return DepFile::unparsed(
                format,
                input.path,
                UnparsedReason::ContentUnavailable,
                "the path is not present on this side of the diff",
                String::new(),
            );
        }
    };
    let size = bytes.len() as u64;
    if bytes.is_empty() {
        return DepFile::unparsed(
            format,
            input.path,
            UnparsedReason::Empty,
            "the file is empty",
            String::new(),
        )
        .with_bytes(0);
    }
    if size > options.max_blob_bytes {
        return DepFile::unparsed(
            format,
            input.path,
            UnparsedReason::TooLarge,
            format!(
                "{size} bytes, over the {0}-byte cap",
                options.max_blob_bytes
            ),
            String::new(),
        )
        .with_bytes(0)
        .with_size_on_disk(size);
    }
    let mut notes = Notes::new();
    let text = match text::decode(bytes) {
        Ok(text) => text,
        Err(reason) => {
            // Even here the result carries something to look at: see `text::preview_binary`.
            let (preview, _) = text::preview_binary(bytes, options.preview_bytes);
            return DepFile::unparsed(
                format,
                input.path,
                reason,
                "the content is not text this parser can read",
                preview,
            )
            .with_bytes(size)
            .with_size_on_disk(size);
        }
    };
    match parse_body(format, &text, &mut notes) {
        Ok(body) => {
            let (parsed_notes, status) = notes.finish();
            let mut file = DepFile::parsed(
                format,
                input.path,
                if options.keep_bodies {
                    body
                } else {
                    body.summarize()
                },
            )
            .with_bytes(size)
            .with_size_on_disk(size)
            .with_status(status)
            .with_notes(parsed_notes);
            if size == options.max_blob_bytes {
                file.notes
                    .push("the read stopped at the size cap; the file may continue".to_owned());
                file.status = Status::Partial;
            }
            file
        }
        Err(failure) => {
            let (preview, truncated) = text::preview(&text, options.preview_bytes);
            let mut file =
                DepFile::unparsed(format, input.path, failure.reason, failure.detail, preview)
                    .with_bytes(size)
                    .with_size_on_disk(size);
            // A cut-off read explains most "malformed" lockfiles we have seen, and the difference is the
            // whole advice: "your lockfile is corrupt" versus "raise the cap".
            if size >= options.max_blob_bytes
                && failure.reason == UnparsedReason::MalformedSyntax
                && let Body::Unparsed(unparsed) = &mut file.body
            {
                unparsed.reason = UnparsedReason::Truncated;
            }
            let (mut failure_notes, _) = notes.finish();
            failure_notes.push(format!("cap: {size} of {size} bytes read"));
            if truncated {
                failure_notes.push(format!(
                    "preview shows the first {} bytes",
                    options.preview_bytes
                ));
            }
            file.notes = failure_notes;
            file
        }
    }
}

/// Group files into members and add the scan-level notes about membership.
fn group(files: &[DepFile], notes: &mut Vec<String>) -> Vec<Member> {
    let mut manifest_by_dir: BTreeMap<String, &DepFile> = BTreeMap::new();
    let mut locks: Vec<&DepFile> = Vec::new();
    let mut configs: Vec<&DepFile> = Vec::new();
    for file in files {
        match file.role {
            Role::Manifest => {
                let dir = discover::directory_of(&file.path);
                if let Some(previous) = manifest_by_dir.insert(dir.clone(), file) {
                    notes.push(format!(
                        "`{}` and `{}` are both {} manifests in `{dir}`",
                        previous.path,
                        file.path,
                        file.ecosystem.as_str()
                    ));
                }
            }
            Role::Lock => locks.push(file),
            Role::Config => configs.push(file),
        }
    }
    // Locks walk up to the nearest manifest of their own ecosystem: one root lock for a JS workspace, one
    // `go.sum` for a Go workspace, one `Gemfile.lock` for a single Ruby gem.
    let manifest_dirs: BTreeMap<String, Ecosystem> = manifest_by_dir
        .iter()
        .map(|(dir, file)| (dir.clone(), file.ecosystem))
        .collect();
    let mut locks_by_dir: BTreeMap<String, Vec<&DepFile>> = BTreeMap::new();
    for lock in &locks {
        let dir = discover::directory_of(&lock.path);
        let owner = discover::lock_owner(&dir, lock.ecosystem, &manifest_dirs);
        locks_by_dir.entry(owner).or_default().push(lock);
    }
    // Workspace globs, from manifests and from config files, decide who is *claimed*.
    let mut globs: Vec<(String, String)> = Vec::new();
    for file in files {
        let dir = discover::directory_of(&file.path);
        let declared: Vec<String> = match &file.body {
            Body::Manifest(Manifest { workspaces, .. }) => workspaces.clone(),
            Body::Workspace { members } => members.clone(),
            _ => Vec::new(),
        };
        for glob in declared {
            // `go.work` and `pnpm-workspace.yaml` spell membership as `./api` and `'apps/*'`; the leading `./`
            // is decoration, and leaving it in means a member is silently unclaimed.
            let glob = glob
                .trim()
                .trim_matches('\'')
                .trim()
                .trim_start_matches("./")
                .to_owned();
            if !glob.is_empty() {
                globs.push((dir.clone(), glob));
            }
        }
    }
    let mut members: Vec<Member> = Vec::new();
    for (dir, manifest) in &manifest_by_dir {
        let locks_here = locks_by_dir
            .remove(dir)
            .unwrap_or_default()
            .into_iter()
            .map(|lock| lock.path.clone())
            .collect();
        let (direct, resolved, name) = match &manifest.body {
            Body::Manifest(manifest) => (
                manifest.requirements.len(),
                0,
                manifest.name.clone().or_else(|| Some(dir.clone())),
            ),
            Body::Unparsed(_) => (0, 0, None),
            _ => (0, 0, None),
        };
        let claimed = dir.is_empty()
            || globs.iter().any(|(from, glob)| {
                let absolute = if from.is_empty() {
                    glob.clone()
                } else {
                    format!("{from}/{glob}")
                };
                discover::glob_matches(&absolute, dir) || discover::glob_matches(glob, dir)
            });
        members.push(Member {
            dir: dir.clone(),
            ecosystem: manifest.ecosystem,
            name,
            manifest: manifest.path.clone(),
            locks: locks_here,
            direct,
            resolved,
            status: manifest.status,
            claimed,
        });
    }
    // A lock with no manifest anywhere is still surface: it proves the repository installs something.
    let mut orphan_dirs: Vec<String> = locks_by_dir.keys().cloned().collect();
    orphan_dirs.sort();
    for dir in orphan_dirs {
        let orphans = locks_by_dir.remove(&dir).unwrap_or_default();
        for lock in &orphans {
            // Named, because a lock whose manifest has been deleted (or never committed) changes what the
            // resolved versions mean: there is nothing on disk declaring an intent for them to match.
            notes.push(format!(
                "`{}` is a {} lockfile with no manifest beside it",
                lock.path,
                lock.ecosystem.as_str()
            ));
            members.push(Member {
                dir: dir.clone(),
                ecosystem: lock.ecosystem,
                name: None,
                manifest: String::new(),
                locks: vec![lock.path.clone()],
                direct: 0,
                resolved: lock_resolved(lock),
                status: lock.status,
                claimed: false,
            });
        }
    }
    members.sort_by(|left, right| left.dir.cmp(&right.dir));
    // Fill in resolved counts now that every member knows its locks.
    let by_path: BTreeMap<&str, &DepFile> = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    for member in &mut members {
        member.resolved = member
            .locks
            .iter()
            .map(|path| {
                by_path
                    .get(path.as_str())
                    .map(|file| lock_resolved(file))
                    .unwrap_or(0)
            })
            .sum();
        member.status = worst(
            member.status,
            member
                .locks
                .iter()
                .filter_map(|path| by_path.get(path.as_str()).copied()),
        );
    }
    for member in &members {
        if !member.claimed && !member.dir.is_empty() {
            notes.push(format!(
                "`{}` is a {} package no workspace glob claims",
                member.manifest,
                member.ecosystem.as_str()
            ));
        }
    }
    for config in configs {
        let _ = config;
    }
    members.shrink_to_fit();
    members
}

fn worst<'b>(base: Status, files: impl Iterator<Item = &'b DepFile>) -> Status {
    let mut worst = base;
    for file in files {
        worst = match (worst, file.status) {
            (Status::Unparsed, _) | (_, Status::Unparsed) => Status::Unparsed,
            (Status::Partial, _) | (_, Status::Partial) => Status::Partial,
            _ => Status::Parsed,
        };
    }
    worst
}

fn lock_resolved(file: &DepFile) -> usize {
    match &file.body {
        Body::Lock(Lock { packages, .. }) => packages.len(),
        _ => 0,
    }
}

fn totals_of(files: &[DepFile]) -> Totals {
    let mut totals = Totals::default();
    for file in files {
        totals.files += 1;
        totals.bytes += file.bytes;
        *totals
            .by_ecosystem
            .entry(file.ecosystem.as_str().to_owned())
            .or_insert(0) += 1;
        match file.role {
            Role::Manifest => totals.manifests += 1,
            Role::Lock => totals.locks += 1,
            Role::Config => totals.configs += 1,
        }
        totals.parsed += usize::from(file.status == Status::Parsed);
        totals.partial += usize::from(file.status == Status::Partial);
        totals.unparsed += usize::from(file.status == Status::Unparsed);
        match &file.body {
            Body::Manifest(manifest) => {
                totals.direct += manifest.requirements.len();
                totals.runtime_direct += manifest
                    .requirements
                    .iter()
                    .filter(|dependency| dependency.kind.is_runtime())
                    .count();
            }
            Body::Lock(lock) => totals.resolved += lock.packages.len(),
            Body::Workspace { .. } | Body::Unparsed(_) => {}
        }
    }
    totals
}

/// What happened to one dependency file between two revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// New dependency file.
    Added,
    /// Dependency file gone.
    Deleted,
    /// Same path, different content.
    Modified,
    /// Same path, different mode only — a `chmod` on a lockfile is odd but reportable.
    ModeOnly,
}

impl ChangeKind {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "add",
            ChangeKind::Deleted => "delete",
            ChangeKind::Modified => "modify",
            ChangeKind::ModeOnly => "mode",
        }
    }
}

/// One changed path offered to [`delta`].
#[derive(Debug, Clone)]
pub struct Change<'a> {
    /// Repository-relative path.
    pub path: &'a str,
    /// The bytes before, if the path existed.
    pub before: Option<&'a [u8]>,
    /// The bytes after, if the path still exists.
    pub after: Option<&'a [u8]>,
    /// Why the path changed, if the caller knows. `None` lets [`delta`] infer it from what is present.
    pub kind: Option<ChangeKind>,
}

/// The diff-side answer: one row per changed dependency file, with both sides where they exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    /// One row per changed dependency file, sorted by path.
    pub paths: Vec<DeltaRow>,
    /// Files this build could not parse on at least one side — the list to render with the raw diff under
    /// it, which is exactly the ticket's `unparsed — here is the raw diff`.
    pub unparsed: Vec<DepFile>,
    /// The counts.
    pub totals: DeltaTotals,
    /// Scan-level statements.
    pub notes: Vec<String>,
    /// Whether the delta covered every changed path it was offered.
    pub complete: bool,
}

/// One changed dependency file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaRow {
    /// Repository-relative path.
    pub path: String,
    /// What happened to it.
    pub kind: ChangeKind,
    /// The format, taken from whichever side exists.
    pub format: Format,
    /// Ecosystem.
    pub ecosystem: Ecosystem,
    /// The before state, when the path existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<DepFile>,
    /// The after state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<DepFile>,
    /// Whether a viewer should show the raw diff instead of, or beside, the parsed comparison: true when
    /// either side failed to parse, and always true for an unparsable format.
    pub show_raw: bool,
    /// One-line summary of the difference, in counts.
    pub summary: String,
}

/// Delta counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaTotals {
    /// Changed dependency files.
    pub files: usize,
    /// Added.
    pub added: usize,
    /// Deleted.
    pub deleted: usize,
    /// Modified in content.
    pub modified: usize,
    /// Changes this build could not attribute because a side did not parse.
    pub unparsed: usize,
    /// Declared requirements added.
    pub direct_added: usize,
    /// Declared requirements removed.
    pub direct_removed: usize,
    /// Resolved packages added across all locks.
    pub resolved_added: usize,
    /// Resolved packages removed.
    pub resolved_removed: usize,
    /// Bytes diffed.
    pub bytes: u64,
}

/// Compare changed paths.
#[must_use]
pub fn delta<'a>(changes: impl IntoIterator<Item = Change<'a>>, options: &Options) -> Delta {
    let mut rows: Vec<DeltaRow> = Vec::new();
    let mut unparsed: Vec<DepFile> = Vec::new();
    let mut notes = Vec::new();
    let mut complete = true;
    let mut totals = DeltaTotals::default();
    for change in changes {
        if rows.len() >= options.max_files {
            complete = false;
            notes.push(format!(
                "the diff stopped at {} recognized paths",
                options.max_files
            ));
            break;
        }
        let format = match discover::recognize(change.path) {
            Recognition::Recognized(format) => format,
            Recognition::Ignored(reason) => {
                notes.push(format!("`{}` skipped: {reason}", change.path));
                continue;
            }
            Recognition::NotDependency => continue,
        };
        let before = change.before.map(|bytes| {
            parse_one(
                Input {
                    path: change.path,
                    content: Content::Bytes(bytes),
                },
                format,
                options,
            )
        });
        let after = change.after.map(|bytes| {
            parse_one(
                Input {
                    path: change.path,
                    content: Content::Bytes(bytes),
                },
                format,
                options,
            )
        });
        let kind = change.kind.unwrap_or(if after.is_none() {
            ChangeKind::Deleted
        } else if before.is_none() {
            ChangeKind::Added
        } else {
            ChangeKind::Modified
        });
        let show_raw = before
            .as_ref()
            .is_some_and(|file| file.status == Status::Unparsed)
            || after
                .as_ref()
                .is_some_and(|file| file.status == Status::Unparsed);
        let summary = summarize(&before, &after, kind);
        if show_raw && let Some(file) = after.as_ref().or(before.as_ref()) {
            unparsed.push(file.clone());
        }
        totals.files += 1;
        match kind {
            ChangeKind::Added => totals.added += 1,
            ChangeKind::Deleted => totals.deleted += 1,
            ChangeKind::Modified => totals.modified += 1,
            ChangeKind::ModeOnly => {}
        }
        totals.unparsed += usize::from(show_raw);
        let (direct_before, resolved_before) = counts(&before);
        let (direct_after, resolved_after) = counts(&after);
        totals.direct_added += direct_after.saturating_sub(direct_before);
        totals.direct_removed += direct_before.saturating_sub(direct_after);
        totals.resolved_added += resolved_after.saturating_sub(resolved_before);
        totals.resolved_removed += resolved_before.saturating_sub(resolved_after);
        totals.bytes += before.as_ref().map_or(0, |file| file.bytes)
            + after.as_ref().map_or(0, |file| file.bytes);
        rows.push(DeltaRow {
            path: change.path.to_owned(),
            kind,
            format,
            ecosystem: format.ecosystem(),
            before,
            after,
            show_raw,
            summary,
        });
    }
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    unparsed.sort_by(|left, right| left.path.cmp(&right.path));
    Delta {
        paths: rows,
        unparsed,
        totals,
        notes,
        complete,
    }
}

fn counts(file: &Option<DepFile>) -> (usize, usize) {
    match file.as_ref().map(|file| &file.body) {
        Some(Body::Manifest(manifest)) => (manifest.requirements.len(), 0),
        Some(Body::Lock(lock)) => (
            lock.importers.iter().map(|i| i.requirements.len()).sum(),
            lock.packages.len(),
        ),
        _ => (0, 0),
    }
}

fn summarize(before: &Option<DepFile>, after: &Option<DepFile>, kind: ChangeKind) -> String {
    let (direct_before, resolved_before) = counts(before);
    let (direct_after, resolved_after) = counts(after);
    let (path, role) = after
        .as_ref()
        .or(before.as_ref())
        .map(|file| (file.path.as_str(), file.role))
        .unwrap_or(("", Role::Manifest));
    let _ = path;
    match kind {
        ChangeKind::Added => match role {
            Role::Manifest => format!("new manifest: {direct_after} declared requirements"),
            Role::Lock => format!("new lockfile: {resolved_after} resolved packages"),
            Role::Config => "new workspace config".to_owned(),
        },
        ChangeKind::Deleted => match role {
            Role::Manifest => {
                format!("manifest removed: {direct_before} requirements went with it")
            }
            Role::Lock => format!("lockfile removed: {resolved_before} resolutions went with it"),
            Role::Config => "workspace config removed".to_owned(),
        },
        ChangeKind::ModeOnly => "only the mode changed".to_owned(),
        ChangeKind::Modified => {
            let direct = direct_after as i64 - direct_before as i64;
            let resolved = resolved_after as i64 - resolved_before as i64;
            // The path is already on the row; repeating it in the summary would double the width of the
            // one column that is already the widest thing on screen.
            match role {
                Role::Manifest => format!("declared requirements {direct:+} (now {direct_after})"),
                Role::Lock => {
                    if resolved == 0 {
                        format!("resolution content changed; {resolved_after} packages recorded")
                    } else {
                        format!("resolved packages {resolved:+} (now {resolved_after})")
                    }
                }
                Role::Config => "workspace membership changed".to_owned(),
            }
        }
    }
}

impl Body {
    /// Drop the parts a listing view will not show, keeping counts and names.
    ///
    /// This is what [`Options::keep_bodies`] toggles: a monorepo scan that keeps every lock's every edge
    /// is megabytes of graph a listing throws away, and ADR-004 §5's cache rows would carry it forever.
    #[must_use]
    pub fn summarize(self) -> Body {
        match self {
            Body::Manifest(manifest) => Body::Manifest(Manifest {
                name: manifest.name,
                version: manifest.version,
                requirements: manifest
                    .requirements
                    .into_iter()
                    .map(|mut dependency| {
                        dependency.actual = None;
                        dependency
                    })
                    .collect(),
                workspaces: manifest.workspaces,
                properties: manifest.properties,
            }),
            Body::Lock(lock) => Body::Lock(Lock {
                version: lock.version,
                packages: lock
                    .packages
                    .into_iter()
                    .map(|mut package| {
                        package.edges = Vec::new();
                        package.integrity = None;
                        package
                    })
                    .collect(),
                importers: lock.importers,
                records_edges: lock.records_edges,
                complete: lock.complete,
                properties: lock.properties,
            }),
            other => other,
        }
    }
}

/// One blob read for [`scan_repository`]: the path, what came back, and why if nothing did.
type ReadBlob = (String, Vec<u8>, Option<u64>, Option<&'static str>);

/// One changed path read for [`delta_repository`]: the path, the two sides, and the kind.
type ChangeBlobs = (String, Option<Vec<u8>>, Option<Vec<u8>>, ChangeKind);

/// The git adapter: every dependency file in `tree`, read through `fathom-ingest`.
///
/// A bounded whole-tree read — `max_files` caps the *recognized* files, so a 500k-file repository still
/// costs one tree enumeration and a handful of blob reads. That is the cold path's cost, and it is what
/// ADR-004 §6.6 R2 forbids on a warm path: the warm answer is [`delta_repository`], whose cost is
/// O(changed paths).
///
/// # Errors
///
/// Only for the failures that are not a file's fault: an unreadable tree, a missing tree object. A blob
/// that cannot be read becomes a row with [`UnparsedReason::ContentUnavailable`], and the scan still
/// returns.
pub fn scan_repository(
    repo: &fathom_ingest::repo::Repository,
    tree: fathom_ingest::oid::Oid,
    options: &Options,
) -> Result<Surface, crate::error::Error> {
    let listing = repo.tree_files(tree, None)?;
    // Read the blobs first, so the inputs can borrow the bytes rather than copy them twice.
    let mut blobs: Vec<ReadBlob> = Vec::new();
    for file in &listing.files {
        let path = file.path.to_string_lossy().replace('\\', "/");
        if matches!(discover::recognize(&path), Recognition::NotDependency) {
            continue;
        }
        match repo.blob(file.oid, options.max_blob_bytes as usize)? {
            BlobRead::Present { bytes, .. } => blobs.push((path, bytes, Some(file.size), None)),
            BlobRead::Oversized { size } => blobs.push((
                path,
                Vec::new(),
                Some(size as u64),
                Some("the blob is over the read cap"),
            )),
            BlobRead::WrongKind { .. } => blobs.push((
                path,
                Vec::new(),
                Some(file.size),
                Some("the tree entry is not a blob"),
            )),
        }
    }
    let inputs = blobs.iter().map(|(path, bytes, size, why)| Input {
        path,
        content: match why {
            Some(why) => Content::Unread { size: *size, why },
            None => Content::Bytes(bytes),
        },
    });
    let mut surface = scan(inputs, options);
    if !listing.complete() {
        surface.complete = false;
        surface
            .notes
            .push("the tree enumeration was capped; the scan is a prefix".to_owned());
    }
    Ok(surface)
}

/// The git adapter for a diff: the changed paths between two trees, with both sides read.
///
/// This is the warm path, and its cost is the changed-path walk plus one blob per changed dependency
/// file — never a tree enumeration (ADR-004 §6.6 R2, R7).
///
/// # Errors
///
/// As [`scan_repository`].
pub fn delta_repository(
    repo: &fathom_ingest::repo::Repository,
    old_tree: Option<fathom_ingest::oid::Oid>,
    new_tree: Option<fathom_ingest::oid::Oid>,
    options: &Options,
) -> Result<Delta, crate::error::Error> {
    let changes = repo.changed_paths(old_tree, new_tree)?;
    let mut blobs: Vec<ChangeBlobs> = Vec::new();
    for change in changes {
        let path = change.path.to_string_lossy().replace('\\', "/");
        if matches!(discover::recognize(&path), Recognition::NotDependency) {
            continue;
        }
        let kind = match change.kind {
            fathom_ingest::diff::PathChangeKind::Added => ChangeKind::Added,
            fathom_ingest::diff::PathChangeKind::Deleted => ChangeKind::Deleted,
            fathom_ingest::diff::PathChangeKind::ModeChanged => ChangeKind::ModeOnly,
            fathom_ingest::diff::PathChangeKind::Modified
            | fathom_ingest::diff::PathChangeKind::TypeChanged => ChangeKind::Modified,
        };
        let read = |entry: Option<fathom_ingest::diff::EntryRef>| {
            let entry = entry?;
            match repo.blob(entry.oid, options.max_blob_bytes as usize).ok()? {
                BlobRead::Present { bytes, .. } => Some(bytes),
                BlobRead::Oversized { .. } | BlobRead::WrongKind { .. } => Some(Vec::new()),
            }
        };
        blobs.push((path, read(change.old), read(change.new), kind));
    }
    let inputs = blobs.iter().map(|(path, before, after, kind)| Change {
        path,
        before: before.as_deref(),
        after: after.as_deref(),
        kind: Some(*kind),
    });
    Ok(delta(inputs, options))
}
