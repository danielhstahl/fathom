//! `fathom_analyze::narrative` — the episode narrative: prose a human can skim, with a citation on
//! every claim (`e4.7`, `reco-2rx`).
//!
//! The delta document answers seven questions in seven rows of figures. That is the right shape for a
//! machine and the wrong shape for a person who has ninety seconds: spike `s5` measured exactly that and
//! scored the template's readability at 2.23 of 5 while scoring its accuracy at 4.667
//! ([`docs/concepts/narrative-polish.md`](../../../docs/concepts/narrative-polish.md)). The conclusion the
//! spike recorded is the design of this module: the prose problem was a prose bug, and the fix is a
//! composer that ranks before it prints, says each thing once, and never writes a sentence that the
//! document it reads cannot back up.
//!
//! # The one rule
//!
//! **A claim cites, or the claim does not exist.** Every sentence this module emits carries one or more
//! citation ids, every citation resolves to an address the reader can go and look at, and every number a
//! sentence prints appears inside the text of the citation beside it. That is not a style rule the
//! composer tries to obey; it is what [`Narrative::verify`] fails the narrative on, and it is checked in
//! the same pass that checks the delta document checks itself.
//!
//! The composer therefore has no vocabulary of its own beyond the connectives, no motive sentences, and no
//! channel to invent anything through: it copies figure values out of the document and prints the pointer
//! the document already carried. Spike `s5` made the same point about its control arm and it is worth
//! repeating here, because it is the whole argument against the alternative ([`llm`]): a template is not
//! *less likely* to hallucinate than a model, it has nothing to hallucinate with.
//!
//! # What is *not* here, deliberately
//!
//! **No model.** Spike `s5` measured a polished arm against this one and shipped neither the polish nor
//! the seam that would have carried it ([`reco-nv0`]'s decision, recorded in
//! [`llm::SHIPPED`] as `false`): zero measured win on trustworthiness, coverage halved, and byte-stability
//! lost. `net-llm` has no consumer in this crate, and [`llm::assert_unwired`] is the test that keeps it
//! that way — five gates in `reco-nv0` have to be cleared before a code path appears here, not one
//! opinion that the prose could read better.
//!
//! **No store, no clock, no config.** [`compose`] is a pure function of one [`EpisodeDelta`] and one
//! [`Options`], which is what makes its output byte-stable (the criterion below) and what lets `e5.7`
//! bake a golden artefact out of it. A surface that wants the narrative for a page reads the deltas and
//! calls this, the same way it does for [`crate::insight::render`].
//!
//! **No second set of figures.** Everything printed here is a value the document already carries, and
//! every citation re-publishes the evidence pointer that came with it. A narrative that recomputed a
//! number would be a second opinion, which is the thing `e2.11` exists to prevent.
//!
//! # Determinism, and why a release artefact depends on it
//!
//! Same document in, same bytes out: no clock, no locale, no terminal probe, no hash-order iteration.
//! `e5.7`'s exported artefacts are blessed files, and a blessed file whose prose moved between two runs of
//! the same build is not a record of anything. The test asserts it over every fixture in the corpus, in
//! the same process and through a JSON round-trip, so a figure that stopped serialising cannot hide behind
//! a re-parse.
//!
//! [`reco-nv0`]: ../../../.beads
//! [`llm`]: crate::narrative::llm

#![deny(missing_docs)]

mod compose;
pub mod llm;

use serde::Deserialize;
use serde::Serialize;

use crate::insight::EpisodeDelta;

pub use compose::{
    CANNOT, GAP_SECTION, HEADLINE, Options, PRIORITY_FLOOR, READING_ORDER, TOP_DEFAULT, TOP_MAX,
    compose, is_component, sentence, weight_of,
};

/// The heading of the gap paragraph — what the reader cannot do, said that way.
pub const GAP_TITLE: &str = "What you cannot tell from this document";

/// The contract id of a narrative, so a consumer can tell prose from a document.
///
/// Versioned independently of [`crate::insight::CONTRACT`]: the delta is the evidence and this is the
/// prose, and a change to the phrasing must not read as a change to the model.
pub const CONTRACT: &str = "fathom-narrative/1";

/// The version of [`CONTRACT`]'s shape.
pub const VERSION: u32 = 1;

/// Who wrote the sentences, printed in the artefact and in `--json`.
///
/// This is the watermark. The acceptance criterion `e4.7` wrote assumed two authors — template and model —
/// and asked that the model's contribution be marked. `s5` removed the second author, and the stamp stays:
/// it is the line that tells a reader, in the artefact itself, whether a model was in the path. A future
/// polished narrative would carry a different stamp and the same field, and nothing downstream would have
/// to guess.
pub const GENERATOR: &str = "fathom-template/1";

