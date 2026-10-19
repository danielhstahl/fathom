//! `narrative::compose` — the template that ranks before it prints (`e4.7`, `reco-2rx`).
//!
//! Spike `s5` scored the delta document's own rendering at 2.23 of 5 for usefulness and then named the
//! three reasons, all of them template bugs
//! ([`docs/concepts/narrative-polish.md`](../../../../docs/concepts/narrative-polish.md) §5): figures
//! printed in document order instead of by importance, the attention block relayed twice, and the
//! document's house style quoted at the reader. This module is the fix for those three, in the order the
//! spike gave them:
//!
//! 1. **Rank, then print.** Every claim gets an integer weight from the price list in [`weight_of`], and
//!    every paragraph is ordered by it. The claims at or above [`PRIORITY_FLOOR`] are lifted into *Read
//!    this first* and are **not** repeated below, because the promotion is the ordering and a duplicate is
//!    not an ordering.
//! 2. **Say each thing once.** A figure is claimed by exactly one sentence — the figures of a section are
//!    partitioned across claims — the ranking's component list is capped and the cut is counted, and the
//!    one pair the document states twice (`lines` and `churn`) is folded into a single sentence that cites
//!    both.
//! 3. **Phrase a gap as what the reader cannot do.** A gap is not printed as the delta document's sentence
//!    about its own plumbing; it becomes *You cannot …*, from the closed clause list in [`CANNOT`], with
//!    the owning plan key and the remedy beside it. The ranking's blind-spot list joins that paragraph in
//!    the same voice: what a score cannot see is a limit on the reader's view, not a list of accusations.
//!
//! # The rule that decides every sentence
//!
//! A sentence is built from figure values and nothing else, and it cites the figures it copied. That is what
//! lets [`super::Narrative::verify`] check the *content* of a claim rather than the shape of it: every
//! number in the prose has to appear in the text of the citations the claim carries. Nothing in this file
//! adds up, averages or infers, and nothing supplies a motive: the only "why" the narrative is allowed is
//! the recorded intent, quoted, with the rung that recorded it — the same discipline `e4.2` holds the intent
//! block to.

use std::collections::BTreeMap;

use crate::insight::{self, EpisodeDelta, Figure, Section as DeltaSection, SectionId};

use super::{Address, CHECKS, CONTRACT, Citation, Claim, GENERATOR, Integrity, Narrative, VERSION};

/// The paragraph a promoted claim belongs to.
pub const HEADLINE: &str = "read-first";
/// The paragraph every gap belongs to, rephrased as what the reader cannot do.
pub const GAP_SECTION: &str = "cannot";

/// How many claims the *Read this first* paragraph holds by default.
pub const TOP_DEFAULT: usize = 3;
/// The most it will ever hold — past this the paragraph is the whole narrative.
pub const TOP_MAX: usize = 6;
/// The weight a claim needs before it is allowed into *Read this first*.
///
/// The floor is what keeps the paragraph honest on a quiet episode: a documentation-only turn with no
/// findings has nothing at or above 40, prints no *Read this first* paragraph, and does not invent one.
pub const PRIORITY_FLOOR: i64 = 40;

/// The paragraphs, in the order a reviewer reads them, each with the question it answers.
///
/// The first two are this module's own: `e2.11`'s seven questions start at *what changed*, and the delta
/// document carries attribution and intent on the episode rather than in a section. They are paragraphs
/// here because a person skimming an episode wants *who* and *why* before *what*, and because both are
/// answers the document already carries — which is the only reason printing them is not an invention.
pub const READING_ORDER: [(&str, &str, &str); 9] = [
    (
        "intent",
        "Why it was asked for",
        "What was this change for, in the asker's own words?",
    ),
    ("who", "Who wrote it", "Who wrote it, and what says so?"),
    (
        "what",
        "What changed",
        "What changed, once the noise is folded away?",
    ),
    (
        "where",
        "Where it landed",
        "Where did the code land, and did the tree move?",
    ),
    (
        "dependencies",
        "What it depends on",
        "What does the tree now depend on that it did not?",
    ),
    (
        "surface",
        "What surface it touches",
        "Which non-code surfaces moved?",
    ),
    (
        "tests",
        "What is tested",
        "What is tested after this change?",
    ),
    (
        "blast",
        "What it reaches",
        "How far does this change reach?",
    ),
    (
        "attention",
        "Why it is in front of you",
        "Why is this episode in front of a human?",
    ),
];

