//! `graph.rs` — walking the commit graph (e1.1 / reco-cey).
//!
//! This is the walk `fathom index` is built on: given some tips, some things to exclude and a shape
//! (first-parent or not), emit commit headers in a stated order — without a `git` subprocess, and
//! without pretending a truncated history is complete.
//!
//! Three decisions are worth stating, because none of them is the obvious one.
//!
//! **The walker is ours, not [`gix`'s rev-walk](gix::Repository::rev_walk).** `gix`'s walker is good,
//! but when a commit object cannot be read it returns an error without naming the object, and the whole
//! point here is that a missing object is a *named, classified state* rather than an opaque failure
//! (ADR-001 §7.3: degrade, never block). Owning the frontier is also what lets a caller choose how much
//! to read per commit ([`Detail`]) at one object read per commit either way.
//!
//! **The commit-graph is not consulted.** `.git/objects/info/commit-graph` caches parent lists and
//! generation numbers; using it would skip object reads only for [`Detail::Ids`] walks, and it is a
//! cache that `git gc` may or may not have refreshed — a second source of truth about parentage, with
//! its own staleness rules, is not worth a partial speedup. If a 100k-commit monorepo says otherwise,
//! it gets added with a measurement in `benchmarks/` to justify it.
//!
//! **Shallow and filter-cloned histories terminate; they do not fail.** `.git/shallow` names the commits
//! git grafts parentless; a filtered clone is missing objects with no such marker. Both stop the walk
//! with a [`Truncation`] and a [`Notice`] naming the boundary. [`WalkOptions::strict`] turns the same
//! condition into `Err(Error::HistoryTruncated)` for callers that would rather fail than show half a
//! history.
//!
//! **Terminology, so `grafts` means one thing in this crate.** *Shallow graft* — the parentless boundary
//! a `--depth` clone creates, recorded in `.git/shallow` and read here through `gix`'s shallow snapshot,
//! which reloads the file when it changes. *Replace ref* — `git replace`, which `gix` applies inside the
//! object database (unless `GIT_NO_REPLACE_OBJECTS`/`objects.noReplace` says otherwise), so rewritten
//! history is what the walk sees, exactly as `git log` sees it. *`info/grafts`* — the pre-1.8.5 file that
//! `git` deleted a decade ago: not read, and if a repository still carries one it is ignored silently,
//! because reading a format `git` itself ignores would make fathom disagree with every other tool.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet, VecDeque};

use fathom_core::time::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{Error, TruncationCause, Unavailable};
use crate::oid::Oid;
use crate::repo::Repository;

/// Who wrote a commit, and when.
///
/// Author and committer differ under rebase and under `git am`, and episode boundaries (`e1.3`) turn on
/// which of the two changed, so both are carried and neither is averaged.
///
/// Serialized because this is a field of [`crate::record::CommitRecord`], and the UTC offset is part of
/// the normalized record the ticket asks for (the store keeps the instant; the offset is in the export).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// The display name, lossy-converted: git stores bytes and names are not always UTF-8.
    pub name: String,
    /// The address, lossy-converted for the same reason.
    pub email: String,
    /// The author/committer's local wall clock, as an absolute instant.
    pub when: Timestamp,
    /// The offset from UTC in minutes at that instant. Kept because "the agent committed at 02:14" is a
    /// sentence about the local clock, and because two commits can be adjacent in `when` and far apart
    /// in local time.
    pub offset_minutes: i32,
}

impl Signature {
    /// `Name <email>` — the form git prints and the form a reviewer expects.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{} <{}>", self.name, self.email)
    }
}

impl std::fmt::Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display())
    }
}

