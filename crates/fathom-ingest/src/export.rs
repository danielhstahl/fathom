//! `export.rs` — the canonical JSON form of the normalized records (e1.2 / reco-76g).
//!
//! This is the file the acceptance criterion is about: a 500-commit fixture ingests, exports, and the
//! bytes match the committed golden byte for byte. Two things make that claim worth having rather than
//! merely green, and they are the whole design of this module.
//!
//! # Canonical, in the two senses that matter
//!
//! **Field order** is fixed by the structs in [`crate::record`]: serde writes fields in declaration order,
//! so there is no map-iteration randomness to fight. **Record order** is *not* walk order — records are
//! sorted by commit id ascending ([`sort_commits`]). Walk order depends on commit timestamps and on which
//! ref was the starting point, and a golden file whose diff is a timestamp jitter is a golden file that
//! gets regenerated the first time it is annoying. Sorting by id makes the file a pure function of the
//! set of commits.
//!
//! **Nothing nondeterministic is in it.** No absolute path, no repository name, no wall-clock timestamp,
//! no fathom version. ADR-004 §4.2 makes the same argument about `drift_measured_utc` not belonging in
//! `code_quality`: a fact that changes between two runs of the same input is provenance, not content, and
//! provenance belongs in `meta`/`index_run` — which are *not* exported here. That is a deliberate omission
//! from the round trip, and `tests/export_round_trip.rs` names it.
//!
//! # The header is the recipe
//!
//! [`Normalization`] records the knobs that change the bytes — the size caps, the merge policy, whether
//! copies were detected, the rename threshold, the taxonomy version. Without it a golden file is a
//! screenshot; with it, a byte mismatch is a diagnosis: the file says which rules produced it, so a change
//! in a limit and a change in the repository are distinguishable without rerunning anything.
//!
//! # Round trip
//!
//! [`from_str`] reads a file back into records, and re-exporting those records must produce the identical
//! bytes. That is the assertion that the export is the record and not a rendering of part of it: any field
//! that serde skipped, flattened or lossily converted shows up as a diff. The store is a *projection* of
//! these records (ADR-004 Appendix A keeps less than the record holds — `blob_old`, hunk headers, a
//! rename's score, an unexamined file's reason all live here and not in SQLite), and
//! `tests/store_projection.rs` checks the rows against this file rather than pretending they are the same
//! thing.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::normalize::Options;
use crate::record::{CommitRecord, Limits};

/// The format discriminator, written first so a reader can reject the wrong file before parsing it.
pub const FORMAT: &str = "fathom-commits";

/// The format version. Bumped when the record shape changes incompatibly — and only then, because a golden
/// file is a contract with the future.
pub const VERSION: u32 = 1;

/// The knobs that decide what the bytes mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Normalization {
    /// The size caps in force ([`Limits`]).
    pub limits: Limits,
    /// `skip` or `first-parent`, as [`crate::normalize::MergePolicy`]'s spelling.
    pub merge_policy: String,
    /// The taxonomy table version stamped on every file row.
    pub taxonomy_version: u32,
}

impl Normalization {
    /// Describe a run's options, for the header.
    #[must_use]
    pub fn of(options: &Options, taxonomy_version: u32) -> Self {
        Normalization {
            limits: options.limits,
            merge_policy: match options.merge {
                crate::normalize::MergePolicy::Skip => String::from("skip"),
                crate::normalize::MergePolicy::FirstParent => String::from("first-parent"),
            },
            taxonomy_version,
        }
    }
}

/// The counts, so a reader can sanity-check a file without parsing the array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Counts {
    /// Commits in the file.
    pub commits: usize,
    /// File rows across them.
    pub files: usize,
    /// Lines added.
    pub added: u64,
    /// Lines removed.
    pub removed: u64,
    /// Commits with at least one unexamined file.
    pub commits_with_unexamined: usize,
}

