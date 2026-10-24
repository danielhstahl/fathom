//! The plugin API: a detector written outside this repository (`e6.7`, `reco-raw`).
//!
//! The built-in catalogue in [`crate::detectors`] answers the questions this product is *about*: the ones
//! an agent-driven repository asks over and over. A repository with a question of its own has always had one
//! answer, which was "send a patch". This module is the second answer, and the whole of it is four things:
//! a trait, a versioned finding, a registry that applies the rules, and a selection grammar that turns
//! detectors on and off.
//!
//! # The shape, and why it is this shape
//!
//! ```text
//!     fathom analyze <episode> --json      ──►   your Detector::detect()   ──►   Vec<Finding>
//!        the immutable delta model                    pure, no capabilities         versioned, namespaced
//! ```
//!
//! * **The input is the delta document, not the store.** [`Input`] borrows an
//!   [`EpisodeDelta`] — the same versioned model `fathom analyze --json` prints, the TUI renders and the
//!   export ships (ADR-002 §5.3). A plugin therefore cannot be wrong in a way a reviewer cannot reproduce:
//!   the file that produced the finding is a file you can commit. It also means a plugin cannot open a
//!   store, run `git`, read a clock or reach the network, because it is not *handed* the capability — the
//!   same refusal [`crate::detectors`] makes of its own catalogue, enforced by the argument list rather
//!   than by review.
//! * **The output is a versioned finding**, [`Finding`], with the four parts every finding in this product
//!   carries: a sentence, a jump, a confidence, and one key that mutes it. Its type is namespaced
//!   (`acme.orphan_test`) so two out-of-tree detectors cannot raise `missing_test` and mean different
//!   things — see [the rules](finding).
//! * **The registry is the one that decides.** [`Registry::run`] re-checks every finding a plugin returns
//!   against the rules in [`finding`]. A plugin that lies about a severity, invents a type it never
//!   declared, or points at a view that does not exist gets a notice and a demotion, not a crash and not a
//!   silent pass: the least-trusted input in the product is the one place where "ignore it and say so" is
//!   the right verb (ADR-001 §7.6).
//! * **On and off is the same switch as everything else.** [`crate::detectors::policy::Selection`] — the
//!   `--detectors`, `--enable-detector` and `--disable-detector` grammar — is what a plugin is selected
//!   with, exactly as a built-in is. There is no second "plugin enable" mechanism to forget about.
//!
//! # Writing one
//!
//! ```text
//! use fathom_analyze::plugin::{Detector, Finding, FindingType, Input, Meta};
//! use fathom_analyze::detectors::{Evidence, EvidenceKind, Severity};
//!
//! struct OrphanTest;
//!
//! impl Detector for OrphanTest {
//!     fn meta(&self) -> Meta {
//!         Meta::new("acme", "orphan_test")
//!             .expect("a namespace and an id this crate owns")
//!             .version(1)
//!             .title("a test that tests nothing")
//!             .asks("does this episode add a test whose assertions all disappeared?")
//!             .declares(&["acme.orphan_test"])
//!     }
//!
//!                 fn detect(&self, input: &Input<'_>) -> Vec<Finding> {
//!         let mut out = Vec::new();
//!         for file in input.files() {
//!             if !file.path.starts_with("tests/") || file.add == 0 {
//!                 continue;
//!             }
//!             let finding = Finding::new(
//!                     FindingType::parse("acme.orphan_test").expect("declared above"),
//!                     "acme-orphan-test",
//!                     1,
//!                     input.episode_id(),
//!                     &file.path,
//!                     format!("{path} grew {add} lines and no assertion", path = file.path, add = file.add),
//!                 )
//!                 .with_severity(Severity::Low)
//!                 .with_evidence(Evidence::inferred(
//!                     EvidenceKind::Path,
//!                     file.path.clone(),
//!                     "added lines, no assertion seen",
//!                 ));
//!             out.push(finding);
//!         }
//!         out
//!     }
//! }
//! ```
//!
//! That is the whole contract. The worked version — a crate of its own, compiled out-of-tree against this
//! library, with a binary that reads a delta document on stdin and prints findings — is
//! [`plugins/acme-detectors`](../../../plugins/acme-detectors), and CI builds and runs it
//! ([`docs/concepts/extensibility.md`](../../../docs/concepts/extensibility.md)).
//!
//! # What is *not* here
//!
//! No dynamic loading, no `.so`, no `dlopen`, no subprocess supervisor. v1 publishes crates to nobody
//! (`publish = false`, ADR-002 §10.1's refusal), so "out-of-tree" means *a crate that compiles against this
//! library as a dependency and is linked into a binary by whoever ships it* — which is the only form of
//! plugin that can be reproduced, reviewed and statically linked on Windows. A plugin that wants to be a
//! separate process writes the JSONL this module reads ([`finding::read_stream`]); what it does not get is a
//! loader, and [docs/concepts/extensibility.md](../../../docs/concepts/extensibility.md) §7 says which
//! ticket would own one.