/// What one commit says, minus its diffs.
///
/// Diffs are `e1.2`'s (`reco-76g`): this is the record a walk yields, complete enough to segment an
/// episode and to render a timeline row without a second object read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitHeader {
    /// The commit's own id.
    pub oid: Oid,
    /// The root tree of the snapshot this commit introduces.
    pub tree: Oid,
    /// Parents in the order the commit lists them; the first is the first parent.
    pub parents: Vec<Oid>,
    /// Who wrote the change.
    pub author: Signature,
    /// Who applied it, which is a different person after a rebase.
    pub committer: Signature,
    /// The first line of the message, lossy-converted. Empty when the message is empty.
    pub subject: String,
    /// The whole message, raw bytes: trailers are parsed by `e1.3` and a lossy copy cannot be re-parsed.
    pub message: Vec<u8>,
    /// Names of extra headers present (`gpgsig`, `mergetag`, …), so a caller can say "this is signed"
    /// without this crate deciding what a signature means.
    pub extra_header_names: Vec<String>,
}

impl CommitHeader {
    /// A merge (more than one parent).
    #[must_use]
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// The commit date: the committer's time, which is what `git log` orders by by default.
    #[must_use]
    pub fn when(&self) -> Timestamp {
        self.committer.when
    }

    /// The value of a message trailer; the last occurrence wins, which is git's rule.
    ///
    /// Here rather than in `e1.3` because the trailer grammar is a dozen lines and the segmentation
    /// rules, a `--trailer` CLI filter and the hook-event writer all need the same reading of it —
    /// including the rule that a trailer block is a *paragraph*, separated from what precedes it by a
    /// blank line. A one-paragraph message is a subject, and a `fix: …` subject is shaped exactly like
    /// a trailer, so the loose reading invented one for every conventional commit (`reco-2ss`).
    #[must_use]
    pub fn trailer(&self, key: &str) -> Option<String> {
        let message = String::from_utf8_lossy(&self.message);
        let mut lines: Vec<&str> = message.lines().collect();
        while matches!(lines.last(), Some(line) if line.trim().is_empty()) {
            lines.pop();
        }
        // No blank line, no trailer block: the whole message is the subject.
        let start = lines.iter().rposition(|line| line.trim().is_empty())?;
        let mut found: Option<String> = None;
        for line in lines[start + 1..].iter().rev() {
            let line = line.trim();
            let Some((name, value)) = line.split_once(':') else {
                break;
            };
            if name.trim().eq_ignore_ascii_case(key) {
                found = Some(value.trim().to_owned());
            } else {
                break;
            }
        }
        found
    }
}

/// How much to read per commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Detail {
    /// Id, parents and commit time only. The streaming case: a store writer that decodes each commit
    /// itself (and in parallel, `e1.2`) does not need a header decoded twice.
    Ids,
    /// Id, parents and the decoded header: tree, author, committer, subject and message.
    #[default]
    Headers,
}

/// The order a walk emits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WalkOrder {
    /// Depth-first, first parent first: a merge's mainline appears before its side branches, and memory
    /// stays flat however wide the history is. The cheapest order, and the default.
    ///
    /// It does **not** promise that a parent is emitted after all of its children, and it cannot: a commit
    /// reachable by two paths can be reached — and so emitted — through the first one before the second
    /// path is walked, so `main~1` can be emitted before a `feature` commit that has it as a parent.
    /// Following the first-parent chain to the root before any side branch is exactly what makes this
    /// order cheap and readable, and the property it gives up is only obtainable by reading the whole
    /// reachable set first (which is why git's own `--topo-order` is documented as the expensive one and
    /// why `git log --date-order` can do the same thing). A caller that needs "every parent after every
    /// child" must read the full set — [`WalkOrder::DateNewestFirst`] over a history with normally-dated
    /// commits has it, and [`crate::graph::CommitLog::finish`] returns the whole walk when a caller wants
    /// to topologically sort what it has.
    #[default]
    AncestryFirst,
    /// Newest commit first among the commits reachable so far — `git log --date-order`'s rule, streamed.
    DateNewestFirst,
    /// Oldest first, streamed in reverse: same frontier as [`WalkOrder::DateNewestFirst`], emitted in
    /// reverse. Needs the whole reachable set in memory, so it is the one order that is not flat; say so
    /// in any UI that offers it.
    DateOldestFirst,
}

