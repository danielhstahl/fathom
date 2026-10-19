//! `fathom_tui::empty` — what every pane says when it has no rows (`e5.9`).
//!
//! [`crate::index::Facts`] answers one question — *can this repository be read at all* — and answers it
//! the same way in every pane, which is right for the index axis and wrong for the other axis: nine
//! views go empty for nine different reasons, and "no episodes yet" is a different next action from
//! "nothing selected" is a different next action from "nothing searched for". Before this module the
//! generic fallback printed `` `manifest` — nothing to show yet``, which is the shrug ADR-006 §10.3
//! forbids in prose: it names the pane, says nothing about the cause, and offers no verb or keystroke.
//!
//! So the copy lives here, in one module, in one reviewable batch — the ticket's fourth acceptance
//! criterion is *"copy reviewed once as a batch so the voice is consistent"*, and copy scattered
//! through nine view modules is copy nobody re-read as a whole. Three rules, and `tests/empty_views.rs`
//! holds all three:
//!
//! 1. **every empty state names an action that works from the pane you are standing in** — a keystroke
//!    that is in the merged keymap, a `fathom` verb this binary has, or the ticket that owns what is
//!    missing. A chord no keymap binds and a flag no binary parses are both worse than silence,
//!    because they get tried.
//! 2. **the reason comes before the remedy**, in the pane's own terms: "structure is one episode's
//!    treemap, and no episode is selected" teaches the reviewer something that will still be true after
//!    they press the key.
//! 3. **the index axis wins when the index is the problem.** Every pane is empty on a repository that
//!    has never been indexed, and the one action worth printing there is `fathom index` — not the
//!    per-view chore that would fill an empty list.
//!
//! Nothing here is generated and nothing here is a placeholder: a pane whose content another ticket
//! has not written yet says *who* owns it, which is rule 1's last resort and the honest one.

use crate::index::{Facts, Read};
use crate::views::{Pane, Role, ViewId};

/// What the caller knows about *this* emptiness, beyond the index state.
///
/// Plain data again, for the same reason [`Facts`] is: the shell may not go and look (ADR-002 §5.2), so
/// whatever decided that the pane has no rows hands over the facts that explain it.
#[derive(Debug, Clone, Default)]
pub struct Reason {
    /// The episode the view is showing, if it has one. The secondary panes of `s`, `g`, `p` and `o`
    /// are all views *of a selection*, so this is the difference between "nothing selected" and
    /// "the selection has no manifest delta" — two different actions.
    pub episode: Option<String>,
    /// What the search field holds, for the results pane.
    pub query: Option<String>,
    /// The owner of the read this build does not have, when the caller knows it: the pane's ticket,
    /// which the shell prints verbatim rather than guessing at a cause.
    pub owner: Option<String>,
}

impl Reason {
    /// Nothing is selected and nothing is being looked for: the state a view is in before any input.
    #[must_use]
    pub const fn none() -> Self {
        Reason {
            episode: None,
            query: None,
            owner: None,
        }
    }
}

/// The empty state for one pane, as lines: the reason, then the action, then who owns the rest.
///
/// Two lines for a pane that fits two; the caller clips, so a line is a line and not a wrapped
/// sentence. The `reads` citation is appended by the caller (`App::pane_rows`), never duplicated here.
#[must_use]
pub fn words(view: ViewId, pane: &Pane, facts: &Facts, reason: &Reason) -> Vec<String> {
    // Rule 3: an unreadable index, an unfinished one, and an index that finished with nothing in it are
    // every pane's problem and no pane's chore. Only `fathom doctor` may adjudicate a store, and the
    // sentence that says "the index has nothing to list" is worth more in a treemap than a treemap-shaped
    // instruction, because it is the sentence that explains why the treemap is empty at all.
    if !facts.is_present()
        || facts.episodes.is_some_and(|episodes| episodes == 0)
        || facts.is_building()
    {
        // Short lines, not one sentence: a pane body clips rather than wraps, and the whole point of the
        // index axis's copy is that the remedy survives being read in a 40-column rail. The rail body, which
        // does wrap, prints [`Facts::empty_words`] instead — the same facts in one sentence.
        let mut lines = vec![facts.summary(), facts.remedy()];
        if facts.read == Read::Absent {
            lines.push(
                "or `fathom --demo` reviews a miniature repository that ships inside this binary"
                    .to_owned(),
            );
        }
        if facts.is_building() {
            lines.push(
                "if a rebuild died halfway, `fathom index --rebuild` starts a clean one".to_owned(),
            );
        }
        // …but the view still gets the last word, so the reviewer learns what the pane is *for* while they
        // are being told to go and index: this is the sentence that makes the next empty state
        // self-explanatory.
        if matches!(pane.role, Role::Primary | Role::Secondary)
            && let Some(purpose) = purpose(view, pane)
        {
            lines.push(purpose.to_owned());
        }
        return lines;
    }
    if let Some(purpose) = purpose(view, pane) {
        vec![
            purpose_for(view, pane, reason, purpose),
            action(view, pane, facts, reason),
        ]
    } else {
        vec![facts.summary(), action(view, pane, facts, reason)]
    }
}

