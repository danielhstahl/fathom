//! `normalize.rs` — one commit in, one [`CommitRecord`] out (e1.2 / reco-76g).
//!
//! This is the pipeline the ticket names: read a commit, diff its tree against its base, and turn every
//! changed path into a [`FileRecord`] with a change kind, both modes, a binary verdict, hunk headers and
//! line counts — under size caps, and with the gaps named instead of filled in.
//!
//! # Where the diff comes from
//!
//! The tree diff is `gix`'s ([`gix::Repository::diff_tree_to_tree`]) with the options written out here
//! rather than read from the repository's configuration, for the same reason user config is otherwise
//! isolated ([`crate::repo::DiscoverOptions::use_user_config`]): a record whose shape depends on the
//! machine reading it cannot be a golden file, and `diff.renames` in somebody's global config changing
//! the rows this writes is exactly the second source of truth ADR-001 §7.4 exists to refuse.
//!
//! Line counts and hunks are computed here from the stored blob bytes using the same diff engine `gix`
//! uses (`gix::diff::blob`: imara-diff with git's slider heuristics, `histogram` — the algorithm git
//! itself defaults to). Two consequences are deliberate:
//!
//! * **No program runs.** `.gitattributes` `clean`/`smudge`/`textconv` drivers are the one way a
//!   repository can make a reader execute a program the repository named, and fathom does not do that
//!   (ADR-001 §8.2, and `tests/no_git_subprocess.rs` asserts it). Content facts therefore describe the
//!   *stored* bytes — which is also the only answer that is identical on every machine.
//! * **Binary-ness is git's own rule**: a NUL byte in the first 8 KiB of either side, the same test git's
//!   `buffer_is_binary()` applies before consulting attributes.
//!
//! # What a merge is
//!
//! A merge contributes no per-file rows by default ([`MergePolicy::Skip`]), which is what
//! `git log --raw` does and what keeps a branch's forty changes from being counted twice — once in their
//! own commits, once again in the merge that carried them. [`MergePolicy::FirstParent`] is the other
//! answer, and a record always says which one produced it ([`CommitRecord::diff_base`]) so the choice is
//! never invisible.
//!
//! # Cost, and the cap that bounds it
//!
//! Per file, at most two blobs enter memory, each at most [`Limits::blob_memory_cap`] (8 MiB by default,
//! from `benchmarks/budgets.json`'s `ingest.file_size_cap.mib`), and both are dropped before the next
//! file: memory is bounded per *file*, not per commit, which is the difference between "a large commit
//! is slow" and "a large commit is fatal". Blob sizes come from object headers, so deciding *not* to read
//! a 500 MB blob costs a header read and no inflation ([`crate::repo::Repository::object_header`]).
//!
//! Normalizing one commit touches no shared state, which is what lets [`crate::ingest`] run it on several
//! threads with one repository handle per worker ([`crate::repo::Repository`]'s clone is an `Arc` clone).

use crate::diff::{Measurement, measure_blobs};
use crate::error::Error;
use crate::graph::CommitHeader;
use crate::oid::Oid;
use crate::paths::RepoPath;
use crate::record::{
    ChangeKind, CommitRecord, Content, DiffBase, FileRecord, Limits, MessageFacts, NotExamined,
    RenameCandidate, Taxonomy, Totals, Trailers,
};
use crate::repo::Repository;

// The per-file measurement moved to [`crate::diff`] in `e1.4`, so the analysis layer can ask for one
// file's measurement without normalizing its commit; both names stay published here, where `e1.2`
// shipped them, so no call site had to change.
pub use crate::diff::is_binary;

/// How a merge's per-file records are derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergePolicy {
    /// No file rows: a merge's `files` is empty and its totals are zero. The default, and git's.
    #[default]
    Skip,
    /// Diff against the first parent. The changes a merge *brought in* become visible, at the cost of
    /// counting them a second time next to the commits that introduced them.
    FirstParent,
}

/// Everything the normalizer needs beyond the repository, in one value.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The size caps ([`Limits`]).
    pub limits: Limits,
    /// How merges are handled.
    pub merge: MergePolicy,
}