/// Which merges to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeFilter {
    /// Every commit.
    #[default]
    All,
    /// No merges — `git log --no-merges`.
    NoMerges,
    /// Only merges — the shape of a merge-only timeline, and of `git log --merges`.
    OnlyMerges,
}

/// What to do when a commit object cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingPolicy {
    /// Record a [`Notice`] and keep going; the walk ends with a [`Truncation`] if anything was skipped.
    /// This is the product behaviour: a shallow clone still shows the commits it has.
    #[default]
    Degrade,
    /// Stop at the first commit that cannot be read, with the reason the read failed: a shallow graft
    /// surfaces as [`Error::HistoryTruncated`] when the walk reached a graft (a structural limit), and a
    /// damaged repository as [`Error::ObjectUnavailable`] naming the object, because those two want
    /// different remedies and a caller cannot tell them apart afterwards. For a caller that must not
    /// present a partial history as a full one.
    Fail,
}

/// Everything a walk needs to know, in one value, so a CLI flag set and a stored query are the same
/// thing.
#[derive(Debug, Clone, Default)]
pub struct WalkOptions {
    /// Where to start. Empty means `HEAD`; an unborn `HEAD` yields an empty walk with a notice.
    pub starts: Vec<Oid>,
    /// Tips not to cross and not to emit — `git log A ^B`.
    pub hides: Vec<Oid>,
    /// Follow only the first parent: `git log --first-parent`.
    pub first_parent: bool,
    /// The emit order.
    pub order: WalkOrder,
    /// Emit at most this many entries (after `skip`).
    pub limit: Option<usize>,
    /// Drop this many entries before emitting — `git log --skip`, which is pagination, not filtering.
    pub skip: usize,
    /// Do not emit commits committer-dated before this instant.
    pub since: Option<Timestamp>,
    /// Do not emit commits committer-dated after this instant.
    pub until: Option<Timestamp>,
    /// Which merges to emit.
    pub merges: MergeFilter,
    /// How much to read per commit.
    pub detail: Detail,
    /// What to do about unreadable commits.
    pub on_missing: MissingPolicy,
}

impl WalkOptions {
    /// A walk from `HEAD` with no options at all — the shape almost every caller starts from.
    #[must_use]
    pub fn head() -> Self {
        Self::default()
    }

    /// Start at these commits.
    #[must_use]
    pub fn starting_at(mut self, starts: impl IntoIterator<Item = Oid>) -> Self {
        self.starts = starts.into_iter().collect();
        self
    }

    /// Do not cross (or emit) these commits.
    #[must_use]
    pub fn hiding(mut self, hides: impl IntoIterator<Item = Oid>) -> Self {
        self.hides = hides.into_iter().collect();
        self
    }

    /// Follow only the first parent.
    #[must_use]
    pub fn first_parent(mut self) -> Self {
        self.first_parent = true;
        self
    }

    /// Emit in this order; see [`WalkOrder`] for what each order promises and costs.
    #[must_use]
    pub fn order(mut self, order: WalkOrder) -> Self {
        self.order = order;
        self
    }

    /// Which merges to emit — `git log --no-merges` and `git log --merges`.
    #[must_use]
    pub fn merges(mut self, merges: MergeFilter) -> Self {
        self.merges = merges;
        self
    }

    /// Do not emit commits committer-dated before `when`.
    #[must_use]
    pub fn since(mut self, when: fathom_core::time::Timestamp) -> Self {
        self.since = Some(when);
        self
    }

    /// Do not emit commits committer-dated after `when`.
    #[must_use]
    pub fn until(mut self, when: fathom_core::time::Timestamp) -> Self {
        self.until = Some(when);
        self
    }

    /// Drop this many entries before emitting — pagination, not filtering.
    #[must_use]
    pub fn skip(mut self, count: usize) -> Self {
        self.skip = count;
        self
    }

    /// Emit at most `count` entries.
    #[must_use]
    pub fn limit(mut self, count: usize) -> Self {
        self.limit = Some(count);
        self
    }

