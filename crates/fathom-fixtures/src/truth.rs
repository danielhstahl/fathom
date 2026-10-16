//! `truth.rs` — what a scenario is *for*, written by a human.
//!
//! Every scenario ships a ground-truth JSON next to the code that builds it. It says three things:
//!
//! * **episodes** — which commits a reviewer would call one reviewable unit, and which boundary rule they
//!   would say opened it. This is what `e1.3` scores itself against, so the number in its acceptance
//!   criterion is a comparison against reality rather than against whatever the segmentation happened to
//!   produce on the day it was written.
//! * **findings** — what a detector is supposed to see (and, in `clean_baseline`, what it is supposed *not*
//!   to see). Each names the commit or episode it belongs to, so a detector that fires on the wrong episode
//!   fails rather than accidentally passing.
//! * **claims** — small, checkable statements about the fixture's own bytes: "this commit changed exactly
//!   these paths", "this one only moved whitespace", "this file really contains this line". They are
//!   re-checked on every run by [`crate::verify`], which is what stops a scenario from drifting away from
//!   the file that describes it — the failure mode where a golden corpus quietly stops meaning anything.
//!
//! There is a fourth section, `determinism`, and it is the only one no one writes by hand: `--bless` stamps
//! the object, work-tree and event digests and the ref tips, and refuses to touch the rest. That separation
//! is the point. Semantics are argued by a human; hashes are machine property, and a generator can bless its
//! own hashes without being able to bless its own expectations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::policy::GROUND_TRUTH_SCHEMA;

/// One scenario's ground truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundTruth {
    /// See [`GROUND_TRUTH_SCHEMA`].
    pub schema: String,
    /// The scenario this describes; must match the file's stem and the registry entry.
    pub scenario: String,
    /// One or two sentences on what the scenario exercises, written for whoever reads the failure.
    pub purpose: String,
    /// The plan keys that consume this scenario, e.g. `["e1.3", "e2.2"]`.
    #[serde(default)]
    pub consumers: Vec<String>,
    /// See above: the blessed, machine-owned part.
    #[serde(default)]
    pub determinism: Determinism,
    /// The expected episodes, in walk order.
    #[serde(default)]
    pub episodes: Vec<Episode>,
    /// Free-form notes for whoever reads a failure: what the scenario looks like from the outside.
    #[serde(default)]
    pub notes: Vec<String>,
    /// The expected findings.
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// Structural claims about the fixture's own bytes, re-checked every run.
    #[serde(default)]
    pub claims: Vec<Claim>,
}

impl GroundTruth {
    /// Parse a ground-truth document, with the errors named rather than panicked on.
    ///
    /// # Errors
    ///
    /// [`Error::Truth`] when the JSON does not parse or names another schema.
    pub fn parse(text: &str, file: &str) -> Result<Self> {
        let mut truth: GroundTruth = serde_json::from_str(text)
            .map_err(|source| Error::Truth(format!("{file}: {source}")))?;
        if truth.schema != GROUND_TRUTH_SCHEMA {
            return Err(Error::Truth(format!(
                "{file}: schema `{}` but this build reads `{GROUND_TRUTH_SCHEMA}`",
                truth.schema
            )));
        }
        truth.expand();
        Ok(truth)
    }
}

impl GroundTruth {
    /// Expand the compact `[first, length]` episode spelling into explicit commit indices, so every other
    /// consumer of `commits` sees one form.
    fn expand(&mut self) {
        for episode in &mut self.episodes {
            if episode.commits.is_empty()
                && let Some([first, length]) = episode.span
            {
                episode.commits = (first..first + length).collect();
            }
        }
    }
}

/// The blessed, machine-written section of a ground-truth file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Determinism {
    /// What wrote this, and why it is not hand-editable.
    #[serde(default)]
    pub note: Option<String>,
    /// Commits in the fixture.
    #[serde(default)]
    pub commit_count: Option<usize>,
    /// The tip of the default branch.
    #[serde(default)]
    pub head: Option<String>,
    /// Every ref in the repository, including the ones the default branch does not reach.
    #[serde(default)]
    pub tips: BTreeMap<String, String>,
    /// Digest over every object written.
    #[serde(default)]
    pub objects_digest: Option<String>,
    /// Digest over the checked-out work tree.
    #[serde(default)]
    pub worktree_digest: Option<String>,
    /// Digest over the harness event log, with the machine-dependent root normalised away.
    #[serde(default)]
    pub events_digest: Option<String>,
    /// The digest over all of the above, and the number the CI job prints.
    #[serde(default)]
    pub digest: Option<String>,
}

