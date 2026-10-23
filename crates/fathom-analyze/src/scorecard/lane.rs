//! The store seam: one revision's graph in, one scorecard out (`e3.9`, `reco-al1`).
//!
//! This is the same shape as [`crate::cycles::episode`] and [`crate::layering::episode`] — a lane in, a
//! report out, one materialisation — and it exists for the same reason those two do: the *order* and the
//! *sharing*. A caller that wants all five dimensions can pay for the graph once and read four answers off
//! it:
//!
//! ```text
//!   score(lane, tree)
//!       ├─ materialise(tree)                    one chain walk — the only graph built
//!       ├─ naming                               the dictionary, read once, names every endpoint
//!       ├─ layering::assessment_of(graph)       a linear pass over the rows already in hand
//!       ├─ cycles Tracker::build(graph)         the partition, from the same rows
//!       ├─ hotspots::table_from(graph, churn)   fan-in/out/instability from the same rows again
//! ├─ lane.stored_ops(tree)              this revision's own rows, for the export-grain API signal
//!       └─ Scorecard
//! ```
//!
//! A caller that ran each report's own top-level `episode(...)` function instead would get the same numbers
//! for three chain walks per revision, which is the difference between a trend that can cover 500 revisions
//! and one that cannot.
//!
//! # What is read from the store, and what is not
//!
//! The graph and its dictionary: yes, through [`Lane`]. The churn window: yes, once, because the hotspot
//! ranking's sixth metric is churn velocity and the alternative is to leave that metric unmeasured — the
//! caller hands the window over so a backfill can reuse one read rather than buy one per revision. The
//! `e3.1` structural records: **no**, because this store has no per-revision structural table at all — the
//! structural records live in the analysis cache, keyed by blob, and are not reachable as "every file of
//! revision R". So the hotspot ranking here runs on the four metrics the graph *can* supply plus churn, and
//! says so in its own notices, which arrive in the report as caveats on the dimension rather than as a
//! silently smaller score. That stated partial is [`Measurement::complete`] doing the job it exists for.
//!
//! # Who writes rows
//!
//! Not this module. It returns [`ScorecardRow`]s; the caller writes them after the lane is dropped, because
//! [`Lane`] holds the store mutably and a writer that fights its own reader for the connection is a
//! deadlock with extra steps.

use fathom_core::time::Timestamp;
use fathom_store::graph::ChainPartial;
use fathom_store::scorecard::ScorecardRow;

use crate::cycles::debt::Ratchet;
use crate::cycles::track::{Options, Tracker};
use crate::graph::lane::Lane;
use crate::graph::layers::Layers;
use crate::graph::{ChurnReport, Graph, Naming};
use crate::hotspots::policy::Policy as HotspotPolicy;
use crate::hotspots::{FileFacts, table_from};
use crate::layering::Locate;
use crate::layering::report::Report as LayerReport;
use crate::layering::rules::Rules;

use super::dimension::{ApiSignal, Measurement, acyclicity, boundaries, coupling, hotspots};
use super::policy::Policy;
use super::score::Scorecard;

/// Everything the five dimensions need that is not the graph: the rules, the debt ledger, the knobs and the
/// clock — all supplied by the composition root, so nothing here reads a file or a wall clock (ADR-002
/// §4.4, §4.5).
#[derive(Debug, Clone)]
pub struct Knobs {
    /// The scorecard's own knobs.
    pub policy: Policy,
    /// The declared architecture, for the boundaries dimension.
    pub rules: Rules,
    /// The committed allowances, shared by the boundaries and acyclicity dimensions so a baselined finding
    /// is forgiven in both, exactly once.
    pub ratchet: Ratchet,
    /// The hotspot ranking's knobs, for the hotspot-pressure dimension.
    pub hotspots: HotspotPolicy,
    /// The instant to judge expiries against.
    pub now: Timestamp,
    /// Anything the caller's own read could not do, folded into the report's notices.
    pub notices: Vec<String>,
}

impl Knobs {
    /// Knobs with no declared rules and no committed debt: everything the store can answer, with the two
    /// rule-based dimensions reporting that no rules exist.
    #[must_use]
    pub fn bare(policy: Policy, hotspots: HotspotPolicy, now: Timestamp) -> Self {
        let (rules, notices) = Rules::from_repo_config_text(&Layers::new([]), "");
        Self {
            policy,
            rules,
            ratchet: Ratchet::empty(),
            hotspots,
            now,
            notices,
        }
    }
}