    /// Error instead of notice when a commit is unreadable.
    #[must_use]
    pub fn strict(mut self) -> Self {
        self.on_missing = MissingPolicy::Fail;
        self
    }

    /// Read ids and times only.
    #[must_use]
    pub fn ids_only(mut self) -> Self {
        self.detail = Detail::Ids;
        self
    }
}

/// Something the caller should know about a walk's result.
///
/// A notice is never an error: the walk still produced what it could. It is what keeps "degrade, never
/// block" from becoming "silently lie".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The walk stopped at a shallow graft: nothing older than this is present, by design.
    ShallowBoundary {
        /// The commit git grafted parentless.
        at: Oid,
    },
    /// A commit could not be read, and was skipped.
    ObjectUnavailable {
        /// The commit that was skipped.
        oid: Oid,
        /// Why it is missing, as far as the repository can say.
        why: Unavailable,
    },
    /// A start point was not a commit — a blob, or a tree, or a tag that does not peel to a commit.
    NotACommit {
        /// What was given.
        oid: Oid,
    },
    /// There was nothing to walk: `HEAD` points at a branch with no commits yet.
    UnbornHead {
        /// The branch name, without `refs/heads/`.
        branch: String,
    },
    /// The walk stopped because it emitted as much as was asked for.
    LimitReached {
        /// How many were emitted.
        emitted: usize,
    },
}

/// Why a walk did not reach the root of history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Truncation {
    /// The history is shallow: it was cloned with `--depth` and grafted at these commits.
    Shallow {
        /// The grafts hit, in the order they were hit.
        boundary: Vec<Oid>,
    },
    /// Objects were missing and skipped, with no shallow marker to explain it.
    Unreadable {
        /// How many commits were skipped.
        skipped: usize,
        /// The first few of them — enough to name the problem without a wall of output.
        examples: Vec<Oid>,
        /// The clone filter, if the repository recorded one.
        filter: Option<String>,
    },
    /// Both: shallow grafts and missing objects.
    ShallowAndUnreadable {
        /// The grafts hit.
        boundary: Vec<Oid>,
        /// How many commits were skipped.
        skipped: usize,
    },
}

impl Truncation {
    /// The same fact in the error vocabulary, for `strict` walks and for `--json` output.
    #[must_use]
    pub fn cause(&self) -> TruncationCause {
        match self {
            Truncation::Shallow { boundary } => TruncationCause::Shallow {
                boundary: boundary.clone(),
            },
            Truncation::Unreadable {
                skipped, filter, ..
            } => TruncationCause::Filtered {
                unreadable: *skipped,
                filter: filter.clone(),
            },
            Truncation::ShallowAndUnreadable { boundary, skipped } => {
                if *skipped == 0 {
                    TruncationCause::Shallow {
                        boundary: boundary.clone(),
                    }
                } else {
                    TruncationCause::Filtered {
                        unreadable: *skipped,
                        filter: None,
                    }
                }
            }
        }
    }

    /// The commits that explain it, for an error message or a JSON field.
    #[must_use]
    pub fn boundary(&self) -> Vec<Oid> {
        match self {
            Truncation::Shallow { boundary } => boundary.clone(),
            Truncation::Unreadable { examples, .. } => examples.clone(),
            Truncation::ShallowAndUnreadable { boundary, .. } => boundary.clone(),
        }
    }

    /// The suggested remedy.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        self.cause().hint()
    }
}

/// One commit as the walk emits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// The commit.
    pub oid: Oid,
    /// Its parents, in the commit's own order — a first-parent walk lists at most one.
    pub parents: Vec<Oid>,
    /// How many parent steps the walk took to reach it. Useful for a graph lane, and it makes a
    /// `--since` cut-off debuggable.
    pub depth: usize,
    /// The decoded header, unless [`Detail::Ids`] was asked for.
    pub detail: Option<CommitHeader>,
}