/// The clause per section for the gap paragraph: what the reader cannot do when that section is short.
///
/// Keyed by the delta's own section ids, so a gap cannot arrive without a sentence — and so the sentence is
/// about the reader's view rather than about this build's plumbing, which is the fix `s5` asked for.
pub const CANNOT: [(&str, &str); 7] = [
    (
        "what",
        "which of these lines a reformat and which a rewrite",
    ),
    ("where", "the shape of the move, rather than its endpoints"),
    ("dependencies", "what new thing the tree now depends on"),
    (
        "surface",
        "what somebody outside this review has to do as a result",
    ),
    ("tests", "which named cases were added, removed or weakened"),
    ("blast", "what depends on the paths this change touched"),
    ("attention", "why this episode was ranked where it was"),
];

/// The price list: attention, not probability — what a reviewer would regret missing.
///
/// One place, so that "why is that sentence first" has an answer a reviewer can disagree with instead of a
/// shrug. The prices are deliberately coarse: an operator surface and a directory rollup are an order of
/// magnitude apart in that currency, and four decimal places would be a precision the `s5` panel could not
/// have reproduced. Findings are priced by the severity their own detector published, so the narrative and
/// [`crate::detectors`] cannot disagree about what is `high`.
#[must_use]
pub fn weight_of(label: &str) -> i64 {
    match label {
        s if s.starts_with("[high]") => 60,
        s if s.starts_with("[notable]") => 42,
        s if s.starts_with("[note]") => 24,
        "risk" => 46,
        "intent" => 44,
        "operator surfaces" => 38,
        "reach" => 34,
        "headline" => 34,
        "reviewable churn" => 30,
        "moves" => 30,
        "shape" => 28,
        "test side" => 28,
        "test share of churn" => 26,
        "scope" => 26,
        "lines" => 22,
        "caveat" => 22,
        "blind spots" => 22,
        "plan" => 24,
        "binary" => 18,
        "attribution" => 18,
        "files" => 16,
        "review" => 12,
        "file rows" => 12,
        "directories" | "more directories" => 12,
        "path classes" | "path mix" | "change kinds" => 10,
        "surfaces" => 10,
        "dependencies" | "tests" => 10,
        "digest" => 20,
        "absent intent" => 18,
        "cannot see" => 8,
        "boundary" => 8,
        "churn" => 6,
        "basis" => 4,
        _ => 10,
    }
}

/// Is this attention-section figure one row of the risk vector?
/// [`crate::insight`] spells a component as `surfaces (50/50)`, so a parenthesised label carrying a ratio
/// is the component shape and nothing else in the document has it.
#[must_use]
pub fn is_component(label: &str) -> bool {
    label.ends_with(')') && label.contains('(') && label.contains('/')
}

/// What a *figure* is worth, as opposed to what its label is worth: the price list plus the two
/// demotions that keep the *Read this first* paragraph about what happened.
///
/// An unknown figure is informative — "nothing answered blast" is a fact about the build — but it is not
/// the fact a reviewer reads first, and a template that promotes its own gaps over the change is the
/// failure `s5` named when it said the delta "quotes the document's house style at the reader". A zero is
/// the same trap in the other direction: `0 file(s)` on the test side of a one-line documentation fix is
/// true, uninteresting, and would otherwise outrank the sentence that says what the episode did.
fn price(fig: &Figure, risk_score: Option<i64>) -> i64 {
    if fig.label.starts_with('[') {
        // A finding keeps the price its own severity set, whatever the value looks like.
        return weight_of(&fig.label);
    }
    if fig.value.is_empty() {
        return 10;
    }
    if fig.label == "risk" {
        // A score of zero is "no signal", and "no signal" is not the sentence to read first.
        return if risk_score == Some(0) {
            12
        } else {
            weight_of("risk")
        };
    }
    if fig.label == "operator surfaces" && fig.value.starts_with("none") {
        return 6;
    }
    if leads_with_zero(&fig.value) {
        return 12;
    }
    weight_of(&fig.label)
}

/// Does the first number in this value say nothing happened?
///
/// `+0 −0`, `0 of 0`, `0 file(s)` — a zero is a fact, and it is printed, but it is very rarely the fact a
/// reviewer should be told first, and a quiet episode dressed up as a loud one is how a queue loses trust.
fn leads_with_zero(value: &str) -> bool {
    let mut run = String::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            run.push(ch);
        } else if !run.is_empty() {
            break;
        }
    }
    !run.is_empty() && run.chars().all(|ch| ch == '0')
}