#![deny(missing_docs)]

use std::sync::Arc;

use serde::Serialize;

use crate::detectors::policy::Selection;
use crate::insight::{EpisodeDelta, EpisodeRef, FileDelta, Section, SectionId, Totals};

pub mod finding;
pub mod schema;

pub use crate::detectors::policy::Selection as DetectorSelection;
pub use finding::{
    Finding, FindingType, Limits, SCHEMA_VERSION as FINDING_SCHEMA_VERSION,
    Stream as FindingStream, TypeError as FindingTypeError, Verdict,
    from_line as finding_from_line, from_value as finding_from_value, gate,
    read_stream as read_finding_stream,
};

/// The version of the plugin API itself — the trait, [`Meta`] and this module's contracts.
///
/// A plugin declares the version it was written against through [`Meta::api_version`], and
/// [`Registry::register`] refuses a declaration this build cannot honour. That is a deliberate
/// asymmetry with the finding schema: a *finding* from a newer schema can be skipped, but a *trait impl*
/// from a newer API cannot be called at all, so finding out at registration beats finding out in a
/// monomorphised call three frames down.
pub const API_VERSION: u32 = 1;

/// What a detector is told.
///
/// Borrows, always: nothing here is a capability, and there is nothing to call. A detector that needs a
/// second look at a file asks for it in the finding's evidence — a pointer and a command, which is how
/// every other analysis in this crate hands evidence to a reviewer.
#[derive(Debug, Clone)]
pub struct Input<'a> {
    /// The episode's delta document — the model, unchanged, exactly as `fathom analyze --json` prints it.
    pub delta: &'a EpisodeDelta,
    /// The episodes before this one, oldest first. Empty when the read did not carry them, which a
    /// detector that needs a sequence must report as "not examined" rather than read as a clean history
    /// (the same refusal [`crate::detectors`] makes about an `absent` content state).
    pub prior: &'a [EpisodeRef],
}

impl<'a> Input<'a> {
    /// An input carrying one episode and nothing else.
    #[must_use]
    pub fn new(delta: &'a EpisodeDelta) -> Self {
        Input { delta, prior: &[] }
    }

    /// An input with the history a sequence-shaped detector needs.
    #[must_use]
    pub fn with_prior(delta: &'a EpisodeDelta, prior: &'a [EpisodeRef]) -> Self {
        Input { delta, prior }
    }

    /// The episode's id.
    #[must_use]
    pub fn episode_id(&self) -> &str {
        &self.delta.episode.id
    }

    /// The file rows, in path order.
    #[must_use]
    pub fn files(&self) -> &[FileDelta] {
        &self.delta.files
    }

    /// The scale of the change.
    #[must_use]
    pub fn totals(&self) -> &Totals {
        &self.delta.totals
    }

    /// One section of the document, by id.
    #[must_use]
    pub fn section(&self, id: SectionId) -> Option<&Section> {
        let name = id.as_str();
        self.delta
            .sections
            .iter()
            .find(|section| section.id == name)
    }