/// What the generator did *not* do, in one line, printed under every rendering.
///
/// Short enough to live at the bottom of a PR comment, specific enough to be checkable: it names the
/// contract the figures came from, so the stamp and the evidence cannot disagree about which build wrote
/// the artefact.
#[must_use]
pub fn watermark(delta: &EpisodeDelta) -> String {
    format!(
        "*Composed by `{GENERATOR}` from `{}` v{} — a template, not a model: every figure above is copied \
         from the citation printed beside it.*",
        delta.contract, delta.schema_version
    )
}

/// The same stamp without the Markdown, for the text rendering.
#[must_use]
pub fn watermark_text() -> String {
    format!(
        "{GENERATOR}: a template, not a model — every figure above is copied from the citation \
             printed beside it"
    )
}

/// Where a citation's evidence actually lives.
///
/// The three forms are the delta model's three ([`crate::insight::Evidence`]) and no more: the narrative
/// republishes pointers, it does not invent addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Address {
    /// A JSON pointer into the delta document this narrative was composed from, e.g. `#/files/3`.
    Document(String),
    /// A command or locator the reader can act on — `fathom deps <episode> --json`, or
    /// `crates/fathom-store/src/spine.rs:240-262`.
    Locator(String),
    /// The plan key of the ticket that owns the answer, e.g. `e3.2`.
    Ticket(String),
}

impl Address {
    /// The published spelling, as `--json` prints the `kind` field.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Address::Document(_) => "document",
            Address::Locator(_) => "locator",
            Address::Ticket(_) => "ticket",
        }
    }

    /// The address itself, whatever the form.
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            Address::Document(text) | Address::Locator(text) | Address::Ticket(text) => text,
        }
    }
}

/// One addressable piece of evidence, with the text a claim is allowed to assert.
///
/// `text` is not decoration: it is the *authority* for the sentence. [`Narrative::verify`] checks every
/// number a claim prints against the text of the citations it carries, so a citation that quotes too
/// little is caught as reliably as a claim that asserts too much.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Citation {
    /// The short id printed in the prose, e.g. `e7`.
    pub id: String,
    /// What it is, e.g. ``what` figure `reviewable churn``.
    pub what: String,
    /// The address, as the reader would use it.
    pub address: Address,
    /// The evidence in its own words — the figure's value, the file row, the gap's reason.
    pub text: String,
}

impl Citation {
    /// The printable address, with the command or ticket spelled out.
    #[must_use]
    pub fn address_text(&self) -> String {
        match &self.address {
            Address::Document(pointer) => pointer.clone(),
            Address::Locator(run) => run.clone(),
            Address::Ticket(ticket) => format!("owned by {ticket}"),
        }
    }
}

/// One sentence of prose, with the ids it cites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// Which paragraph it belongs to — one of [`READING_ORDER`]'s names, or `read first`.
    pub section: String,
    /// The sentence, already punctuated, without its citation markers.
    pub text: String,
    /// The citation ids it asserts, in the order they are printed. Never empty, and
    /// [`Narrative::verify`] is the check that keeps it that way.
    pub cites: Vec<String>,
    /// The importance this claim was ranked at. Printed by `--json` and by `fathom narrative --explain`
    /// so that "why is that sentence first" is a number the reader can disagree with.
    pub weight: i64,
}

impl Claim {
    /// The sentence with its citations appended, e.g. `… churn 9 356 [c1][c4].`
    #[must_use]
    pub fn rendered(&self) -> String {
        let mut out = self.text.clone();
        for id in &self.cites {
            out.push_str(&format!("[{id}]"));
        }
        out.push('.');
        out
    }
}

/// A composed narrative: the claims, the evidence they cite, and its own verdict on the pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Narrative {
    /// [`CONTRACT`].
    pub contract: String,
    /// [`VERSION`].
    pub schema_version: u32,
    /// [`GENERATOR`] — the watermark.
    pub generator: String,
    /// The contract of the document this was composed from, so the prose and the evidence name each other.
    pub source_contract: String,
    /// The version of that document, so the stamp can be checked against the artefact it describes.
    pub source_version: u32,
    /// The episode this is about — an id and a summary, and nothing else.
    pub episode: String,
    /// The heading line: the episode's own subject, quoted, never rephrased.
    pub title: String,
    /// Every claim, in printing order.
    pub claims: Vec<Claim>,
    /// Every citation, in the order first cited.
    pub citations: Vec<Citation>,
    /// The self-check: what was verified, and what failed.
    pub integrity: Integrity,
}

