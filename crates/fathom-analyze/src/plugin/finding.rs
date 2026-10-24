//! `finding.rs` — what a plugin says, in the one shape the product will accept (`e6.7`, `reco-raw`).
//!
//! Two things make a third-party finding safe to take: it says **what it is** in a name nobody else can
//! claim, and it can be **refused without refusing the plugin**. This module owns the first and [`super`]
//! owns the second.
//!
//! # Namespacing
//!
//! A built-in detector's id is bare and snake_case (`lint_escape`); a plugin's finding type is
//! `namespace.id` (`acme.orphan_test`), and the two spellings never overlap:
//!
//! * the namespace is the publisher's, chosen once, lowercase `[a-z][a-z0-9_]{0,31}`;
//! * `fathom`, `core`, `builtin` and `analyze` are reserved to this repository;
//! * a namespace may not be a built-in detector id, so a plugin cannot take `lint_escape`'s suppression
//!   family from it;
//! * the suppression key is `<namespace>.<id>:<subject>`, which is the key `detectors.suppress` matches —
//!   so muting one plugin's noise is the same act, in the same file, as muting a built-in's.
//!
//! # Severity rules
//!
//! The vocabulary is ADR-004 Appendix A's `high | medium | low` and nothing else — see
//! [`crate::detectors::Severity`], whose own header explains why a second vocabulary was refused. On top of
//! the vocabulary, a plugin's severity is checked by [`gate`] rather than trusted, in one place, so a
//! finding that arrives over JSON from another process is held to exactly the rules one built in Rust is:
//!
//! | rule | what it refuses | what happens |
//! |---|---|---|
//! | R1 | a `schema_version` this build cannot read | the finding is dropped, named, and the run continues |
//! | R2 | a finding type the plugin did not declare, or whose namespace is not the plugin's | dropped, named |
//! | R3 | an empty message or subject | dropped, named |
//! | R4 | a `high` with no evidence to point at | demoted to `medium`, with a notice |
//! | R5 | a confidence outside `0..=1`, or certainty on an all-inferred claim | clamped, with a notice |
//! | R6 | a suppression key outside the plugin's own namespace | rewritten to `<type>:<subject>`, with a notice |
//! | R7 | a jump whose view this build has no such name for | replaced by the episode jump, with a notice |
//! | R8 | more findings than the row cap | the rows past the cap are counted in `hidden`, not lost |
//!
//! Every one of those is a *notice*, not an error, and none of them can fail a run: this is ADR-001 §7.6's
//! "degrade, never block" applied to the boundary where the least-trusted input arrives. An unknown finding
//! type is the case the ticket names, and the answer is the same as the delta document's for an unknown
//! section — skip it, say so, carry on.

use serde::{Deserialize, Serialize};

use crate::detectors::{Evidence, Grade, Jump as DetectorJump, Severity, view};
use crate::insight;

/// The version of the finding shape published here.
///
/// Additive changes keep it; a removed field, a renamed one, a narrowed type or a new required one bumps
/// it. The number is written into every finding, so a reader refuses what it cannot read *before* it reads
/// it rather than discovering the difference in a partially understood object.
pub const SCHEMA_VERSION: u32 = 1;

/// The path the generated schema is checked in at (see [`crate::plugin::schema`]).
pub const SCHEMA_FILE: &str = "schemas/plugin-finding.schema.json";

/// The longest a namespace or an id may be. A detector id a reviewer has to select by double-click is not
/// a name, and 32 characters is well past the longest thing the shipped catalogue prints.
pub const MAX_SEGMENT: usize = 32;

/// Names a plugin may not publish under.
pub const RESERVED_NAMESPACES: &[&str] = &["fathom", "core", "builtin", "analyze", "detectors"];

/// Why a name was refused, in one sentence a plugin author can act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct TypeError {
    /// The refusal, printed verbatim.
    pub reason: String,
}

/// A finding type: `namespace.id`, validated on construction and on parse.
///
/// The dotted form is not decoration. It is what lets `detectors.suppress`, the store's `finding` rows and
/// a log line all say `acme.orphan_test` and mean one thing, and it is what makes "who said this?" a
/// question with an answer when four teams' detectors run over one repository.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FindingType {
    text: String,
    namespace: String,
    id: String,
}