    /// Whether the read carried the file rows at all. `false` is a stated unknown, not a clean episode.
    #[must_use]
    pub fn has_files(&self) -> bool {
        !self.delta.files.is_empty()
    }

    /// Whether a history-shaped detector has anything to look at. One episode is not a sequence.
    #[must_use]
    pub fn has_history(&self) -> bool {
        self.prior.len() > 1
    }
}

/// Who a detector is, in the words the product prints about it.
///
/// Built through the chained setters so the two required fields (namespace, id) are validated first and
/// cannot be skipped:
///
/// ```text
/// Meta::new("acme", "orphan_test")?.version(1).title("a test that tests nothing")
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// The publisher's namespace, validated by [`Meta::new`].
    pub namespace: String,
    /// The detector's id within it, validated by [`Meta::new`].
    pub id: String,
    /// The detector's own version. Bump it when a rule change would move an existing episode's verdict —
    /// the same rule [`crate::detectors::Descriptor::version`] follows, and for the same reason: a stored
    /// finding has to be distinguishable from the one this build would produce.
    pub version: u32,
    /// The plugin API version this detector was written against ([`API_VERSION`]).
    pub api_version: u32,
    /// The one-line name a surface prints.
    pub title: String,
    /// The question it asks, in one sentence, printed beside every finding it raises.
    pub asks: String,
    /// Whether a repository that configures nothing gets it. `false` is the honest default for a detector
    /// nobody outside has measured yet, and `all` at the command line does not override it — a plugin is
    /// opted into by name.
    pub default_on: bool,
    /// The finding types this detector can raise, namespaced and complete. A finding whose type is not on
    /// this list is ignored with a notice (rule R2), which is what makes the list worth writing.
    pub declares: Vec<String>,
}

impl Meta {
    /// Name the detector. The two parts are validated here, once, at the boundary.
    ///
    /// # Errors
    ///
    /// [`finding::TypeError`] when either part is not a lowercase `[a-z][a-z0-9_]` name, is too long, is a
    /// reserved namespace, or shadows a built-in detector id.
    pub fn new(namespace: &str, id: &str) -> Result<Self, finding::TypeError> {
        let typed = FindingType::new(namespace, id)?;
        Ok(Meta {
            namespace: typed.namespace().to_owned(),
            id: typed.id().to_owned(),
            version: 1,
            api_version: API_VERSION,
            title: String::new(),
            asks: String::new(),
            default_on: false,
            declares: Vec::new(),
        })
    }

    /// The detector's version.
    #[must_use]
    pub fn version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// The title a surface prints.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The question it asks.
    #[must_use]
    pub fn asks(mut self, asks: impl Into<String>) -> Self {
        self.asks = asks.into();
        self
    }

    /// Whether a repository that configures nothing gets it.
    #[must_use]
    pub fn default_on(mut self, default_on: bool) -> Self {
        self.default_on = default_on;
        self
    }

    /// The finding types it can raise, in full `namespace.id` form.
    #[must_use]
    pub fn declares<I, S>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.declares = types
            .into_iter()
            .map(|item| item.as_ref().to_owned())
            .collect();
        self
    }

    /// The API version it declares. A plugin written against a newer API is refused at registration, so
    /// this is normally left alone.
    #[must_use]
    pub fn api_version(mut self, api_version: u32) -> Self {
        self.api_version = api_version;
        self
    }

    /// The detector's key: `namespace.id`, which is what `--enable-detector` takes and what a report row
    /// names.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}.{}", self.namespace, self.id)
    }

    /// The finding type of the detector's own name, for messages and for the common one-type plugin.
    #[must_use]
    pub fn finding_type(&self) -> FindingType {
        FindingType::new_unchecked(&self.namespace, &self.id)
    }
    /// The catalogue-shaped row a surface prints about this detector, so a plugin appears in the same list
    /// as a built-in and not in a side table nobody reads.
    #[must_use]
    pub fn row(&self) -> Vec<(&'static str, String)> {
        vec![
            ("id", self.key()),
            ("version", self.version.to_string()),
            ("title", self.title.clone()),
            ("asks", self.asks.clone()),
            ("declares", self.declares.join(",")),
            (
                "default",
                if self.default_on { "on" } else { "off" }.to_owned(),
            ),
        ]
    }
}