// Derived, so a field added to `Options` cannot be silently left out of the default the way a hand-written
// `Default` impl can: the default is "every cap at its documented value, skip merges".

/// One commit's worth of normalization, with the handles it needs.
///
/// `Send`, and cheap to make: an [`crate::ingest`] worker constructs one per batch with its own
/// repository handle.
pub struct Normalizer<'r> {
    repo: &'r Repository,
    options: Options,
    taxonomy: &'r dyn Taxonomy,
}

impl<'r> Normalizer<'r> {
    /// Normalize against `repo`, asking `taxonomy` what each path is.
    pub fn new(repo: &'r Repository, taxonomy: &'r dyn Taxonomy, options: Options) -> Self {
        Normalizer {
            repo,
            options,
            taxonomy,
        }
    }

    /// The options in force.
    #[must_use]
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// The taxonomy table version in force.
    #[must_use]
    pub fn taxonomy_version(&self) -> u32 {
        self.taxonomy.version()
    }

    /// Normalize one commit, reading its object.
    ///
    /// # Errors
    ///
    /// [`Error::NotACommit`] if `oid` names something that is not a commit; the usual [`Error`] variants
    /// for a repository that cannot be read. A *missing* parent or a missing blob is not an error: the
    /// record states what could not be examined and why.
    pub fn record(&self, oid: Oid) -> Result<CommitRecord, Error> {
        let header = self.repo.commit(oid)?.ok_or_else(|| Error::NotACommit {
            oid,
            detail: String::from("the object is not a commit"),
        })?;
        self.record_from_header(&header)
    }

    /// Normalize a commit header the caller already decoded, which is what the walk in [`crate::ingest`]
    /// uses so a commit object is decoded once and not twice.
    pub fn record_from_header(&self, header: &CommitHeader) -> Result<CommitRecord, Error> {
        let message = MessageFacts::new(&header.message, self.options.limits.message_head_bytes);
        let trailers = Trailers::parse(&header.message);
        let (base_tree, diff_base) = self.diff_base(header)?;
        let mut files: Vec<FileRecord> = Vec::new();
        let mut files_truncated = false;
        if let Some(base_tree) = base_tree {
            let changes = self.tree_changes(base_tree, header.tree)?;
            let cap = self.options.limits.files_per_commit;
            files.reserve(changes.len().min(cap));
            for change in changes {
                if files.len() >= cap {
                    files_truncated = true;
                    break;
                }
                files.push(self.file_record(change)?);
            }
        }
        let totals = total_of(&files);
        Ok(CommitRecord {
            oid: header.oid,
            tree: header.tree,
            parents: header.parents.clone(),
            author: header.author.clone(),
            committer: header.committer.clone(),
            message,
            trailers,
            files,
            totals,
            files_truncated,
            diff_base,
        })
    }

    /// The tree to diff against, and the [`DiffBase`] that says so; `None` tree means "no file rows, and
    /// here is the stated reason".
    fn diff_base(&self, header: &CommitHeader) -> Result<(Option<Oid>, DiffBase), Error> {
        match header.parents.as_slice() {
            [] => Ok((
                Some(crate::repo::from_gix(
                    self.repo.object_hash_kind().empty_tree().as_ref(),
                )),
                DiffBase::EmptyTree,
            )),
            [parent] => match self.parent_tree(*parent)? {
                Some(tree) => Ok((Some(tree), DiffBase::Parent { parent: *parent })),
                // The parent is named by the commit and its object is not in this clone. Diffing against
                // the empty tree instead would report the entire tree as newly added, which is a worse
                // lie than an honest empty file list.
                None => Ok((None, DiffBase::UnreadableParent { parent: *parent })),
            },
            [first, ..] => match self.options.merge {
                MergePolicy::Skip => Ok((
                    None,
                    DiffBase::MergeSkipped {
                        parents: header.parents.len(),
                    },
                )),
                MergePolicy::FirstParent => match self.parent_tree(*first)? {
                    Some(tree) => Ok((
                        Some(tree),
                        DiffBase::MergeFirstParent {
                            parent: *first,
                            parents: header.parents.len(),
                        },
                    )),
                    None => Ok((None, DiffBase::UnreadableParent { parent: *first })),
                },
            },
        }
    }