/// What this pane shows when it has rows. `None` for the panes whose emptiness needs no preamble.
#[must_use]
fn purpose(view: ViewId, pane: &Pane) -> Option<&'static str> {
    Some(match (view, pane.name) {
        (ViewId::Queue, "worklist") => "the queue is the ranked worklist — riskiest episode first",
        (ViewId::Queue, "card") => "the card is one episode: every figure the pane prints is cited",
        (ViewId::Timeline, "rail") => "the rail is every episode in time order, newest first",
        (ViewId::Diff, "tree") => "the tree is the selected episode's files, riskiest first",
        (ViewId::Diff, "hunks") => "the hunks pane is one file's diff at a time",
        (ViewId::Structure, "treemap") => {
            "the treemap is one episode's directories, sized by churn"
        }
        (ViewId::Structure, "detail") => "the detail pane is the selected directory's share",
        (ViewId::Graph, "canvas") => "the canvas is one episode's blast radius",
        (ViewId::Graph, "detail") => "the detail pane is the selected edge's evidence",
        (ViewId::Deps, "manifest") => "the manifest table is one episode's dependency delta",
        (ViewId::Deps, "tree") => "the tree is that requirement's transitive closure",
        (ViewId::Ambient, "glance") => "ambient is the newest episode, sized for a second terminal",
        (ViewId::Search, "results") => "search covers titles, paths, authors and symbols",
        (ViewId::Artefact, "preview") => {
            "the preview is the artefact a `fathom export` would write"
        }
        (ViewId::Artefact, "scope") => "the scope is the episode range the artefact covers",
        // The timeline card and the search field draw their own emptiness (`card.rs`, `query_line`).
        _ => return None,
    })
}

/// The purpose line, with the reason attached when the purpose alone would be a definition and not an
/// explanation — "the treemap is one episode's treemap" *and* "nothing is selected".
fn purpose_for(view: ViewId, pane: &Pane, reason: &Reason, purpose: &'static str) -> String {
    match (view, pane.name) {
        (ViewId::Search, "results") if reason.query.is_none() => {
            "nothing has been searched for yet".to_owned()
        }
        (ViewId::Search, "results") => format!(
            "nothing matches `{}`",
            reason.query.clone().unwrap_or_default()
        ),
        (ViewId::Diff | ViewId::Structure | ViewId::Graph | ViewId::Deps | ViewId::Artefact, _)
            if reason.episode.is_none() =>
        {
            format!("{purpose} — and no episode is selected")
        }
        _ => purpose.to_owned(),
    }
}

/// The action line: the one thing to do next, from the pane the reviewer is standing in.
fn action(view: ViewId, pane: &Pane, facts: &Facts, reason: &Reason) -> String {
    // Zero episodes over a finished index is not a quiet repository, and it is not this view's
    // problem either — say what the index said and send the reviewer to the verb that can explain it.
    if facts.episodes.is_some_and(|episodes| episodes == 0) || facts.is_building() {
        return facts.remedy();
    }
    match (view, pane.name) {
        (ViewId::Queue, "worklist") => {
            "`t` lists them in time order; `fathom analyze <episode> --json` prints one delta"
                .to_owned()
        }
        (ViewId::Queue, "card") | (ViewId::Timeline, "card") => {
            "`j`/`k` choose an episode, `<Enter>` opens its card".to_owned()
        }
        (ViewId::Timeline, "rail") => {
            "`F` edits the filter that hid them; `fathom stats` says what the index holds".to_owned()
        }
        (ViewId::Diff, "tree") | (ViewId::Diff, "hunks") => {
            "`q` or `t` picks an episode with `j`/`k`, then `d` opens its diff".to_owned()
        }
        (ViewId::Structure, "treemap") | (ViewId::Structure, "detail") => {
            "`q` then `j`/`k` to pick an episode, then `s` again".to_owned()
        }
        (ViewId::Graph, "canvas") => {
            "blast radius: unknown — no graph snapshot; `fathom index` writes one".to_owned()
        }
        (ViewId::Graph, "detail") => "`<Tab>` walks the panes, `j`/`k` move the cursor".to_owned(),
        (ViewId::Deps, "manifest") => {
            "select an episode that touched a manifest — `p` re-reads it, `fathom stats` lists what \
             the index holds"
                .to_owned()
        }
        (ViewId::Deps, "tree") => "`<Enter>` opens the selected requirement's tree".to_owned(),
        (ViewId::Ambient, "glance") => {
            "`q` picks an episode; `fathom watch` keeps this pane current".to_owned()
        }
        (ViewId::Search, "results") => {
            if reason.query.is_some() {
                "`/` starts again — fewer letters find more; `,` lists every command".to_owned()
            } else {
                "`/` searches titles, paths, authors and symbols; `,` is the command palette"
                    .to_owned()
            }
        }
        (ViewId::Artefact, "preview") => {
            "`o` previews the selected episode; the artefact renderer is e5.7's, and `fathom \
             export` says so when asked"
                .to_owned()
        }
        (ViewId::Artefact, "scope") => {
            "`o` builds the scope from the selected episode; `fathom export --episodes HEAD~9..HEAD` \
             sets it headless"
                .to_owned()
        }
        // A pane this module has not met: name the motion that works rather than invent a cause for
        // it. The caller appends the owner line, so whose the pane is stays said exactly once.
        _ => "`j`/`k` move the cursor, `<Tab>` walks the panes".to_owned(),
    }
}