impl FindingType {
    /// Build one from its two parts.
    ///
    /// # Errors
    ///
    /// [`TypeError`] when a segment is not lowercase `[a-z][a-z0-9_]`, when it is too long, when the
    /// namespace is reserved, or when the namespace names a built-in detector.
    pub fn new(namespace: &str, id: &str) -> Result<Self, TypeError> {
        segment(namespace, "namespace")?;
        segment(id, "id")?;
        if RESERVED_NAMESPACES.contains(&namespace) {
            return Err(TypeError {
                reason: format!(
                    "namespace `{namespace}` is reserved to this repository; publish under your own name"
                ),
            });
        }
        if crate::detectors::by_id(namespace).is_some() {
            return Err(TypeError {
                reason: format!(
                    "namespace `{namespace}` is a built-in detector id; a plugin may not raise findings \
                     under a built-in's name or take its suppression family"
                ),
            });
        }
        Ok(FindingType {
            text: format!("{namespace}.{id}"),
            namespace: namespace.to_owned(),
            id: id.to_owned(),
        })
    }

    /// Parse the printed form.
    ///
    /// # Errors
    ///
    /// [`TypeError`] when the text is not exactly `namespace.id`, or when a segment is not a name.
    pub fn parse(text: &str) -> Result<Self, TypeError> {
        let (namespace, id) = text.split_once('.').ok_or_else(|| TypeError {
            reason: format!(
                "`{text}` is not a namespaced finding type; a bare id like that belongs to a built-in \
                 detector, and a plugin's type is `namespace.id`"
            ),
        })?;
        if id.contains('.') {
            return Err(TypeError {
                reason: format!(
                    "`{text}` has more than one dot; the shape is `namespace.id` and nothing deeper, so \
                     that a suppression key always splits into type and subject on the first colon"
                ),
            });
        }
        Self::new(namespace, id)
    }

    /// The printed form, without re-checking it.
    ///
    /// Crate-internal, and only for a value that came from an already-validated [`super::Meta`]: the plugin
    /// registry refuses a name it cannot parse, so a `Meta` that exists has already been through
    /// [`FindingType::new`].
    pub(crate) fn new_unchecked(namespace: &str, id: &str) -> Self {
        FindingType {
            text: format!("{namespace}.{id}"),
            namespace: namespace.to_owned(),
            id: id.to_owned(),
        }
    }

    /// The publisher's namespace.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The type within it.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The printed form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The suppression key that mutes this type whole — the `detectors.suppress` entry a reviewer types
    /// after the third finding of a kind they have already decided about.
    #[must_use]
    pub fn suppression_key(&self) -> String {
        self.text.clone()
    }
}

impl std::fmt::Display for FindingType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for FindingType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for FindingType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Whether `text` is a usable segment.
fn segment(text: &str, which: &str) -> Result<(), TypeError> {
    if text.is_empty() {
        return Err(TypeError {
            reason: format!("the {which} is empty"),
        });
    }
    if text.len() > MAX_SEGMENT {
        return Err(TypeError {
            reason: format!(
                "the {which} `{text}` is {} characters; the ceiling is {MAX_SEGMENT}",
                text.len()
            ),
        });
    }
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return Err(TypeError {
            reason: format!("the {which} is empty"),
        });
    };
    if !first.is_ascii_lowercase() {
        return Err(TypeError {
            reason: format!("the {which} `{text}` must start with a lowercase letter"),
        });
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(TypeError {
            reason: format!(
                "the {which} `{text}` holds a character outside a-z, 0-9 and underscore; a hyphen in \
                 particular is what makes a key ambiguous with a flag"
            ),
        });
    }
    Ok(())
}

/// Where the surface opens, in the wire shape: a view *name* and a target.
///
/// [`crate::detectors::Jump`] holds a `&'static str` because the shipped catalogue is compiled in. A
/// finding arriving from another process cannot name a static, so the plugin-facing jump holds the name
/// and [`Jump::resolved`] maps it onto ADR-006 §4's registry ids — and says when it could not, which is
/// rule R7 rather than a silent guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jump {
    /// One of [`view`]'s names: `diff`, `timeline`, `structure`, `queue`, `graph`.
    pub view: String,
    /// What to open in it: `<episode>` or `<episode>#<path>`.
    pub target: String,
}