/// A detector written outside this repository.
///
/// Two methods, both pure. `detect` is handed an [`Input`] and returns findings; it may read nothing else,
/// and the type system is what says so: there is no store, no repository, no clock and no surface in the
/// argument list. Implementations must be `Send + Sync` because a registry is shared across the rayon
/// pool the index runs on.
pub trait Detector: Send + Sync {
    /// Who you are, and what you can say.
    fn meta(&self) -> Meta;

    /// What you say about one episode. No findings is an answer; `detect` is not asked to explain itself,
    /// because the registry prints what it did with a per-detector row whether or not anything arrived.
    fn detect(&self, input: &Input<'_>) -> Vec<Finding>;
}

/// Why a detector was not registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    /// The name was unusable.
    #[error("plugin `{0}` could not be named: {1}")]
    Name(String, String),
    /// The same key arrived twice.
    #[error(
        "plugin `{0}` is already registered; a second copy would print every finding twice and make \
         the reviewer decide which one to believe"
    )]
    Duplicate(String),
    /// The declared API version is not this one.
    #[error(
        "plugin `{plugin}` declares plugin API {declared}; this build is API {supported}. \
         Rebuild it against this version, or run it against a build that declares {declared}"
    )]
    ApiVersion {
        /// The plugin's key.
        plugin: String,
        /// What it asked for.
        declared: u32,
        /// What this build is.
        supported: u32,
    },
    /// The plugin declared no finding types, so nothing it raises could be checked.
    #[error(
        "plugin `{0}` declares no finding types; a plugin has to say what it can raise before it \
         raises it, or an invented type has no rule to catch it"
    )]
    Undeclared(String),
}

/// One registered detector.
#[derive(Clone)]
struct Entry {
    meta: Meta,
    detector: Arc<dyn Detector>,
}

/// What happened to one plugin detector during one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Run {
    /// The detector's key, `namespace.id`.
    pub detector: String,
    /// What happened to it.
    pub outcome: Outcome,
    /// Findings that survived the rules and the cap.
    pub findings: usize,
    /// Findings the row cap hid, counted rather than lost.
    pub hidden: usize,
    /// Findings the rules refused, each with a notice.
    pub rejected: usize,
    /// Findings silenced by `detectors.suppress` — the same keys, and the same matcher, the built-in
    /// catalogue uses.
    pub suppressed: usize,
}

/// What happened to one detector. The names are the built-in catalogue's, so one report has one vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// It ran and answered.
    Ran,
    /// Not selected by the [`Selection`] in force.
    Disabled,
    /// It needs something this read did not carry — history, usually.
    NotExamined,
    /// Every finding it raised was suppressed by key.
    Suppressed,
    /// Every finding it raised was refused by a rule. `notices` says which rule, per finding.
    Rejected,
}

impl Outcome {
    /// The printed spelling — the same strings `Serialize` writes, so a text render and a JSON render
    /// cannot name two outcomes for one row.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Outcome::Ran => "ran",
            Outcome::Disabled => "disabled",
            Outcome::NotExamined => "not_examined",
            Outcome::Suppressed => "suppressed",
            Outcome::Rejected => "rejected",
        }
    }
}

/// One run of the plugin registry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// The episode that was scanned.
    pub episode_id: String,
    /// The findings that survived, in registration order and then by severity and subject.
    pub findings: Vec<Finding>,
    /// One row per registered plugin detector, always every one of them.
    pub runs: Vec<Run>,
    /// One line per rule that fired or refused, and per plugin the run had to admit it could not use.
    pub notices: Vec<String>,
}