/// A walk, as an iterator that also remembers what it noticed.
///
/// Iterating gives `Result<LogEntry, Error>`; after the iterator is exhausted, [`notices`](Self::notices)
/// and [`truncation`](Self::truncation) say what the entries do not.
pub struct CommitLog<'r> {
    repo: &'r Repository,
    options: WalkOptions,
    state: State,
    notices: Vec<Notice>,
    truncation: Option<Truncation>,
    failure: Option<Error>,
    emitted: usize,
    dropped: usize,
    seen: HashSet<Oid>,
    hidden: HashSet<Oid>,
    shallow: HashSet<Oid>,
    stack: VecDeque<Pending>,
    heap: BinaryHeap<HeapPending>,
    /// The buffered frontier for [`WalkOrder::DateOldestFirst`]: pulled newest-first, emitted
    /// back-to-front. This is the one order that is not memory-flat, and the field is the reason the doc
    /// says so.
    reversed: VecDeque<Pending>,
    /// Has the oldest-first order finished buffering its frontier?
    buffered: bool,
    shallow_hits: Vec<Oid>,
    skipped_oids: Vec<Oid>,
}

/// A commit read from the object database and queued for emitting.
struct Pending {
    oid: Oid,
    parents: Vec<Oid>,
    depth: usize,
    when: Timestamp,
    detail: Option<CommitHeader>,
}

/// [`Pending`] wrapped so a max-heap can order it.
struct HeapPending {
    key: std::cmp::Reverse<i64>,
    pending: Pending,
}

impl PartialEq for HeapPending {
    fn eq(&self, other: &Self) -> bool {
        self.pending.oid == other.pending.oid
    }
}

impl Eq for HeapPending {}

impl PartialOrd for HeapPending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPending {
    fn cmp(&self, other: &Self) -> Ordering {
        // Newest-first in both orders: for `DateOldestFirst` the whole frontier is buffered and
        // reversed at the end, which keeps the tie-breaking (below) identical between the two.
        self.key
            .cmp(&other.key)
            // Two commits with the same commit time is the common case for an agent that commits in a
            // loop, so the tie-break has to be total and stable: id ascending.
            .then_with(|| other.pending.oid.cmp(&self.pending.oid))
    }
}

#[derive(PartialEq, Eq)]
enum State {
    Idle,
    Running,
    Finished,
}

impl<'r> CommitLog<'r> {
    pub(crate) fn new(repo: &'r Repository, options: WalkOptions) -> Self {
        CommitLog {
            repo,
            options,
            state: State::Idle,
            notices: Vec::new(),
            truncation: None,
            failure: None,
            emitted: 0,
            dropped: 0,
            seen: HashSet::new(),
            hidden: HashSet::new(),
            shallow: HashSet::new(),
            stack: VecDeque::new(),
            heap: BinaryHeap::new(),
            reversed: VecDeque::new(),
            buffered: false,
            shallow_hits: Vec::new(),
            skipped_oids: Vec::new(),
        }
    }

    /// What the walk noticed. Empty means the history was complete and walked as asked.
    #[must_use]
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// Why the walk did not reach the root of history, if it did not.
    #[must_use]
    pub fn truncation(&self) -> Option<&Truncation> {
        self.truncation.as_ref()
    }

    /// How many entries have been emitted so far.
    #[must_use]
    pub fn emitted(&self) -> usize {
        self.emitted
    }

    /// Run the walk to the end and fold its result into one value.
    ///
    /// With [`MissingPolicy::Fail`] a truncated history is an error here; that is the whole point of the
    /// policy, so a caller that must not present half a history as a whole one has one place to check.
    pub fn finish(mut self) -> Result<WalkOutcome, Error> {
        let mut entries = Vec::new();
        for entry in self.by_ref() {
            entries.push(entry?);
        }
        let truncation = self.truncation.clone();
        if self.options.on_missing == MissingPolicy::Fail
            && let Some(truncation) = truncation.clone()
        {
            return Err(Error::HistoryTruncated {
                boundary: truncation.boundary(),
                cause: truncation.cause(),
            });
        }
        Ok(WalkOutcome {
            entries,
            notices: std::mem::take(&mut self.notices),
            truncation,
        })
    }