/// Capitalise the first word of a generated sentence, so the narrative reads as prose and not as a dump.
///
/// Only the first character, and only when it is an ASCII letter: a value that opens with a path or a digit
/// is left exactly as the document wrote it, because a template that rewrites what it quotes has started
/// paraphrasing.
fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out
        }
        _ => text.to_owned(),
    }
}

/// How a claim about one figure is phrased. Values are copied verbatim; the connective is all this module
/// contributes.
#[must_use]
pub fn sentence(label: &str, value: &str, reason: Option<&str>) -> String {
    if value.is_empty() {
        // An unknown figure is still a fact about this build. Printed as the absence it is, with its reason.
        return match reason {
            Some(why) => format!("Nothing answers *{label}*: {why}"),
            None => format!("Nothing answers *{label}*"),
        };
    }
    let quoted = format!("`{value}`");
    match label {
        "lines" => format!("The diff is {quoted} lines"),
        "files" => format!("It touches {quoted}"),
        "churn" => format!("Total churn is {quoted}"),
        "reviewable churn" => format!("Of that churn, what a reviewer reads is {quoted}"),
        "binary" => format!("Binary files: {quoted}"),
        "directories" | "directory" => format!("It landed in {quoted}"),
        "more directories" => format!("Spread across {quoted}"),
        "moves" => format!("It moved things: {value}"),
        "shape" => format!("The shape of it: {value}"),
        "surfaces" => format!("Surfaces touched: {quoted}"),
        "operator surfaces" => {
            if value.starts_with("none") {
                "Nothing here needs an action outside this review".to_owned()
            } else {
                format!("Someone outside this review has to act: {quoted}")
            }
        }
        "test side" => format!("On the test side: {quoted}"),
        "source side" => format!("On the source side: {quoted}"),
        "test share of churn" => format!("The test share of churn: {quoted}"),
        "headline" => format!("The answer: {value}"),
        "reach" => format!("Blast radius: {quoted}"),
        "risk" => format!("The ranking puts it at {quoted}"),
        "caveat" => format!("The ranking comes with a caveat: {value}"),
        "cannot see" => format!("What the ranking cannot see: {value}"),
        "basis" => format!("Basis: {value}"),
        other => format!("{}: {quoted}", capitalise(other)),
    }
}

/// What to tune the composition with. Every knob is a legibility limit, not a threshold: nothing here
/// changes *what* is true, only how much of it one paragraph lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// How many claims the *Read this first* paragraph holds, capped at [`TOP_MAX`].
    pub top: usize,
    /// How many risk components to name before the remainder is counted rather than listed.
    pub components: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options::new()
    }
}

impl Options {
    /// The shipped composition.
    #[must_use]
    pub const fn new() -> Self {
        Options {
            top: TOP_DEFAULT,
            components: 3,
        }
    }

    /// Ask for a longer or shorter *Read this first* paragraph.
    #[must_use]
    pub const fn with_top(mut self, top: usize) -> Self {
        self.top = if top > TOP_MAX { TOP_MAX } else { top };
        self
    }
}

/// One sentence's worth of composition, before citations get their ids.
#[derive(Debug, Clone)]
struct Draft {
    section: String,
    text: String,
    /// Keys into the pending citation list, resolved to ids in [`Composer::finish`].
    keys: Vec<usize>,
    weight: i64,
    /// A stable position, so two claims of equal weight always print in the same order.
    index: usize,
}

/// A citation before it has an id. Interned, so two claims that lean on the same figure share one row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    what: String,
    address: Address,
    text: String,
}

/// The composition state: draft claims by paragraph, and the citations they interned.
struct Composer<'a> {
    delta: &'a EpisodeDelta,
    options: Options,
    drafts: Vec<Draft>,
    pending: Vec<Pending>,
    interned: BTreeMap<(String, String, String), usize>,
    next: usize,
}

impl<'a> Composer<'a> {
    fn new(delta: &'a EpisodeDelta, options: Options) -> Self {
        Composer {
            delta,
            options,
            drafts: Vec::new(),
            pending: Vec::new(),
            interned: BTreeMap::new(),
            next: 0,
        }
    }