/// One expected episode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// A stable handle for this row, e.g. `ep-1`. Findings and other episodes refer to it by this name.
    pub id: String,
    /// The commit indices it covers, in creation order.
    #[serde(default)]
    pub commits: Vec<usize>,
    /// The compact spelling of the same thing, for the scenarios whose runs are all the same length:
    /// `[first, length]`. Expanded into [`Episode::commits`] on load, so nothing downstream has to know.
    #[serde(default)]
    pub span: Option<[usize; 2]>,
    /// The rule that should have opened it: one of `root`, `hook`, `trailer`, `branch`, `author`,
    /// `time-gap`, `message-shape`, `override`.
    pub rule: String,
    /// The lineage the episode belongs to, when a scenario keeps more than one history alive (the rebase
    /// scenario is the only one that does). Defaults to the fixture's default branch.
    #[serde(default)]
    pub lineage: Option<String>,
    /// The committer key of the first member, e.g. `pi-agent`.
    #[serde(default)]
    pub author: Option<String>,
    /// `agent`, `human` or `bot`, for the episodes where the attribution is the point.
    #[serde(default)]
    pub kind: Option<String>,
    /// The harness session, when one named it.
    #[serde(default)]
    pub session: Option<String>,
    /// The harness turn, when one named it.
    #[serde(default)]
    pub turn: Option<String>,
    /// Why this is one episode, in the file's own words.
    #[serde(default)]
    pub note: Option<String>,
}

/// One expected finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// The finding's kind, named the way the consuming ticket names it: `cycle_introduced`, `churn_loop`,
    /// `noise`, `rename_storm`, …
    pub kind: String,
    /// The detector expected to produce it, when the ticket has a named detector table.
    #[serde(default)]
    pub detector: Option<String>,
    /// The commit index it is expected at.
    #[serde(default)]
    pub at: Option<usize>,
    /// The episode it is expected to belong to, by [`Episode::id`].
    #[serde(default)]
    pub episode: Option<String>,
    /// The paths the finding must name.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Rename pairs a rename finding must report, as `[from, to]`.
    #[serde(default)]
    pub pairs: Vec<[String; 2]>,
    /// `fire` (the detector must report this here), `silent` (it must not report this kind anywhere in this
    /// scenario), `collapse`, `survive` — a label for the consumer, not something this crate interprets.
    #[serde(default = "default_expect")]
    pub expect: String,
    /// The claim in prose, so the failure message can quote it.
    pub detail: String,
}

fn default_expect() -> String {
    "fire".to_owned()
}

/// A structural claim about the fixture's own bytes.
///
/// Deliberately a closed set of small primitives and nothing more. Each one is answerable by comparing
/// bytes the generator already has, which is the line this crate does not cross: checking that the fixture
/// contains what the ground truth says it contains is not the same act as analysing the repository
/// (ADR-002 §4.4). Anything that would need an analysis belongs in a consumer's test, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Claim {
    /// This commit changed exactly these paths, and nothing else.
    PathsChanged {
        /// The commit index.
        commit: usize,
        /// The whole set.
        paths: Vec<String>,
    },
    /// This commit changed only whitespace on these paths: strip every space and tab from every line, drop
    /// the blank lines, and the two sides are byte-identical. That is the whole of what a reformat is, and
    /// the token-level claim `e2.2` makes is allowed to be stronger than this, never weaker.
    WhitespaceOnlyChange {
        /// The commit index.
        commit: usize,
        /// The paths that must have moved with nothing but whitespace changed. The commit may have done
        /// other things too; `Self::PathsChanged` is how a file says it did nothing else.
        paths: Vec<String>,
    },
    /// This commit renamed these paths, content unchanged.
    Rename {
        /// The commit index.
        commit: usize,
        /// `[from, to]` pairs.
        pairs: Vec<[String; 2]>,
    },
    /// The file at this commit contains this text.
    TextContains {
        /// The commit index.
        commit: usize,
        /// The path.
        path: String,
        /// The needle.
        needle: String,
    },
    /// The file at this commit does not contain this text.
    TextAbsent {
        /// The commit index.
        commit: usize,
        /// The path.
        path: String,
        /// The needle.
        needle: String,
    },
    /// The file at this commit is at least this many bytes.
    FileAtLeast {
        /// The commit index.
        commit: usize,
        /// The path.
        path: String,
        /// The floor.
        bytes: usize,
    },
    /// The file at this commit reads as binary, by git's own NUL test.
    BinaryAt {
        /// The commit index.
        commit: usize,
        /// The path.
        path: String,
    },
    /// A ref names the commit at this index.
    TipIs {
        /// The ref, e.g. `refs/heads/pre-rewrite`.
        r#ref: String,
        /// The commit index it must name.
        commit: usize,
    },
    /// A ref exists at all.
    RefExists {
        /// The ref.
        r#ref: String,
    },
    /// The commit at this index carries a gitlink at `path` pointing at a companion's commit.
    Gitlink {
        /// The commit index.
        commit: usize,
        /// The mount point.
        path: String,
        /// The companion whose history owns the target.
        companion: String,
        /// Which of the companion's commits the gitlink names.
        at: usize,
    },
    /// The repository is a shallow clone whose `.git/shallow` names these commits.
    IsShallow {
        /// The expected shallow tips, as commit indices into this fixture.
        tips: Vec<usize>,
    },
    /// The harness event log contains this many events. A cheap way to say "the stream covers the history"
    /// without asking a consumer to parse it.
    EventCount {
        /// The expected count.
        events: usize,
    },
}

/// What can go wrong reading a ground-truth file.
#[derive(Debug)]
pub enum Error {
    /// The file was missing, unreadable, or not valid JSON.
    Truth(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truth(what) => write!(f, "ground truth: {what}"),
        }
    }
}

/// The crate's ground-truth result alias.
pub type Result<T> = std::result::Result<T, Error>;

impl std::error::Error for Error {}