/// A narrative's verdict on itself, in the same shape the delta model uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Integrity {
    /// Whether [`Narrative::verify`] found nothing.
    pub verified: bool,
    /// One line per problem, empty when `verified`.
    pub problems: Vec<String>,
    /// What was checked, so `verified` is never a bare boolean.
    pub checks: Vec<String>,
}

/// What was checked, printed with every narrative (`e2.11`'s rule that a self-check names itself).
pub const CHECKS: [&str; 6] = [
    "every claim carries at least one citation",
    "every citation id resolves, and every citation is cited by at least one claim",
    "every document pointer resolves inside the delta document this narrative was composed from, and the \
     narrative was verified against the episode it was composed for",
    "every command or locator citation names a place the reader can go, and every ticket citation names a plan key",
    "every number a claim prints appears in the text of the citations it carries",
    "the citation table is ordered by id and no claim cites the same id twice",
];

impl Narrative {
    /// Compose the narrative for one delta document.
    ///
    /// This is the whole product surface: no store, no clock, no config beyond [`Options`].
    #[must_use]
    pub fn for_delta(delta: &EpisodeDelta, options: &Options) -> Self {
        compose(delta, options)
    }

    /// The Markdown rendering — the text `e5.7`'s artefact uses verbatim.
    ///
    /// Paragraphs are headed, claims are bullets, and the evidence table is the last thing before the
    /// self-check line, so a reader who distrusts a sentence has one screen to look down to.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("## {}\n\n", self.title));
        let mut section: Option<String> = None;
        for claim in &self.claims {
            if section.as_deref() != Some(claim.section.as_str()) {
                if section.is_some() {
                    out.push('\n');
                }
                out.push_str(&format!("**{}**\n\n", title_of(&claim.section)));
                section = Some(claim.section.clone());
            }
            out.push_str(&format!("- {}\n", claim.rendered()));
            if claim.cites.is_empty() {
                // Unreachable through [`compose`], and still refused at render time: a claim that arrived
                // without its evidence should not be printable at all.
                out.pop();
                out.push_str(&format!("- {} *[no evidence attached]*\n", claim.text));
            }
        }
        out.push('\n');
        out.push_str("Evidence\n\n");
        for citation in &self.citations {
            out.push_str(&format!(
                "- `[{}]` {} — {} — {}\n",
                citation.id,
                citation.what,
                citation.address_text(),
                citation.text
            ));
        }
        out.push('\n');
        out.push_str(&self.stamp());
        out.push('\n');
        out.push_str(&self.integrity_line());
        out.push('\n');
        out
    }

    /// The plain-text rendering, for a terminal that has no Markdown.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = vec![self.title.clone(), String::new()];
        let mut section: Option<String> = None;
        for claim in &self.claims {
            if section.as_deref() != Some(claim.section.as_str()) {
                if section.is_some() {
                    out.push(String::new());
                }
                out.push(format!("{}:", title_of(&claim.section)));
                section = Some(claim.section.clone());
            }
            out.push(format!("  {}", claim.rendered()));
        }
        out.push(String::new());
        out.push("Evidence:".to_owned());
        for citation in &self.citations {
            out.push(format!(
                "  [{}] {} — {} — {}",
                citation.id,
                citation.what,
                citation.address_text(),
                citation.text
            ));
        }
        out.push(String::new());
        out.push(watermark_text());
        out.push(self.integrity_line());
        out.join("\n")
    }

    /// The ranking, as lines: why each sentence sits where it does (`fathom narrative --explain`).
    ///
    /// Kept out of the prose itself on purpose: a weight printed inside a sentence would be a number the
    /// sentence's own citations do not license, and the audit in [`Narrative::verify`] is what keeps this
    /// module honest. The ordering rationale belongs beside the narrative, not inside it.
    #[must_use]
    pub fn explain_lines(&self) -> Vec<String> {
        self.claims
            .iter()
            .map(|claim| format!("{:>4}  {:<22} {}", claim.weight, claim.section, claim.text))
            .collect()
    }

    /// The stamp, built from the contract this narrative was composed from — the same sentence
    /// [`watermark`] prints for a document handed over directly.
    #[must_use]
    pub fn stamp(&self) -> String {
        format!(
            "*Composed by `{GENERATOR}` from `{}` v{} — a template, not a model: every figure above is \
             copied from the citation printed beside it.*",
            self.source_contract, self.source_version
        )
    }

    /// The structure itself, for a machine — one line, keys sorted, byte-stable.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    /// Read a narrative back.
    ///
    /// # Errors
    ///
    /// `serde_json::Error` when the bytes are not a narrative.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The self-check line, printed as the last line of every rendering.
    #[must_use]
    pub fn integrity_line(&self) -> String {
        if self.integrity.verified {
            format!(
                "{GENERATOR} verified: {} claims, {} citations, {} checks.",
                self.claims.len(),
                self.citations.len(),
                self.integrity.checks.len()
            )
        } else {
            format!(
                "{GENERATOR} UNVERIFIED — {} problem(s): {}",
                self.integrity.problems.len(),
                self.integrity.problems.join(" | ")
            )
        }
    }

    /// Re-read the narrative the way an auditor would, and return one line per thing that does not hold.
    ///
    /// [`CHECKS`] lists what is checked and travels with the payload; empty means every claim carried its
    /// evidence. The numeral rule is the load-bearing one: it is what makes "every claim cites" a fact
    /// about the *content* rather than about the presence of a bracket.
    #[must_use]
    pub fn verify(&self, delta: &EpisodeDelta) -> Vec<String> {
        let mut problems = Vec::new();

        if self.contract != CONTRACT {
            problems.push(format!(
                "contract `{}` is not this build's `{CONTRACT}`",
                self.contract
            ));
        }
        if self.source_contract != delta.contract {
            problems.push(format!(
                "composed from `{}` but verified against `{}` — prose and evidence are not the same model",
                self.source_contract, delta.contract
            ));
        }
        if self.episode != crate::insight::short_id(&delta.episode.id) {
            problems.push(format!(
                "composed for `{}` but verified against `{}` — a citation that resolves in the wrong \
                 document is worse than one that does not resolve",
                self.episode,
                crate::insight::short_id(&delta.episode.id)
            ));
        }
        if self.integrity.checks != expected_checks() {
            problems.push("the payload does not list the checks this build performs".to_owned());
        }

        let mut by_id: std::collections::BTreeMap<&str, &Citation> =
            std::collections::BTreeMap::new();
        for citation in &self.citations {
            if by_id.insert(citation.id.as_str(), citation).is_some() {
                problems.push(format!("citation id `{}` appears twice", citation.id));
            }
        }
        let mut cited: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

        // 1 and 2 and 5 — one pass per claim: a citation, a resolvable id, and numbers the citations
        // actually carry.
        for claim in &self.claims {
            let where_ = format!("claim in `{}` ({})", claim.section, snippet(&claim.text));
            if claim.cites.is_empty() {
                problems.push(format!("{where_}: no citation — an unattached assertion"));
                continue;
            }
            let mut authority = String::new();
            let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
            for id in &claim.cites {
                let Some(citation) = by_id.get(id.as_str()) else {
                    problems.push(format!(
                        "{where_}: cites `{id}`, which is not in the citation table"
                    ));
                    continue;
                };
                if !seen.insert(id.as_str()) {
                    problems.push(format!("{where_}: cites `{id}` twice"));
                }
                cited.insert(citation.id.as_str());
                authority.push(' ');
                authority.push_str(&citation.text);
                match &citation.address {
                    Address::Document(pointer) => {
                        if !delta.resolves(pointer) {
                            problems.push(format!(
                                "{where_}: pointer `{pointer}` resolves to nothing in the document"
                            ));
                        }
                    }
                    Address::Locator(run) => {
                        if !locator_shaped(run) {
                            problems.push(format!(
                                "{where_}: evidence address `{run}` is neither a command this binary \
                                 answers to nor a single locator the reader can open"
                            ));
                        }
                    }
                    Address::Ticket(ticket) => {
                        if !plan_key(ticket) {
                            problems.push(format!(
                                "{where_}: owner `{ticket}` is not a plan key this build can name"
                            ));
                        }
                    }
                }
            }
            // 5 — every numeral the sentence asserts must be in the text it cited. Space-separated digit
            // grouping (`9 356`) is folded first, because the document groups and the sentence may not.
            let allowed = digit_runs(&authority);
            for number in digit_runs(&strip_citations(&claim.text)) {
                if !allowed.contains(&number) {
                    problems.push(format!(
                        "{where_}: asserts `{number}`, which none of its citations print"
                    ));
                }
            }
        }

        // 2, second half — a citation nobody cites is a decoration.
        for citation in &self.citations {
            if !cited.contains(citation.id.as_str()) {
                problems.push(format!(
                    "citation `{}` ({}) is cited by no claim",
                    citation.id, citation.what
                ));
            }
        }

        // 6 — the table is ordered, so a diff between two renderings is a diff of content and not of
        // a HashMap's mood.
        let mut previous: Option<u32> = None;
        for citation in &self.citations {
            let Some(number) = id_number(&citation.id) else {
                problems.push(format!(
                    "citation id `{}` is not of the form `e<n>`",
                    citation.id
                ));
                continue;
            };
            if let Some(was) = previous {
                let expected = was.saturating_add(1);
                if number != expected {
                    problems.push(format!(
                        "citation ids are not contiguous after `{}`: `{}` jumps to {number}",
                        expected - 1,
                        citation.id
                    ));
                }
            }
            previous = Some(number);
        }
        problems
    }
}