impl Report {
    /// The findings for one detector, by key or by plugin name.
    #[must_use]
    pub fn for_detector(&self, key: &str) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|finding| finding.plugin == key || finding.finding_type.as_str() == key)
            .collect()
    }

    /// How a detector answered, if it is registered.
    #[must_use]
    pub fn run(&self, key: &str) -> Option<&Run> {
        self.runs.iter().find(|run| run.detector == key)
    }

    /// The highest severity present, for a surface that prints one badge.
    #[must_use]
    pub fn ceiling(&self) -> Option<crate::detectors::Severity> {
        self.findings
            .iter()
            .map(|finding| finding.severity)
            .max_by_key(|severity| match severity {
                crate::detectors::Severity::Low => 0,
                crate::detectors::Severity::Medium => 1,
                crate::detectors::Severity::High => 2,
            })
    }

    /// The findings in the delta document's shape, for a caller that is about to compose or print one.
    #[must_use]
    pub fn delta_findings(&self) -> Vec<crate::insight::Finding> {
        self.findings
            .iter()
            .map(Finding::to_delta_finding)
            .collect()
    }
}

/// The registered plugin detectors, and the place the rules are applied.
///
/// A process builds one, registers what it links, and runs it per episode. Cloning is not offered on
/// purpose: a registry that can be cloned can be cloned into two lists that disagree about what is
/// registered, which is the second answer this product refuses everywhere.
#[derive(Default)]
pub struct Registry {
    entries: Vec<Entry>,
    notices: Vec<String>,
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a detector.
    ///
    /// Registration is where a plugin is checked, because it is the last moment before the plugin can say
    /// anything: the name has to be usable, unique and not a built-in's, the API version has to be one this
    /// build can call, and the finding types have to be declared. A refusal here is a *startup* failure
    /// with a sentence attached — the alternative is a plugin that silently never runs, which is the worst
    /// possible failure for a detector.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] for any of the four refusals above.
    pub fn register(&mut self, detector: Arc<dyn Detector>) -> Result<Meta, RegisterError> {
        let meta = detector.meta();
        let key = meta.key();
        if meta.api_version != API_VERSION {
            return Err(RegisterError::ApiVersion {
                plugin: key,
                declared: meta.api_version,
                supported: API_VERSION,
            });
        }
        if meta.declares.is_empty() {
            return Err(RegisterError::Undeclared(key));
        }
        for declared in &meta.declares {
            match FindingType::parse(declared) {
                Ok(typed) if typed.namespace() == meta.namespace => {}
                Ok(typed) => {
                    return Err(RegisterError::Name(
                        key,
                        format!(
                            "`{declared}` is declared by `{}` but named `{}` — a plugin may only declare \
                             types in its own namespace",
                            meta.namespace,
                            typed.namespace()
                        ),
                    ));
                }
                Err(err) => {
                    return Err(RegisterError::Name(key, format!("`{declared}`: {err}")));
                }
            }
        }
        if crate::detectors::by_id(&key).is_some() {
            return Err(RegisterError::Name(
                key,
                "that is a built-in detector's id; the built-in name space is bare and a plugin's is \
                 namespaced, and a collision would let a plugin mute a built-in's suppression family"
                    .to_owned(),
            ));
        }
        if self.entries.iter().any(|entry| entry.meta.key() == key) {
            return Err(RegisterError::Duplicate(key));
        }
        self.entries.push(Entry {
            meta: meta.clone(),
            detector,
        });
        Ok(meta)
    }

    /// Register several, keeping the refusals rather than stopping at the first.
    ///
    /// A binary that links three plugins and has one of them built against an older API should still run
    /// the other two and print the refusal — a partial analysis with a stated gap is the house answer, and
    /// the alternative is a plugin breaking a reviewer's whole run.
    pub fn register_all<I: IntoIterator<Item = Arc<dyn Detector>>>(
        &mut self,
        detectors: I,
    ) -> Vec<Result<Meta, RegisterError>> {
        detectors
            .into_iter()
            .map(|detector| self.register(detector))
            .collect()
    }

