//! The declared event kinds (`e6.7`, `reco-raw`).
//!
//! The list lives in its own file because it *is* the contract: [`crate::events::Kind::is_declared`] checks
//! against it, the documentation table in `docs/concepts/extensibility.md` is compared against it by
//! `tests/test_extensibility_contract.py`, and a kind added here and not documented fails CI. A reviewer
//! asking "what may I subscribe to?" should open one file and see the whole vocabulary.

/// A verb started doing work.
pub const RUN_STARTED: &str = "run.started";
/// A verb finished, with its exit code in `counts.code`.
pub const RUN_FINISHED: &str = "run.finished";
/// A verb gave up, with the reason in `note`.
pub const RUN_FAILED: &str = "run.failed";
/// An index pass began.
pub const INDEX_STARTED: &str = "index.started";
/// An index pass ended: `counts.episodes`, `counts.errors`.
pub const INDEX_FINISHED: &str = "index.finished";
/// One episode's analysis began.
pub const ANALYSIS_STARTED: &str = "analysis.started";
/// One episode's analysis ended: `counts.files`, `counts.findings`, `counts.gaps`.
pub const ANALYSIS_FINISHED: &str = "analysis.finished";
/// A detector answered: `detector`, `outcome`, `counts.findings`.
pub const DETECTOR_FINISHED: &str = "detector.finished";
/// A detector did not answer, and said why: `outcome` is `disabled`, `not_examined` or `suppressed`.
pub const DETECTOR_SKIPPED: &str = "detector.skipped";
/// Which detectors the run was asked for, after the file and the flags: `note` holds the selection.
pub const SELECTION_RESOLVED: &str = "selection.resolved";
/// A finding arrived: `subject`, `severity`, and the namespaced type in `detector`.
pub const FINDING_RAISED: &str = "finding.raised";
/// A plugin detector was registered (or re-registered) in this process.
pub const PLUGIN_REGISTERED: &str = "plugin.registered";
/// A plugin detector was refused, or one of its findings was: `note` says which rule.
pub const PLUGIN_REJECTED: &str = "plugin.rejected";
/// Something the run has to admit and no other kind carries.
pub const NOTICE: &str = "notice.raised";

/// Every declared kind, in family order. The list is what [`Kind::is_declared`] checks and what the
/// contract test compares against the documentation, so a kind added here and not documented fails CI.
pub const ALL: &[&str] = &[
    RUN_STARTED,
    RUN_FINISHED,
    RUN_FAILED,
    INDEX_STARTED,
    INDEX_FINISHED,
    ANALYSIS_STARTED,
    ANALYSIS_FINISHED,
    DETECTOR_FINISHED,
    DETECTOR_SKIPPED,
    SELECTION_RESOLVED,
    FINDING_RAISED,
    PLUGIN_REGISTERED,
    PLUGIN_REJECTED,
    NOTICE,
];