    fn begin(&mut self) {
        self.state = State::Running;
        self.hidden = self.options.hides.iter().copied().collect();
        // A hidden tip is marked seen so that reaching it stops the walk there; it is never queued, so
        // it is never emitted. That is `git log A ^B`, not `--boundary`.
        for oid in &self.options.hides {
            self.seen.insert(*oid);
        }
        if let Ok(Some(commits)) = self.repo.shallow_boundary() {
            self.shallow = commits.into_iter().collect();
        }
        let starts: Vec<Oid> = if self.options.starts.is_empty() {
            match self.repo.head() {
                Ok(head) => match head.oid() {
                    Some(oid) => vec![oid],
                    None => {
                        self.notices.push(Notice::UnbornHead {
                            branch: head.branch_name().unwrap_or("HEAD").to_owned(),
                        });
                        Vec::new()
                    }
                },
                Err(err) => {
                    self.fail(err);
                    return;
                }
            }
        } else {
            self.options.starts.clone()
        };
        // Reverse so the first start point is the first one emitted.
        for oid in starts.into_iter().rev() {
            self.push(oid, 0);
        }
    }

    fn fail(&mut self, err: Error) {
        self.state = State::Finished;
        if self.failure.is_none() {
            self.failure = Some(err);
        }
    }

    /// Read one commit and queue it, or record why it could not be.
    fn push(&mut self, oid: Oid, depth: usize) {
        if !self.seen.insert(oid) {
            return;
        }
        if self.hidden.contains(&oid) {
            return;
        }
        match self.repo.read_commit(oid, self.options.detail) {
            Ok(Some(ready)) => {
                let pending = Pending {
                    oid: ready.oid,
                    parents: ready.parents,
                    depth,
                    when: ready.when,
                    detail: ready.detail,
                };
                match self.options.order {
                    // A LIFO stack, so the first parent — pushed last — is the next thing emitted:
                    // depth-first, mainline-first, and flat in memory however wide the history is.
                    WalkOrder::AncestryFirst => self.stack.push_back(pending),
                    WalkOrder::DateNewestFirst | WalkOrder::DateOldestFirst => {
                        self.heap.push(HeapPending {
                            key: std::cmp::Reverse(-ready.when.as_unix_micros()),
                            pending,
                        })
                    }
                }
            }
            Ok(None) => {
                self.notices.push(Notice::NotACommit { oid });
            }
            Err(why) => match why {
                crate::error::Error::ObjectUnavailable { oid, why } => {
                    if self.skipped_oids.len() < 8 {
                        self.skipped_oids.push(oid);
                    }
                    self.notices.push(Notice::ObjectUnavailable {
                        oid,
                        why: why.clone(),
                    });
                    if self.options.on_missing == MissingPolicy::Fail {
                        self.fail(Error::ObjectUnavailable { oid, why });
                    }
                }
                other => self.fail(other),
            },
        }
    }

    fn take_next(&mut self) -> Option<Pending> {
        match self.options.order {
            WalkOrder::AncestryFirst => self.stack.pop_back(),
            WalkOrder::DateNewestFirst => self.heap.pop().map(|item| item.pending),
            WalkOrder::DateOldestFirst => {
                if !self.buffered {
                    // Drain the whole frontier newest-first into a buffer, then emit it back-to-front.
                    // That is what makes the two date orders agree on the set and its tie-breaks, and
                    // it is why this order is documented as needing the reachable set in memory.
                    self.buffered = true;
                    while let Some(item) = self.heap.pop() {
                        let pending = item.pending;
                        self.queue_parents(&pending);
                        self.reversed.push_back(pending);
                        if self.state == State::Finished {
                            break;
                        }
                    }
                }
                self.reversed.pop_back()
            }
        }
    }