/// One revision's answer, plus what it cost to get.
#[derive(Debug, Clone)]
pub struct Measured {
    /// The scorecard.
    pub scorecard: Scorecard,
    /// What the read could not do, gathered from the walk, the dimensions and the caller.
    pub notices: Vec<String>,
    /// How many chain-walk steps the materialisation took, so a backfill's cost is a printed number and not
    /// a shrug.
    pub steps: usize,
    /// Whether the revision was served from the lane's snapshot cache.
    pub from_cache: bool,
}

impl Measured {
    /// The row this revision earned, ready for [`fathom_store::scorecard::Store::scorecard_write`].
    #[must_use]
    pub fn row(&self, since: Option<String>) -> ScorecardRow {
        ScorecardRow {
            tree_oid: self.scorecard.revision.clone(),
            dims: self.scorecard.to_stored(),
            since,
            built_utc: self.scorecard.built_utc,
        }
    }
}

/// Score one revision, reading its churn window from the lane.
///
/// `previous` is the revision whose rows this one's API signal compares against — the revision the episode
/// started at. `None` is the cold case, and API hygiene reports itself unmeasured rather than scoring a
/// repository's first revision as a clean bill of API health.
///
/// # Errors
///
/// [`ChainPartial`] when the revision's chain reaches no anchor within `graph.query.max_chain_walk`. That is
/// the only failure this returns: every dimension that cannot be measured degrades to a stated absence
/// instead, because a scorecard that refuses to exist is a scorecard nobody reads.
pub fn score(
    lane: &Lane<'_>,
    tree: &str,
    previous: Option<&str>,
    knobs: &Knobs,
) -> Result<Measured, ChainPartial> {
    let churn = lane.churn_recent(knobs.policy.window).ok();
    score_with(lane, tree, previous, churn.as_ref(), knobs)
}

/// The same, with a churn window the caller already measured.
///
/// Split out because a backfill buys one window read per revision and would otherwise pay for `window`
/// stored-op reads per revision on top of the chain walk — the exact shape of cost that made `s3` insist
/// the lane, not the caller, decides where the money goes.
///
/// # Errors
///
/// [`ChainPartial`] as [`score`].
pub fn score_with(
    lane: &Lane<'_>,
    tree: &str,
    previous: Option<&str>,
    churn: Option<&ChurnReport>,
    knobs: &Knobs,
) -> Result<Measured, ChainPartial> {
    let (graph, walk) = lane.materialise(tree)?;
    let naming = lane.naming().map_err(|_| ChainPartial::AnchorLost {
        tree_oid: tree.to_string(),
        steps: 0,
    })?;
    let mut measurements = Vec::new();
    let mut notices = knobs.notices.clone();
    if walk.undecoded > 0 {
        notices.push(format!(
            "{} stored chunk line(s) did not decode, so this revision's graph may be smaller than the \
             revision's own",
            walk.undecoded
        ));
    }
    notices.push(walk.sentence());

    measurements.push(boundaries_of(&graph, &naming, tree, knobs));
    measurements.push(coupling(&graph, &naming, &knobs.policy));
    measurements.push(acyclicity_of(&graph, &naming, knobs));
    measurements.push(api_of(lane, tree, previous, &naming, &knobs.policy));
    measurements.push(hotspots_of(&graph, &naming, tree, churn, knobs, &notices));

    let mut scorecard = Scorecard::of(&measurements, &knobs.policy, tree, micros(knobs.now));
    // The walk's own cost is a fact about the answer, not a log line: a scorecard built from a graph that
    // arrived from an anchor eight deltas back is a weaker claim than one served from the cache.
    scorecard.notices.extend(notices.clone());
    Ok(Measured {
        scorecard,
        notices,
        steps: walk.steps,
        from_cache: walk.from_cache,
    })
}

/// Score every revision in `trees` that has no row yet, newest first.
///
/// One lane, one churn window read per revision, one row per revision. The caller writes the rows; this
/// only decides what each revision scored, and stops on the first chain that will not materialise rather
/// than scoring half a history and calling it a trend.
///
/// # Errors
///
/// [`ChainPartial`] from the first revision that cannot be materialised; the rows scored before it are
/// still returned, because losing them would lose the reason the run stopped.
pub fn backfill(
    lane: &Lane<'_>,
    trees: &[String],
    knobs: &Knobs,
) -> (Vec<ScorecardRow>, Vec<String>, usize) {
    let mut rows = Vec::new();
    let mut notices = Vec::new();
    let mut scored = 0_usize;
    for (index, tree) in trees.iter().enumerate() {
        let previous = trees.get(index + 1).map(String::as_str);
        match score(lane, tree, previous, knobs) {
            Ok(measured) => {
                scored += 1;
                rows.push(measured.row(None));
            }
            Err(partial) => {
                notices.push(format!(
                    "backfill stopped at {}: {}",
                    short(tree),
                    partial.sentence()
                ));
                break;
            }
        }
    }
    (rows, notices, scored)
}