/// The marker that opens the self-check line, so `grep` finds the verdict in an artefact.
#[must_use]
pub fn watermark_marker() -> &'static str {
    GENERATOR
}

/// A heading for a section name, spelled the way the reading order spells it.
#[must_use]
pub fn title_of(section: &str) -> String {
    if section == HEADLINE {
        return "Read this first".to_owned();
    }
    if section == GAP_SECTION {
        return GAP_TITLE.to_owned();
    }
    READING_ORDER
        .iter()
        .find(|(id, _, _)| *id == section)
        .map_or_else(|| section.to_owned(), |(_, title, _)| (*title).to_owned())
}

/// The checks as owned strings, for the payload comparison.
#[must_use]
pub fn expected_checks() -> Vec<String> {
    CHECKS.iter().map(|check| (*check).to_owned()).collect()
}

/// Is this a plan key — `e4.7`, `e3.2`, `spine-1/noisy-path`? A citation that blames a gap on something
/// has to name something a reader can look up in the plan.
#[must_use]
pub fn plan_key(text: &str) -> bool {
    if text.len() < 2 || text.contains(' ') {
        return false;
    }
    let spelled = text
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '/');
    if !spelled {
        return false;
    }
    // `e<major>.<minor>`, a named model rule like `spine-1/noisy-path`, or a beads id like `reco-2rx`.
    let dotted = text.starts_with('e') && text.contains('.');
    let ruled = text.contains('-') && text.chars().any(|ch| ch.is_ascii_digit());
    dotted || ruled
}