impl Jump {
    /// Open this episode's diff at this path — the jump almost every finding wants.
    #[must_use]
    pub fn diff(episode_id: &str, path: &str) -> Self {
        Jump {
            view: view::DIFF.to_owned(),
            target: format!("{episode_id}#{path}"),
        }
    }

    /// Open the episode itself, for a claim about the change as a whole.
    #[must_use]
    pub fn episode(episode_id: &str) -> Self {
        Jump {
            view: view::TIMELINE.to_owned(),
            target: episode_id.to_owned(),
        }
    }

    /// The view name, if this build has such a view.
    #[must_use]
    pub fn resolved(&self) -> Option<&'static str> {
        view::ALL
            .into_iter()
            .find(|known| *known == self.view.as_str())
    }

    /// The compiled form, or `None` when the view name is one this build does not have.
    #[must_use]
    pub fn to_detector_jump(&self) -> Option<DetectorJump> {
        let view = self.resolved()?;
        Some(DetectorJump {
            view,
            target: self.target.clone(),
        })
    }
}

/// One finding from a plugin.
///
/// Construct it with [`Finding::new`] — which takes the four parts a finding cannot be printed without —
/// and let the [registry](super::Registry) apply the rules above. The fields are public because a plugin
/// that has built the shape should not have to rebuild it; the gates run on the value, not on the route
/// taken to make it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// [`SCHEMA_VERSION`] when this build writes one.
    pub schema_version: u32,
    /// What it is, namespaced: `acme.orphan_test`.
    pub finding_type: FindingType,
    /// The plugin's own name, which is its namespace plus whatever it calls itself.
    pub plugin: String,
    /// The plugin's version, so a re-run after the plugin changed is distinguishable — the same job
    /// [`crate::detectors::Descriptor::version`] does for a built-in.
    pub plugin_version: u32,
    /// The episode it is about.
    pub episode_id: String,
    /// What within the episode: a path, a symbol, a test name.
    pub subject: String,
    /// The claim, in one sentence a person wrote.
    pub message: String,
    /// How much attention it asks for. ADR-004's three words, no fourth.
    pub severity: Severity,
    /// `0..=1`. Rule R5 is what keeps a `1.0` off an inferred claim.
    pub confidence: f64,
    /// Why the plugin thinks so. Rule R4 is what makes a `high` cite at least one piece.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
    /// Where the surface opens.
    pub jump: Jump,
    /// The one key that silences it: `<type>` or `<type>:<subject>`.
    pub suppression_key: String,
}

impl Finding {
    /// Start a finding with the four mandatory parts: what it is, what it says, where to look, and the key
    /// that mutes it.
    ///
    /// The key is derived — `<type>:<subject>` — because the alternative is a plugin author inventing a
    /// suppression spelling per finding and getting one wrong in the file a reviewer then has to guess.
    #[must_use]
    pub fn new(
        finding_type: FindingType,
        plugin: impl Into<String>,
        plugin_version: u32,
        episode_id: impl Into<String>,
        subject: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let subject = subject.into();
        let episode_id = episode_id.into();
        let key = if subject.is_empty() {
            finding_type.suppression_key()
        } else {
            format!("{}:{subject}", finding_type.as_str())
        };
        Finding {
            schema_version: SCHEMA_VERSION,
            jump: Jump::episode(&episode_id),
            finding_type,
            plugin: plugin.into(),
            plugin_version,
            episode_id,
            confidence: 0.7,
            message: message.into(),
            severity: Severity::Medium,
            subject,
            suppression_key: key,
            evidence: Vec::new(),
        }
    }