    /// How many are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none — the answer that lets a caller skip building a report at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The keys, in registration order.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.entries.iter().map(|entry| entry.meta.key()).collect()
    }

    /// Everything registered, in registration order — the manifest a `fathom plugins` listing prints.
    #[must_use]
    pub fn manifest(&self) -> Vec<Meta> {
        self.entries
            .iter()
            .map(|entry| entry.meta.clone())
            .collect()
    }

    /// One plugin's metadata, by key.
    #[must_use]
    pub fn meta(&self, key: &str) -> Option<&Meta> {
        self.entries
            .iter()
            .map(|entry| &entry.meta)
            .find(|meta| meta.key() == key)
    }

    /// The notices accumulated since the registry was built — refusals at registration time that a caller
    /// printing a report should print too.
    #[must_use]
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// Run every registered detector over one episode, and apply the rules to what comes back.
    ///
    /// Order is registration order, then severity descending, then subject: the same determinism rule the
    /// built-in scan follows, because a report that reorders itself between two runs of one input cannot
    /// be diffed, and a diff is how a reviewer notices something changed.
    #[must_use]
    pub fn run(&self, input: &Input<'_>, selection: &Selection, limits: &Limits) -> Report {
        let mut findings: Vec<Finding> = Vec::new();
        let mut runs: Vec<Run> = Vec::with_capacity(self.entries.len());
        let mut notices: Vec<String> = Vec::new();

        for entry in &self.entries {
            let meta = &entry.meta;
            let key = meta.key();
            if !selection.allows(&key, meta.default_on) {
                runs.push(Run {
                    detector: key,
                    outcome: Outcome::Disabled,
                    findings: 0,
                    hidden: 0,
                    rejected: 0,
                    suppressed: 0,
                });
                continue;
            }
            if meta.declares.is_empty() {
                // Unreachable through `register`, and still checked: the rules in `finding::gate` lean on
                // the declaration list, so an entry that got here by any other route must not run.
                notices.push(format!(
                    "{key}: registered without declaring anything, so it did not run"
                ));
                runs.push(Run {
                    detector: key,
                    outcome: Outcome::NotExamined,
                    findings: 0,
                    hidden: 0,
                    rejected: 0,
                    suppressed: 0,
                });
                continue;
            }

            let raised = entry.detector.detect(input);
            let suppressions = crate::detectors::policy::Suppressions::new(&limits.suppress);
            let mut kept: Vec<Finding> = Vec::new();
            let mut rejected = 0_usize;
            let mut suppressed = 0_usize;
            for finding in raised {
                let finding = match gate(&finding, meta, limits) {
                    Verdict::Kept(finding) => finding,
                    Verdict::Adjusted { finding, notice } => {
                        notices.push(format!("{key}: {notice}"));
                        finding
                    }
                    Verdict::Dropped { notice } => {
                        notices.push(format!("{key}: {notice}"));
                        rejected += 1;
                        continue;
                    }
                };
                if suppressions
                    .silences(&finding.finding_type.to_string(), &finding.suppression_key)
                {
                    suppressed += 1;
                    continue;
                }
                kept.push(finding);
            }
            kept.sort_by(|a, b| {
                severity_rank(b.severity)
                    .cmp(&severity_rank(a.severity))
                    .then(a.subject.cmp(&b.subject))
                    .then(a.message.cmp(&b.message))
            });
            let hidden = kept.len().saturating_sub(limits.max_findings);
            kept.truncate(limits.max_findings.max(1));
            let outcome = if kept.is_empty() && rejected > 0 {
                Outcome::Rejected
            } else if kept.is_empty() && suppressed > 0 {
                Outcome::Suppressed
            } else {
                Outcome::Ran
            };
            runs.push(Run {
                detector: key,
                outcome,
                findings: kept.len(),
                hidden,
                rejected,
                suppressed,
            });
            findings.extend(kept);
        }

        notices.extend(self.notices.clone());
        Report {
            episode_id: input.episode_id().to_owned(),
            findings,
            runs,
            notices,
        }
    }
}

/// Severity as a sort key: the highest first.
fn severity_rank(severity: crate::detectors::Severity) -> u8 {
    use crate::detectors::Severity;
    match severity {
        Severity::Low => 0,
        Severity::Medium => 1,
        Severity::High => 2,
    }
}