impl Counts {
    /// Sum a record list.
    #[must_use]
    pub fn of(records: &[CommitRecord]) -> Self {
        let mut counts = Counts {
            commits: records.len(),
            files: 0,
            added: 0,
            removed: 0,
            commits_with_unexamined: 0,
        };
        for record in records {
            counts.files += record.files.len();
            counts.added += record.totals.added;
            counts.removed += record.totals.removed;
            if record.totals.unexamined > 0 {
                counts.commits_with_unexamined += 1;
            }
        }
        counts
    }
}

/// An export file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Export {
    /// Must be [`FORMAT`]; checked by [`from_str`] before anything else.
    pub format: String,
    /// Must be [`VERSION`].
    pub version: u32,
    /// The recipe that produced these records.
    pub normalization: Normalization,
    /// The counts.
    pub counts: Counts,
    /// The records, in the order [`sort_commits`] produces.
    pub commits: Vec<CommitRecord>,
}

impl Export {
    /// Build the file for `records` under `normalization`.
    #[must_use]
    pub fn new(mut records: Vec<CommitRecord>, normalization: Normalization) -> Self {
        sort_commits(&mut records);
        Export {
            format: FORMAT.to_owned(),
            version: VERSION,
            normalization,
            counts: Counts::of(&records),
            commits: records,
        }
    }

    /// The records, back out of the file: the identity of the round trip, and what a caller that wants to
    /// re-emit or re-project does not have to know about.
    #[must_use]
    pub fn into_records(self) -> Vec<CommitRecord> {
        self.commits
    }

    /// Serialize canonically: pretty-printed, two-space indent, terminated by a newline (so the file
    /// `diff`s cleanly and so `cat` of the last line does not join with the shell prompt).
    ///
    /// # Errors
    ///
    /// [`Error::Export`] if serialization fails, which for these types means one thing only: a path whose
    /// bytes are not valid UTF-8. git allows such paths, `serde_json` cannot write them, and
    /// `tests/normalize_fixture.rs` pins what fathom does with one.
    pub fn to_json(&self) -> Result<String, Error> {
        let mut text = serde_json::to_string_pretty(self).map_err(|err| Error::Export {
            detail: err.to_string(),
        })?;
        text.push('\n');
        Ok(text)
    }
}

/// An export file read back from disk. See [`from_str`].
pub fn to_json(records: &[CommitRecord], normalization: &Normalization) -> Result<String, Error> {
    Export::new(records.to_vec(), normalization.clone()).to_json()
}

/// Parse an export file.
///
/// # Errors
///
/// [`Error::Export`] if the text is not JSON in this shape, or if it names a format or version this build
/// does not understand. A wrong `format` is refused rather than parsed leniently: reading a
/// `fathom-heatmap` file as a commit list would produce a *plausible* wrong answer, which is the class of
/// failure ADR-004 §4.3's magic-byte check exists for.
pub fn from_str(text: &str) -> Result<Export, Error> {
    let export: Export = serde_json::from_str(text).map_err(|err| Error::Export {
        detail: err.to_string(),
    })?;
    if export.format != FORMAT {
        return Err(Error::Export {
            detail: format!("expected format {FORMAT:?}, found {:?}", export.format),
        });
    }
    if export.version != VERSION {
        return Err(Error::Export {
            detail: format!("expected version {VERSION}, found {}", export.version),
        });
    }
    Ok(export)
}

/// Put records in export order: commit id ascending, as the lowercase hex string sorts.
///
/// Sorted by the *hex* rather than the raw bytes so the order is the order a human sees when they grep for
/// a prefix — the property that makes a prefix search and a diff agree.
pub fn sort_commits(records: &mut [CommitRecord]) {
    records.sort_by_key(|record| record.oid.hex());
}