    /// The severity, set.
    #[must_use]
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// The confidence, set.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence;
        self
    }

    /// Another piece of evidence.
    #[must_use]
    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    /// The jump, set.
    #[must_use]
    pub fn with_jump(mut self, jump: Jump) -> Self {
        self.jump = jump;
        self
    }

    /// The suppression key, set. Rule R6 is what stops it naming another publisher.
    #[must_use]
    pub fn with_suppression_key(mut self, key: impl Into<String>) -> Self {
        self.suppression_key = key.into();
        self
    }

    /// The finding as the delta document prints it, so a plugin's finding lands in the attention section
    /// beside a built-in's and carries the same four parts there.
    #[must_use]
    pub fn to_delta_finding(&self) -> insight::Finding {
        insight::Finding {
            kind: self.finding_type.to_string(),
            subject: self.subject.clone(),
            message: self.message.clone(),
            severity: self.severity.as_str().to_owned(),
            // The pointer stays outside the document, and says so: the evidence is a path in the diff, and
            // the command that shows it is the command a reviewer can run.
            evidence: insight::Evidence::external(format!(
                "fathom analyze {} --json",
                self.episode_id
            )),
            owner: format!("{} {}", self.plugin, self.plugin_version),
        }
    }

    /// The wire form: one JSON value, this build's `schema_version` in it.
    ///
    /// # Errors
    ///
    /// `serde_json`'s error, which for this shape means an allocator problem or a float that is not a
    /// number (`NaN` confidence is refused rather than written as `null`).
    pub fn to_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    /// One line of the plugin wire format: the finding as one JSON object, no newline.
    ///
    /// # Errors
    ///
    /// As [`Finding::to_value`].
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// What [`gate`] decided about one finding.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The finding stands as written.
    Kept(Finding),
    /// The finding stands, changed by a rule, with the sentence that says which and why.
    Adjusted {
        /// The finding as it will be printed.
        finding: Finding,
        /// The notice, naming the rule and the value it refused.
        notice: String,
    },
    /// The finding is not printed, with the sentence that says why.
    Dropped {
        /// The notice.
        notice: String,
    },
}

/// What a plugin may not exceed, per episode.
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    /// Rule R8's row cap. Defaults to the built-in catalogue's, and is the same knob
    /// (`detectors.max_findings`) rather than a second one.
    pub max_findings: usize,
    /// The `detectors.suppress` keys in force. A plugin's finding is silenced by the same key the finding
    /// prints, which is the whole point of the key's shape: one file, one switch, no plugin-specific
    /// suppression grammar to learn.
    pub suppress: Vec<String>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_findings: crate::detectors::policy::DEFAULT_MAX_FINDINGS,
            suppress: Vec::new(),
        }
    }
}

impl From<&crate::detectors::policy::Policy> for Limits {
    fn from(policy: &crate::detectors::policy::Policy) -> Self {
        Limits {
            max_findings: policy.max_findings,
            suppress: policy.suppress.clone(),
        }
    }
}