    fn parent_tree(&self, parent: Oid) -> Result<Option<Oid>, Error> {
        // A parent that is not in this clone (the far side of a shallow graft) is not an error here: the
        // commit still gets a record, with a stated reason for having no file rows.
        Ok(self.repo.commit(parent)?.map(|parent| parent.tree))
    }

    /// The tree diff, with the options this crate chooses rather than the repository's.
    fn tree_changes(&self, old_tree: Oid, new_tree: Oid) -> Result<Vec<Change>, Error> {
        let gix_repo = self.repo.gix();
        let old = self.tree(old_tree)?;
        let new = self.tree(new_tree)?;
        let mut options = gix::diff::Options::default();
        options.track_path();
        options.track_rewrites(Some(gix::diff::Rewrites {
            // git's `-M<n>%`, and `-l<n>` for the pair limit.
            percentage: Some(f32::from(self.options.limits.rename_threshold) / 100.0),
            // Copy detection (git's `-C`) is deliberately not offered yet. Where a rename replaces a
            // deletion with one row, `gix`'s copy tracking *consumes* the source's own modification row,
            // so enabling `-C` here would silently drop a changed file that `git log --raw -C` still prints
            // — a row missing without saying it is missing, which is the one thing a spine row may not do.
            // Similarity pairing for renames is faithful to git's `-M` and is what `e1.2` needs; copies are
            // a `e2.x` question with a fix to make first.
            copies: None,
            limit: self.options.limits.rename_pair_limit,
            // An empty file is not a unique enough identity to pair on; git agrees by default.
            track_empty: false,
        }));
        let detached = gix_repo
            .diff_tree_to_tree(Some(&old), Some(&new), options)
            .map_err(|err| self.repo.unreadable("diffing two trees", &err))?;
        // gix's traversal reports a changed *directory* as a change as well as everything inside it;
        // `git log --raw` never prints a directory row, and one here would double-count every change it
        // contains. Blob, symlink and gitlink entries are the changes there is anything to say about.
        let mut changes: Vec<Change> = detached
            .iter()
            .map(|change| Change::from(change.to_ref()))
            .filter(|change| !change.is_directory())
            .collect();
        changes.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(changes)
    }

    fn tree(&self, oid: Oid) -> Result<gix::Tree<'_>, Error> {
        let id = crate::repo::to_gix(oid, self.repo.object_hash_kind());
        let object = self
            .repo
            .gix()
            .try_find_object(id)
            .map_err(|err| self.repo.unreadable("reading a tree", &err))?
            .ok_or_else(|| Error::ObjectUnavailable {
                oid,
                why: self.repo.why_unavailable(oid),
            })?;
        object
            .try_into_tree()
            .map_err(|err| self.repo.unreadable("decoding a tree", &err))
    }

    fn file_record(&self, change: Change) -> Result<FileRecord, Error> {
        let limits = self.options.limits;
        let path = RepoPath::new(change.path.clone());
        let measured = self.measure(&change, &limits)?;
        Ok(FileRecord {
            change: change.kind,
            // The taxonomy is decided on the path the row is keyed by, which after a rename is the new
            // path: "what is this file" is a question about the file that exists now.
            taxonomy: self.taxonomy.classify(&path),
            path,
            old_path: change
                .source_path
                .as_ref()
                .map(|bytes| RepoPath::new(bytes.to_vec())),
            mode_old: change.mode_old,
            mode_new: change.mode_new,
            blob_old: change.old_blob,
            blob_new: change.new_blob,
            content: measured.content,
            added: measured.added,
            removed: measured.removed,
            hunks: measured.hunks,
            hunk_total: measured.hunk_total,
            rename: change.rename.clone(),
            size_old: self.blob_size(change.old_blob)?,
            size_new: self.blob_size(change.new_blob)?,
        })
    }

    /// A blob's size from its object header — no inflation, which is the whole point of asking the header
    /// instead of the object.
    fn blob_size(&self, oid: Option<Oid>) -> Result<Option<u64>, Error> {
        match oid {
            Some(oid) => Ok(self.repo.object_header(oid)?.map(|header| header.size)),
            None => Ok(None),
        }
    }

    /// Read both sides under the cap, decide binary-ness, and diff if there is anything to diff.
    ///
    /// The measuring itself is [`crate::diff::measure_blobs`], shared with the analysis layer so a cached
    /// per-file analysis and a spine row are the same computation.
    fn measure(&self, change: &Change, limits: &Limits) -> Result<Measurement, Error> {
        // A gitlink (a submodule pointer) has no blob to read: the change is a pointer that moved, and
        // the mode plus the two commit ids say everything there is to say about it. Checked before the
        // read because in a repository whose submodules were never cloned the pointed-at commit is not
        // in this object database, and "not a blob" is the answer rather than "object unavailable".
        if change.gitlink {
            return Ok(Measurement::unmeasured(Content::NotExamined {
                reason: NotExamined::NotABlob {
                    found: "commit".to_owned(),
                },
            }));
        }
        measure_blobs(self.repo, change.old_blob, change.new_blob, limits)
    }
}