/// The boundaries dimension, from a graph already in hand: no second materialisation.
fn boundaries_of(graph: &Graph, naming: &Naming, tree: &str, knobs: &Knobs) -> Measurement {
    let locate = Locate::empty();
    let assessment = crate::layering::episode::assessment_of(graph, naming, &knobs.rules, &locate);
    let report = LayerReport::build(
        &knobs.rules,
        assessment,
        None,
        &locate,
        &knobs.ratchet,
        knobs.now,
    );
    let mut measurement = boundaries(&report);
    // The snapshot case is what a scorecard is, so say which comparison a "no news" line does not include:
    // a scorecard is a state, and the *delta* is `fathom architecture`'s since-review block.
    measurement.basis.push_str(&format!(" at {}", short(tree)));
    measurement
}

/// The acyclicity dimension, from a graph already in hand.
fn acyclicity_of(graph: &Graph, naming: &Naming, knobs: &Knobs) -> Measurement {
    let tracker = Tracker::build(graph, naming, Options::default());
    let report = tracker.snapshot(&knobs.ratchet, knobs.now);
    acyclicity(&report)
}

/// The API dimension, at the grain the store can actually supply: this revision's own rows.
///
/// `previous` is required: without an earlier revision there is no "what did this episode do", and the
/// dimension says so rather than scoring an absence as a success.
fn api_of(
    lane: &Lane<'_>,
    tree: &str,
    previous: Option<&str>,
    naming: &Naming,
    policy: &Policy,
) -> Measurement {
    let Some(previous) = previous else {
        return ApiSignal::absent(
            "no earlier revision in the index to compare against, so nothing arrived and nothing left — \
             API hygiene has no episode to judge",
        )
        .measurement(policy);
    };
    let stored = match lane.stored_ops(tree) {
        Ok(stored) => stored,
        Err(why) => {
            return ApiSignal::absent(format!(
                "this revision's stored graph rows could not be read ({}), so nothing can be said about \
                 what arrived or left",
                why.sentence()
            ))
            .measurement(policy);
        }
    };
    let added: Vec<_> = stored
        .ops
        .iter()
        .filter(|tagged| tagged.op.add)
        .map(|tagged| tagged.op.row)
        .collect();
    let removed: Vec<_> = stored
        .ops
        .iter()
        .filter(|tagged| !tagged.op.add)
        .map(|tagged| tagged.op.row)
        .collect();
    let mut signal = ApiSignal::of_rows(&added, &removed, naming);
    if stored.unreadable > 0 {
        signal.complete = false;
        signal.note.push_str(&format!(
            " {} stored row(s) could not be read.",
            stored.unreadable
        ));
    }
    signal
        .note
        .push_str(&format!(" Compared against {}.", short(previous)));
    signal.measurement(policy)
}

/// The hotspot dimension, from a graph already in hand and the caller's churn window.
fn hotspots_of(
    graph: &Graph,
    naming: &Naming,
    tree: &str,
    churn: Option<&ChurnReport>,
    knobs: &Knobs,
    notices: &[String],
) -> Measurement {
    let mut own: Vec<String> = notices.to_vec();
    own.push(
        "no structural records are stored per revision in this index, so the hotspot ranking runs on the \
         metrics the graph supplies — fan-in, fan-out, instability and churn — with size and complexity \
         unmeasured rather than zero"
            .to_owned(),
    );
    let empty: &[FileFacts] = &[];
    let table = table_from(tree, graph, naming, empty, churn, &knobs.hotspots, &own);
    hotspots(&table, &knobs.policy)
}

/// The instant, as the store's microseconds.
#[must_use]
fn micros(now: Timestamp) -> i64 {
    now.as_unix_micros()
}

/// A revision shortened the way every other pane shortens one.
fn short(revision: &str) -> String {
    if revision.chars().count() <= 12 {
        return revision.to_owned();
    }
    revision.chars().take(7).collect()
}