/// Apply the severity and shape rules above to one finding a plugin raised.
///
/// This is a free function rather than a method for the reason the module header gives: the same rules have
/// to run on a finding that arrived as JSON from a process this crate never saw, and a method on the type
/// invites a caller to skip them by constructing the type instead.
#[must_use]
pub fn gate(finding: &Finding, meta: &super::Meta, limits: &Limits) -> Verdict {
    let typed = format!("{} v{}", meta.finding_type(), meta.version);

    // R1 — a version this build cannot read is a document from the future.
    if finding.schema_version != SCHEMA_VERSION {
        return Verdict::Dropped {
            notice: format!(
                "{typed} raised a finding declaring schema_version {}; this build reads \
                 {SCHEMA_VERSION}, so it was ignored",
                finding.schema_version
            ),
        };
    }

    // R2 — the type must be the plugin's own, and one it declared.
    if finding.finding_type.namespace() != meta.namespace {
        return Verdict::Dropped {
            notice: format!(
                "{typed} raised `{}`, which is not in its own namespace `{}` — a plugin may not raise \
                 findings for another publisher",
                finding.finding_type, meta.namespace
            ),
        };
    }
    if meta.declares.is_empty()
        || !meta
            .declares
            .iter()
            .any(|declared| declared == finding.finding_type.as_str())
    {
        return Verdict::Dropped {
            notice: format!(
                "`{}` is not a finding type {typed} declares ({}); it was ignored rather than \
                 guessed at. A plugin declares what it can say before it says it.",
                finding.finding_type,
                if meta.declares.is_empty() {
                    "nothing, so far".to_owned()
                } else {
                    meta.declares.join(", ")
                },
            ),
        };
    }

    // R3 — an empty sentence is not a finding.
    if finding.message.trim().is_empty() || finding.subject.trim().is_empty() {
        return Verdict::Dropped {
            notice: format!(
                "{typed} raised a {} `{}` with an empty {}; a finding is a claim and a claim has words",
                finding.finding_type,
                if finding.subject.trim().is_empty() {
                    "subject"
                } else {
                    "message"
                },
                if finding.subject.trim().is_empty() {
                    "subject"
                } else {
                    "message"
                }
            ),
        };
    }

    let mut adjusted: Vec<String> = Vec::new();
    let mut out = finding.clone();

    // R4 — `high` means "before this ships", and that claim owes a pointer.
    if out.severity == Severity::High && out.evidence.is_empty() {
        out.severity = Severity::Medium;
        adjusted.push(format!(
            "{} `{}` asked for high with no evidence to point at; severity is medium — an unpointed \
             claim is not a reason to block a merge",
            finding.finding_type, finding.subject
        ));
    }

    // R5 — the confidence ceiling, and its one exception.
    if !(0.0..=1.0).contains(&out.confidence) {
        let clamped = out.confidence.clamp(0.0, 1.0);
        adjusted.push(format!(
            "{} wrote confidence {}; it is a 0–1 fraction and is now {clamped}",
            finding.finding_type, out.confidence
        ));
        out.confidence = clamped;
    }
    let all_inferred = !out.evidence.is_empty()
        && out
            .evidence
            .iter()
            .all(|piece| matches!(piece.grade, Grade::Inferred));
    if all_inferred && out.confidence >= 1.0 {
        out.confidence = 0.95;
        adjusted.push(format!(
            "{} claimed certainty on a claim every piece of evidence calls inferred; confidence is \
             now 0.95 — the same price the built-in catalogue pays for the same claim",
            finding.finding_type
        ));
    }

    // R6 — the key stays inside the publisher's own namespace.
    let prefix = format!("{}.", meta.namespace);
    if !finding.suppression_key.starts_with(prefix.as_str()) {
        adjusted.push(format!(
            "{} wrote suppression key `{}`, which is outside its own namespace; it is now \
             `{}:{}` — a plugin may mute its own findings and nothing else",
            finding.finding_type, finding.suppression_key, finding.finding_type, finding.subject
        ));
        out.suppression_key = format!("{}:{}", finding.finding_type, finding.subject);
    }

    // R7 — a jump to a view this build has no such name for is a jump to the episode.
    if out.jump.resolved().is_none() {
        adjusted.push(format!(
            "{} pointed at view `{}`, which is not one of {}; the jump opens the episode instead",
            finding.finding_type,
            out.jump.view,
            view::ALL.join(", ")
        ));
        out.jump = Jump::episode(&finding.episode_id);
    }

    let _ = limits; // R8 is a per-run count, applied by the registry that holds the whole list.
    if adjusted.is_empty() {
        Verdict::Kept(out)
    } else {
        Verdict::Adjusted {
            finding: out,
            notice: adjusted.join("; "),
        }
    }
}

/// A finding read from the wire, before it is gated.
pub fn from_value(value: &serde_json::Value) -> Result<Finding, String> {
    serde_json::from_value::<Finding>(value.clone()).map_err(|err| format!("{err}"))
}

/// One line of a plugin's output, read.
///
/// Never fatal. A line that is not JSON, is not an object, or does not describe a finding this build can
/// read comes back as `Err(reason)` and the run goes on with the findings that did read — the ticket's
/// "unknown finding types are ignored rather than fatal", at the byte boundary rather than in the type.
pub fn from_line(line: &str) -> Result<Finding, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("blank line".to_owned());
    }
    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|err| format!("not JSON: {err}"))?;
    from_value(&value)
}

/// A whole plugin stream: the findings that read, and one line per the ones that did not.
#[derive(Debug, Clone, Default)]
pub struct Stream {
    /// The findings that parsed, in file order.
    pub findings: Vec<Finding>,
    /// One line per refused line, with its number in the stream.
    pub ignored: Vec<String>,
}

/// Read a JSONL stream of findings.
#[must_use]
pub fn read_stream(text: &str) -> Stream {
    let mut stream = Stream::default();
    for (index, line) in text.lines().enumerate() {
        match from_line(line) {
            Ok(finding) => stream.findings.push(finding),
            Err(reason) => {
                if !line.trim().is_empty() {
                    stream.ignored.push(format!("line {}: {reason}", index + 1));
                }
            }
        }
    }
    stream
}