/// Sum a file list into the totals a surface reads without recomputing anything.
fn total_of(files: &[FileRecord]) -> Totals {
    let mut totals = Totals {
        files: files.len(),
        ..Totals::default()
    };
    for file in files {
        totals.added += u64::from(file.added);
        totals.removed += u64::from(file.removed);
        totals.hunks += file.hunk_total;
        if file.content.is_binary() {
            totals.binary += 1;
        }
        if matches!(file.content, Content::NotExamined { .. }) {
            totals.unexamined += 1;
        }
        if file.rename.is_some() {
            totals.renames += 1;
        }
    }
    totals
}

/// The change shape this module needs, flattened out of `gix`'s four variants.
///
/// Collapsing them here is what keeps [`Normalizer::file_record`] from matching on `gix` in three places,
/// and it is the boundary where a `gix` type stops being a `gix` type (ADR-002 §4.2).
struct Change {
    kind: ChangeKind,
    /// The path after the change (or before it, for a deletion).
    path: Vec<u8>,
    /// The rename/copy source, when the pairing said one.
    source_path: Option<Vec<u8>>,
    mode_old: Option<u32>,
    mode_new: Option<u32>,
    old_blob: Option<Oid>,
    new_blob: Option<Oid>,
    rename: Option<RenameCandidate>,
    /// The entry is a gitlink rather than a blob.
    gitlink: bool,
}

impl Change {
    /// The entry is a subtree, which is a location rather than a change to a file.
    fn is_directory(&self) -> bool {
        let mode = self.mode_new.or(self.mode_old).unwrap_or(0);
        mode & 0o170_000 == 0o040_000
    }
}