    /// The next stable position.
    fn stamp(&mut self) -> usize {
        let index = self.next;
        self.next += 1;
        index
    }

    /// Intern a citation, returning its key. Identical citations are one row, so a figure cited by three
    /// sentences is one row of the evidence table and not three that could drift.
    fn cite(&mut self, what: &str, address: Address, text: String) -> usize {
        let key = (String::from(what), format!("{address:?}"), text.clone());
        if let Some(found) = self.interned.get(&key) {
            return *found;
        }
        let index = self.pending.len();
        self.pending.push(Pending {
            what: String::from(what),
            address,
            text,
        });
        self.interned.insert(key, index);
        index
    }

    /// Record one sentence. A sentence with nothing to cite is refused here rather than caught later.
    fn push(&mut self, section: &str, text: String, keys: Vec<usize>, weight: i64) {
        if text.is_empty() || keys.is_empty() {
            return;
        }
        let index = self.stamp();
        self.drafts.push(Draft {
            section: section.to_owned(),
            text,
            keys,
            weight,
            index,
        });
    }

    /// The authority text for one figure: label, value and reason, because every one of those numbers has
    /// to be licensed by the row it came from.
    fn authority(fig: &Figure) -> String {
        if fig.value.is_empty() {
            return match &fig.reason {
                Some(reason) => format!("{}: unknown — {reason}", fig.label),
                None => format!("{}: unknown", fig.label),
            };
        }
        format!("{}: {}", fig.label, fig.value)
    }

    /// Where a figure's evidence lives, in the narrative's three forms. The figure's own evidence is
    /// republished rather than re-addressed: one pointer per fact, across the document and the prose.
    fn address(fig: &Figure) -> Address {
        let evidence = &fig.evidence;
        if let Some(pointer) = &evidence.pointer {
            return Address::Document(pointer.clone());
        }
        if let Some(run) = &evidence.run {
            return Address::Locator(run.clone());
        }
        Address::Ticket(evidence.owner.clone().unwrap_or_default())
    }
}

/// Compose the narrative for one delta document.
#[must_use]
pub fn compose(delta: &EpisodeDelta, options: &Options) -> Narrative {
    let mut composer = Composer::new(delta, *options);
    intent_claims(&mut composer);
    attribution_claims(&mut composer);
    for section in &delta.sections {
        section_claims(&mut composer, section);
    }
    gap_claims(&mut composer);
    composer.finish()
}

/// The *why* paragraph: the recorded intent in the asker's words, the digest that proves a request happened,
/// or the stated absence. Three states, three sentences — `e4.2`'s rule that "nothing was recorded" and
/// "nothing was asked" are different facts.
fn intent_claims(composer: &mut Composer<'_>) {
    let Some(intent) = &composer.delta.episode.intent else {
        return;
    };
    let section = "intent";
    let address = Address::Document("#/episode/intent".to_owned());
    let state = intent.state_of();
    if state == fathom_ingest::intent::State::Recorded {
        if let Some(text) = intent.text.as_deref() {
            let authority = format!(
                "intent, in the asker's words: {text} — recorded by `{}` at {:.2}, state {}",
                intent.source, intent.confidence, intent.state
            );
            let key = composer.cite("the recorded intent", address.clone(), authority);
            composer.push(
                section,
                format!("Asked for, in the asker's own words: “{text}”"),
                vec![key],
                weight_of("intent"),
            );
        }
        if !intent.scope.is_empty() {
            let key = composer.cite(
                "the declared scope",
                address.clone(),
                format!("declared scope: {}", intent.scope.join(", ")),
            );
            composer.push(
                section,
                format!(
                    "It was meant to stay inside `{}`",
                    intent.scope.join("`, `")
                ),
                vec![key],
                weight_of("scope"),
            );
        }
        if !intent.plan.is_empty() {
            let listed: Vec<String> = intent
                .plan
                .iter()
                .map(|item| match &item.status {
                    Some(status) => format!("{status}: {}", item.text),
                    None => item.text.clone(),
                })
                .collect();
            let key = composer.cite(
                "the plan items as stated",
                address.clone(),
                format!("plan: {}", listed.join(" | ")),
            );
            composer.push(
                section,
                format!("The plan it was working from: {}", listed.join("; ")),
                vec![key],
                weight_of("plan"),
            );
        }
        return;
    }
    if state == fathom_ingest::intent::State::Digest {
        let digest = intent
            .prompt
            .as_ref()
            .and_then(|prompt| prompt.sha256.as_deref())
            .unwrap_or("no digest recorded");
        let authority = format!(
            "prompt digest {digest}, chars {}, text not captured — recorded by `{}` at {:.2}",
            intent
                .prompt
                .as_ref()
                .and_then(|prompt| prompt.chars)
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
            intent.source,
            intent.confidence
        );
        let key = composer.cite("the prompt digest", address.clone(), authority);
        composer.push(
            section,
            format!("Something was asked and its words were not kept — the prompt's digest is `{digest}`"),
            vec![key],
            weight_of("digest"),
        );
        return;
    }
    let reason = intent
        .reason
        .as_deref()
        .unwrap_or("no hook event, no trailer, no prompt digest");
    let authority = format!(
        "no intent was recorded: {reason} — source {}, state {}",
        intent.source, intent.state
    );
    let key = composer.cite("the absence of an intent", address, authority);
    composer.push(
        section,
        format!("Nothing was recorded about what this was for: {reason}"),
        vec![key],
        weight_of("absent intent"),
    );
}