/// The digest recorded alongside a golden file, and printed by `fathom export --json`.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    crate::record::sha256_hex(bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::graph::Signature;
    use crate::oid::Oid;
    use crate::paths::RepoPath;
    use crate::record::{
        ChangeKind, DiffBase, FileRecord, MessageFacts, NotExamined, Taxonomy, Totals, Trailers,
        Unclassified,
    };
    use fathom_core::time::Timestamp;

    fn oid(hex: &str) -> Oid {
        hex.parse::<Oid>().unwrap()
    }

    fn signature() -> Signature {
        Signature {
            name: String::from("Ada Lovelace"),
            email: String::from("ada@example.invalid"),
            when: Timestamp::from_unix_micros(1_700_000_000_000_000),
            offset_minutes: 60,
        }
    }

    fn record(id: &str, files: usize) -> CommitRecord {
        let files: Vec<FileRecord> = (0..files)
            .map(|index| {
                FileRecord::unexamined(
                    ChangeKind::Add,
                    RepoPath::new(format!("dir/file-{index}.rs")),
                    NotExamined::Oversized { size: 1 },
                )
            })
            .collect();
        // Totals derived from the files that were actually built, so that a test about counts is a test about
        // counting and not about a hand-written literal that can drift from the rows beside it.
        let totals = Totals {
            files: files.len(),
            unexamined: files.iter().filter(|file| !file.content.measured()).count(),
            ..Totals::default()
        };
        CommitRecord {
            oid: oid(id),
            tree: Oid::ZERO,
            parents: Vec::new(),
            author: signature(),
            committer: signature(),
            message: MessageFacts::new(b"thing\n", 512),
            trailers: Trailers::parse(b"thing\n"),
            files,
            totals,
            files_truncated: false,
            diff_base: DiffBase::EmptyTree,
        }
    }

    #[test]
    fn commits_sort_by_hex_id_not_by_arrival() {
        let mut records = vec![
            record("ffff000000000000000000000000000000000000", 0),
            record("0000ffffffffffffffffffffffffffffffffffff", 0),
        ];
        sort_commits(&mut records);
        assert!(records[0].oid.hex().starts_with('0'));
        assert!(records[1].oid.hex().starts_with('f'));
    }

    #[test]
    fn the_header_and_counts_describe_the_body() {
        let records = vec![record("1111000000000000000000000000000000000000", 3)];
        let export = Export::new(records, Normalization::of(&Options::default(), 0));
        assert_eq!(export.counts.commits, 1);
        assert_eq!(export.counts.files, 3);
        assert_eq!(export.counts.commits_with_unexamined, 1);
        assert_eq!(export.version, VERSION);
        assert_eq!(export.normalization.taxonomy_version, 0);
    }

    #[test]
    fn counts_only_notice_unexamined_commits_that_have_files() {
        let mut record = record("2222000000000000000000000000000000000000", 2);
        record.totals = Totals {
            files: 2,
            added: 7,
            removed: 3,
            unexamined: 0,
            ..Totals::default()
        };
        let counts = Counts::of(&[record]);
        assert_eq!(counts.added, 7);
        assert_eq!(
            counts.commits_with_unexamined, 0,
            "the totals, not the file list, decide"
        );
    }

    #[test]
    fn a_foreign_format_is_refused_not_guessed_at() {
        let export = Export::new(Vec::new(), Normalization::of(&Options::default(), 0));
        let text = export
            .to_json()
            .unwrap()
            .replace("fathom-commits", "fathom-heatmap");
        let err = from_str(&text).unwrap_err();
        assert!(err.to_string().contains("expected format"));
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_dropped_silently() {
        let export = Export::new(Vec::new(), Normalization::of(&Options::default(), 0));
        let text = export
            .to_json()
            .unwrap()
            .replace("\"version\":", "\"version\": 1, \"sneaky\": 1, \"");
        assert!(from_str(&text).is_err());
    }

    #[test]
    fn the_header_is_the_first_thing_in_the_file() {
        let export = Export::new(Vec::new(), Normalization::of(&Options::default(), 0));
        let text = export.to_json().unwrap();
        assert!(text.starts_with("{\n  \"format\": \"fathom-commits\",\n  \"version\": 1"));
        assert!(text.ends_with("}\n"));
        let _ = Unclassified.version();
    }
}