    fn queue_parents(&mut self, pending: &Pending) {
        // Beyond a shallow graft there is nothing to read: the graft commit names parents that the
        // clone never received, so asking for them produces a spurious "missing object" for what is a
        // documented absence. `git` treats a shallow commit as parentless, and so does this walk —
        // which is also what keeps a shallow walk's truncation `Shallow` rather than "unreadable".
        if self.shallow.contains(&pending.oid) {
            return;
        }
        let parents: &[Oid] = if self.options.first_parent {
            &pending.parents[..pending.parents.len().min(1)]
        } else {
            &pending.parents
        };
        // Pushed in reverse onto a LIFO stack, so the first parent is the next one emitted.
        for oid in parents.iter().rev() {
            self.push(*oid, pending.depth + 1);
        }
    }

    fn passes(&self, pending: &Pending) -> bool {
        match self.options.merges {
            MergeFilter::All => {}
            MergeFilter::NoMerges if pending.parents.len() > 1 => return false,
            MergeFilter::OnlyMerges if pending.parents.len() <= 1 => return false,
            MergeFilter::NoMerges | MergeFilter::OnlyMerges => {}
        }
        if let Some(since) = self.options.since
            && pending.when < since
        {
            return false;
        }
        if let Some(until) = self.options.until
            && pending.when > until
        {
            return false;
        }
        true
    }

    fn finish_up(&mut self) {
        self.state = State::Finished;
        let shallow = std::mem::take(&mut self.shallow_hits);
        let skipped = self.skipped_oids.len();
        let examples = std::mem::take(&mut self.skipped_oids);
        if !shallow.is_empty() && !examples.is_empty() {
            self.truncation = Some(Truncation::ShallowAndUnreadable {
                boundary: shallow,
                skipped,
            });
        } else if !shallow.is_empty() {
            self.truncation = Some(Truncation::Shallow { boundary: shallow });
        } else if !examples.is_empty() {
            self.truncation = Some(Truncation::Unreadable {
                skipped,
                examples,
                filter: self.repo.partial_clone().filter.clone(),
            });
        }
    }
}

impl Iterator for CommitLog<'_> {
    type Item = Result<LogEntry, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(err) = self.failure.take() {
            self.state = State::Finished;
            return Some(Err(err));
        }
        if self.state == State::Idle {
            self.begin();
        }
        if self.state == State::Finished {
            return None;
        }
        loop {
            let Some(pending) = self.take_next() else {
                self.finish_up();
                return None;
            };
            // Parents are queued before the emit decision, so `--since`/`--no-merges` filter what is
            // *shown* without also pruning what is *reachable* — the same asymmetry git has.
            self.queue_parents(&pending);
            if self.shallow.contains(&pending.oid) {
                self.shallow_hits.push(pending.oid);
                self.notices
                    .push(Notice::ShallowBoundary { at: pending.oid });
            }
            if !self.passes(&pending) {
                continue;
            }
            if self.dropped < self.options.skip {
                self.dropped += 1;
                continue;
            }
            if let Some(limit) = self.options.limit
                && self.emitted >= limit
            {
                self.notices.push(Notice::LimitReached {
                    emitted: self.emitted,
                });
                self.finish_up();
                return None;
            }
            self.emitted += 1;
            return Some(Ok(LogEntry {
                oid: pending.oid,
                parents: pending.parents,
                depth: pending.depth,
                detail: pending.detail,
            }));
        }
    }
}

/// The result of a walk that ran to completion.
#[derive(Debug, Clone)]
pub struct WalkOutcome {
    /// The entries, in the requested order.
    pub entries: Vec<LogEntry>,
    /// What the walk noticed.
    pub notices: Vec<Notice>,
    /// Why history did not reach its root, if it did not.
    pub truncation: Option<Truncation>,
}

impl WalkOutcome {
    /// Did history end early for a structural reason (shallow, filtered)?
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.truncation.is_some()
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is the walk empty? (An empty walk is not an error: an unborn `HEAD` is a real repository state.)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