/// The *who* paragraph: the attribution with the rung and agent that decided it, the boundary rule that
/// grouped the episode, and the review state that says whether a human has looked.
fn attribution_claims(composer: &mut Composer<'_>) {
    let episode = &composer.delta.episode;
    let section = "who";
    if let Some(attribution) = &episode.attribution {
        let mut authority_parts = vec![format!(
            "attribution {} via {} at {:.2}",
            attribution.kind, attribution.source, attribution.confidence
        )];
        authority_parts.push(format!(
            "agent {}",
            attribution
                .agent
                .clone()
                .unwrap_or_else(|| "unnamed".to_owned())
        ));
        if let Some(reason) = &attribution.reason {
            authority_parts.push(format!("reason {reason}"));
        }
        authority_parts.push(format!(
            "evidence {}",
            attribution
                .evidence
                .iter()
                .map(|item| format!("{} {:.2} {}", item.source, item.confidence, item.detail))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
        let authority = authority_parts.join("; ");
        let key = composer.cite(
            "the attribution and its evidence",
            Address::Document("#/episode/attribution".to_owned()),
            authority,
        );
        let mut text = format!(
            "Written by {}, decided by `{}` at {:.2}",
            attribution.kind, attribution.source, attribution.confidence
        );
        if let Some(agent) = &attribution.agent {
            text.push_str(&format!(" — the evidence names `{agent}`"));
        }
        if let Some(reason) = &attribution.reason {
            text.push_str(&format!(" — {reason}"));
        }
        composer.push(section, text, vec![key], weight_of("attribution"));
    }
    let boundary_key = composer.cite(
        "the boundary rule that opened the episode",
        Address::Document("#/episode/boundary_rule".to_owned()),
        format!(
            "boundary rule {} at {}",
            episode.boundary_rule,
            episode
                .boundary_confidence
                .map_or_else(|| "unknown".to_owned(), |value| format!("{value:.2}"))
        ),
    );
    let review_key = composer.cite(
        "the review state",
        Address::Document("#/episode/review".to_owned()),
        format!(
            "review {}, live {}, {} member commit(s)",
            episode.review, episode.live, episode.n_commits
        ),
    );
    composer.push(
        section,
        format!(
            "Grouped into one episode by `{}`, and its review state is `{}` on a {} episode with {} commit(s) in it",
            episode.boundary_rule,
            episode.review,
            if episode.live { "live" } else { "rewritten-away" },
            episode.n_commits
        ),
        vec![boundary_key, review_key],
        weight_of("review"),
    );
}

/// The seven questions: one sentence per figure, ranked inside its paragraph, with the pairs the document
/// states twice folded into one.
fn section_claims(composer: &mut Composer<'_>, section: &DeltaSection) {
    let id = section.id.clone();
    let named = |label: &str| {
        section
            .figures
            .iter()
            .find(|fig| fig.label == label)
            .cloned()
    };
    let lines = named("lines");
    let churn = named("churn");
    let fold = id == SectionId::What.as_str() && lines.is_some() && churn.is_some();
    // A section that answered nothing is the gap paragraph's business, not this one's: printing "Nothing
    // answers reach" and then "You cannot tell what depends on these paths" is the relay `s5` counted, and
    // the second sentence is the one a reader can act on.
    let silent = section.state == insight::STATE_UNAVAILABLE;
    let mut components = 0_usize;
    let mut hidden_components = 0_usize;
    let mut component_authority = Vec::new();
    for fig in &section.figures {
        if fold && fig.label == "churn" {
            continue;
        }
        if fig.label == "cannot see" {
            // Routed to the gap paragraph, where a limit on the reader's view belongs.
            continue;
        }
        if silent && fig.value.is_empty() {
            continue;
        }
        if is_component(&fig.label) {
            component_authority.push(Composer::authority(fig));
            if components >= composer.options.components {
                hidden_components += 1;
                continue;
            }
            components += 1;
        }
        if fold && fig.label == "lines" {
            let churn = churn.as_ref().unwrap_or(fig);
            let key_lines = composer.cite(
                &format!("`{id}` figure `{}`", fig.label),
                Composer::address(fig),
                Composer::authority(fig),
            );
            let key_churn = composer.cite(
                &format!("`{id}` figure `{}`", churn.label),
                Composer::address(churn),
                Composer::authority(churn),
            );
            composer.push(
                &id,
                format!(
                    "{}, with total churn `{}`",
                    sentence(&fig.label, &fig.value, fig.reason.as_deref()),
                    churn.value
                ),
                vec![key_lines, key_churn],
                price(fig, composer.delta.risk.as_ref().map(|risk| risk.score))
                    .max(price(churn, None)),
            );
            continue;
        }
        let text = if let Some(label) = fig.label.strip_prefix('[') {
            // A finding. The severity and the kind are the headline; the message is the detector's own
            // sentence, quoted rather than summarised, because the detector already wrote the sentence and
            // this module does not get an opinion of its own.
            let (severity, kind) = label
                .split_once("] ")
                .map_or((String::from("notable"), label.to_owned()), |(s, k)| {
                    (s.to_owned(), k.to_owned())
                });
            format!("A {severity} finding, `{kind}`: {}", fig.value)
        } else {
            sentence(&fig.label, &fig.value, fig.reason.as_deref())
        };
        if text.is_empty() {
            continue;
        }
        let what = if fig.label.starts_with('[') {
            format!("finding `{}`", fig.label)
        } else {
            format!("`{id}` figure `{}`", fig.label)
        };
        let key = composer.cite(&what, Composer::address(fig), Composer::authority(fig));
        let score = composer.delta.risk.as_ref().map(|risk| risk.score);
        composer.push(&id, text, vec![key], price(fig, score));
    }
    if hidden_components > 0 {
        // The cap, counted. A cut that is not counted is a lost fact — `e2.10`'s rule, applied here.
        let authority = component_authority.join(" | ");
        let key = composer.cite(
            "the ranking's remaining components",
            Address::Document("#/risk/components".to_owned()),
            authority,
        );
        composer.push(
            &id,
            "The remaining terms of the vector are in the document's `risk.components`".to_owned(),
            vec![key],
            6,
        );
    }
}

/// Every gap, rephrased as what the reader cannot do, plus the ranking's blind spots in the same voice.
fn gap_claims(composer: &mut Composer<'_>) {
    for (index, gap) in composer.delta.gaps.iter().enumerate() {
        let clause = CANNOT
            .iter()
            .find(|(section, _)| *section == gap.section.as_str())
            .map_or(
                "what the missing answer would have changed",
                |(_, clause)| *clause,
            );
        let authority = format!(
            "{} — {} (owner {}; remedy {})",
            gap.short,
            gap.reason,
            gap.owner,
            gap.remedy
                .clone()
                .unwrap_or_else(|| "none in this build".to_owned())
        );
        let key = composer.cite(
            &format!("the `{}` gap", gap.section),
            Address::Document(format!("#/gaps/{index}")),
            authority,
        );
        let mut text = format!(
            "You cannot tell {clause} — the document's own words are “{}”",
            gap.short
        );
        if let Some(remedy) = &gap.remedy {
            text.push_str(&format!("; meanwhile, run `{remedy}`"));
        }
        let weight = if gap.section == "blast" || gap.section == "tests" {
            23
        } else {
            15
        };
        composer.push(GAP_SECTION, text, vec![key], weight);
    }
    if let Some(risk) = &composer.delta.risk
        && !risk.blind_spots.is_empty()
    {
        let key = composer.cite(
            "what the ranking cannot see",
            Address::Document("#/risk/blind_spots".to_owned()),
            risk.blind_spots.join(" | "),
        );
        composer.push(
            GAP_SECTION,
            format!(
                "You cannot tell from the score: {}",
                risk.blind_spots.join("; ")
            ),
            vec![key],
            weight_of("blind spots"),
        );
    }
}

impl Composer<'_> {
    /// Rank inside the paragraphs, promote the headline, intern the citations, verify, and hand back.
    fn finish(mut self) -> Narrative {
        // Rank inside each paragraph: weight first, composition order second. The second key is what makes
        // two equal-priced figures land in the same place in every run, on every machine.
        self.drafts.sort_by(|a, b| {
            b.weight
                .cmp(&a.weight)
                .then(a.index.cmp(&b.index))
                .then(a.section.cmp(&b.section))
        });

        let slots = self.options.top.min(TOP_MAX);
        let promoted: Vec<usize> = self
            .drafts
            .iter()
            .enumerate()
            .filter(|(_, draft)| draft.weight >= PRIORITY_FLOOR)
            .map(|(index, _)| index)
            .take(slots)
            .collect();
        let mut headline: Vec<Draft> = Vec::new();
        let mut rest: Vec<Draft> = Vec::new();
        for (index, mut draft) in self.drafts.into_iter().enumerate() {
            if promoted.contains(&index) {
                draft.section = HEADLINE.to_owned();
                headline.push(draft);
            } else {
                rest.push(draft);
            }
        }
        let rank = |section: &str| -> usize {
            if section == HEADLINE {
                0
            } else if section == GAP_SECTION {
                READING_ORDER.len().saturating_add(1)
            } else {
                READING_ORDER
                    .iter()
                    .position(|(id, _, _)| *id == section)
                    .map_or(READING_ORDER.len(), |position| position.saturating_add(1))
            }
        };
        rest.sort_by(|a, b| {
            rank(&a.section)
                .cmp(&rank(&b.section))
                .then(b.weight.cmp(&a.weight))
                .then(a.index.cmp(&b.index))
        });
        let mut ordered = headline;
        ordered.extend(rest);

        // One sentence, once. Two sections can quote the same row — `surfaces` is in both `where` and
        // `surface` — and a paragraph that repeats the previous paragraph is the bug this module was written
        // to fix, whatever the section boundaries say.
        let mut said: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        ordered.retain(|draft| said.insert(draft.text.clone()));

        // Intern in final order: the ids come out contiguous, and every citation is cited by something.
        let mut claims: Vec<Claim> = Vec::new();
        let mut citations: Vec<Citation> = Vec::new();
        let mut id_of: BTreeMap<usize, String> = BTreeMap::new();
        for draft in ordered {
            let mut cites: Vec<String> = Vec::new();
            for key in &draft.keys {
                if let Some(found) = id_of.get(key) {
                    if !cites.contains(found) {
                        cites.push(found.clone());
                    }
                    continue;
                }
                let pending = &self.pending[*key];
                let id = format!("c{}", citations.len() + 1);
                id_of.insert(*key, id.clone());
                citations.push(Citation {
                    id: id.clone(),
                    what: pending.what.clone(),
                    address: pending.address.clone(),
                    text: pending.text.clone(),
                });
                cites.push(id);
            }
            if cites.is_empty() {
                continue;
            }
            claims.push(Claim {
                section: draft.section.clone(),
                text: draft.text.clone(),
                cites,
                weight: draft.weight,
            });
        }

        let checks = CHECKS.iter().map(|check| (*check).to_owned()).collect();
        let mut narrative = Narrative {
            contract: CONTRACT.to_owned(),
            schema_version: VERSION,
            generator: GENERATOR.to_owned(),
            source_contract: self.delta.contract.clone(),
            source_version: self.delta.schema_version,
            episode: insight::short_id(&self.delta.episode.id),
            title: format!(
                "{} — {}",
                insight::short_id(&self.delta.episode.id),
                self.delta.episode.summary
            ),
            claims,
            citations,
            integrity: Integrity {
                verified: false,
                problems: Vec::new(),
                checks,
            },
        };
        let problems = narrative.verify(self.delta);
        narrative.integrity.problems = problems;
        narrative.integrity.verified = narrative.integrity.problems.is_empty();
        narrative
    }
}