/// Is this a locator a reader can act on: a command this binary answers to, or one token naming a place?
///
/// The tight half of the check is the space: `src/parser.rs:240-262` is an address, and `a sentence about
/// the file` is a sentence wearing an address's hat — which is exactly the kind of decoration this module
/// exists to refuse.
#[must_use]
pub fn locator_shaped(text: &str) -> bool {
    if text.is_empty() || text.contains('\n') || text.contains('\t') {
        return false;
    }
    if let Some(rest) = text.strip_prefix("fathom ") {
        return !rest.trim().is_empty();
    }
    !text.contains(' ')
}

/// Strip the citation markers from a sentence so their digits are not read as assertions.
#[must_use]
pub fn strip_citations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        match rest[start..].find(']') {
            Some(end) => rest = &rest[start + end + 1..],
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The digit runs in a string, with space and underscore grouping folded (`9 356` → `9356`).
///
/// Folding is what makes the numeral check fair rather than theatrical: the document prints `2 999 of
/// 3 103` and a sentence is allowed to say 3103 or the difference, provided the citation printed the
/// numbers it did.
#[must_use]
pub fn digit_runs(text: &str) -> std::collections::BTreeSet<String> {
    let mut runs = std::collections::BTreeSet::new();
    let folded = text.replace([' ', '_'], "");
    let mut current = String::new();
    for ch in folded.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current.push(ch);
        } else {
            push_run(&mut runs, &mut current);
        }
    }
    push_run(&mut runs, &mut current);
    runs
}

fn push_run(runs: &mut std::collections::BTreeSet<String>, current: &mut String) {
    if !current.is_empty() {
        // A trailing separator is not part of the number, and `1.` must match a document's `1`.
        let trimmed = current.trim_matches('.').to_owned();
        if !trimmed.is_empty() && trimmed != "." {
            runs.insert(trimmed);
        }
        current.clear();
    }
}

/// The number in a citation id (`e12` → `12`).
#[must_use]
pub fn id_number(id: &str) -> Option<u32> {
    id.strip_prefix('c')?.parse::<u32>().ok()
}

/// Walk a JSON pointer through a document — the same rule [`crate::insight`] verifies its own pointers
/// with, so one broken pointer cannot pass one check and fail the other.
#[must_use]
pub fn resolves(delta: &EpisodeDelta, pointer: &str) -> bool {
    delta.resolves(pointer)
}

/// A short quote of a claim, for a problem line that has to identify the sentence it is complaining about.
fn snippet(text: &str) -> String {
    const MAX: usize = 48;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX {
        return text.to_owned();
    }
    chars[..MAX].iter().collect::<String>() + "…"
}