impl<'a> From<gix::diff::tree_with_rewrites::ChangeRef<'a>> for Change {
    fn from(change: gix::diff::tree_with_rewrites::ChangeRef<'a>) -> Self {
        use gix::diff::tree_with_rewrites::ChangeRef as C;
        match change {
            C::Addition {
                location,
                entry_mode,
                id,
                ..
            } => Change {
                kind: ChangeKind::Add,
                path: location.to_vec(),
                source_path: None,
                mode_old: None,
                mode_new: Some(u32::from(entry_mode.value())),
                old_blob: None,
                new_blob: Some(crate::repo::from_gix(id.as_ref())),
                rename: None,
                gitlink: is_gitlink(u32::from(entry_mode.value())),
            },
            C::Deletion {
                location,
                entry_mode,
                id,
                ..
            } => Change {
                kind: ChangeKind::Delete,
                path: location.to_vec(),
                source_path: None,
                mode_old: Some(u32::from(entry_mode.value())),
                mode_new: None,
                old_blob: Some(crate::repo::from_gix(id.as_ref())),
                new_blob: None,
                rename: None,
                gitlink: is_gitlink(u32::from(entry_mode.value())),
            },
            C::Modification {
                location,
                previous_entry_mode,
                previous_id,
                entry_mode,
                id,
            } => Change {
                kind: ChangeKind::Modify,
                path: location.to_vec(),
                source_path: None,
                mode_old: Some(u32::from(previous_entry_mode.value())),
                mode_new: Some(u32::from(entry_mode.value())),
                old_blob: Some(crate::repo::from_gix(previous_id.as_ref())),
                new_blob: Some(crate::repo::from_gix(id.as_ref())),
                rename: None,
                gitlink: is_gitlink(u32::from(entry_mode.value())),
            },
            C::Rewrite {
                source_location,
                source_entry_mode,
                source_id,
                diff,
                entry_mode,
                location,
                id,
                copy,
                ..
            } => {
                let source = crate::repo::from_gix(source_id.as_ref());
                let target = crate::repo::from_gix(id.as_ref());
                Change {
                    kind: if copy {
                        ChangeKind::Copy
                    } else {
                        ChangeKind::Rename
                    },
                    path: location.to_vec(),
                    source_path: Some(source_location.to_vec()),
                    mode_old: Some(u32::from(source_entry_mode.value())),
                    mode_new: Some(u32::from(entry_mode.value())),
                    old_blob: Some(source),
                    new_blob: Some(target),
                    rename: Some(RenameCandidate {
                        source: RepoPath::new(source_location.to_vec()),
                        source_blob: Some(source),
                        score: rename_score(diff.as_ref(), source == target),
                        copy,
                    }),
                    gitlink: is_gitlink(u32::from(entry_mode.value())),
                }
            }
        }
    }
}

/// A diffcore-style similarity score, derived from the line statistics the pairing diff produced.
///
/// `gix` leaves [`gix::diff::blob::DiffLineStats::similarity`] at `0.0` — the field is declared and
/// never filled — so this computes the number from the same statistics with diffcore's formula,
/// `100 − 100·(added+removed)/max(before, after)`. It is labelled as ours in
/// [`crate::record::RenameCandidate`] rather than passed off as git's. Identical content needs no
/// statistics at all and scores a perfect `100`.
fn rename_score(stats: Option<&gix::diff::blob::DiffLineStats>, identical: bool) -> Option<u8> {
    if identical {
        // The pairing needs no statistics to know the two blobs are the same object: that is a similarity
        // of 100 by definition, and reporting anything less would be arithmetic pretending to be a measure.
        return Some(100);
    }
    similarity_percent(stats?)
}

fn similarity_percent(stats: &gix::diff::blob::DiffLineStats) -> Option<u8> {
    let denominator = u64::try_from(stats.before.max(stats.after).max(1)).unwrap_or(u64::MAX);
    let changed = u64::from(stats.removals) + u64::from(stats.insertions);
    let penalty = (changed * 100) / denominator;
    Some(100_u8.saturating_sub(u8::try_from(penalty.min(100)).unwrap_or(100)))
}

/// `0o160000` — a gitlink, i.e. a submodule entry.
#[must_use]
fn is_gitlink(mode: u32) -> bool {
    mode & 0o170000 == 0o160000
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn binary_rule_is_the_first_eight_kibibytes() {
        assert!(is_binary(b"abc\0def"));
        assert!(!is_binary(b"abc\ndef"));
        // Past the window, git calls it text however many NULs there are: the rule is a prefix test.
        let mut late = vec![b'a'; 9000];
        late.push(0);
        assert!(!is_binary(&late));
    }

    #[test]
    fn gitlink_modes_are_recognised_and_nothing_else_is() {
        assert!(is_gitlink(0o160000));
        assert!(!is_gitlink(0o100644));
        assert!(!is_gitlink(0o120000));
        assert!(!is_gitlink(0o040000));
    }

    #[test]
    fn similarity_score_is_diffcore_s_formula() {
        let stats = gix::diff::blob::DiffLineStats {
            removals: 0,
            insertions: 0,
            before: 100,
            after: 100,
            similarity: 0.0,
        };
        assert_eq!(similarity_percent(&stats), Some(100));
        let half = gix::diff::blob::DiffLineStats {
            removals: 50,
            insertions: 0,
            before: 100,
            after: 50,
            similarity: 0.0,
        };
        assert_eq!(similarity_percent(&half), Some(50));
    }
}
