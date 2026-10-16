//! The timeline fixture corpus — the rows `e1.6`'s acceptance criteria name, as data.
//!
//! Three corpora, all deterministic (ADR-002 §4.4: a fixture that reads a clock or an RNG is a fixture
//! with a flake in it):
//!
//! * [`mock`] is ADR-006 §5.2's own picture, transcribed: three UTC days, two author kinds, an unknown
//!   author, and every review state the marker column can carry. It is the row-for-row check that what
//!   the document promises is what the rail draws.
//! * [`generated`] is the 10 000-episode index the perf criterion talks about, spread over a year with
//!   realistic skew: a few big agent turns, many small ones, a mix of paths, sessions and states. A
//!   deterministic xorshift, because "p99 under 16 ms" measured over rows that are all identical is
//!   not a measurement.
//! * [`rebased`], [`reworded`], [`squashed`] and [`without`] are the *rewrites*: the same content with
//!   new member SHAs, a reword, a squash, and a deletion — the four things ADR-003 §6's relink tiers
//!   exist for, which is where "selection survives re-index and re-attaches after a rebase" is
//!   actually decided.
//!
//! Shared by `timeline_behaviour.rs`, `timeline_golden.rs` and `timeline_bench.rs` through this one
//! module, so a fixture cannot be tuned to make one of them pass.

#![allow(dead_code)] // each test binary uses part of it

use fathom_core::time::Timestamp;
use fathom_tui::timeline::{AuthorKind, Episode, ReviewState};

/// The instant every timeline fixture is drawn for: Tue 14 Oct 2025, 12:00 UTC. Fixed, not `now`.
pub const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
/// One minute in microseconds.
pub const MINUTE: i64 = 60_000_000;
/// One hour.
pub const HOUR: i64 = 60 * MINUTE;
/// One day.
pub const DAY: i64 = 24 * HOUR;

/// How many episodes the perf fixture holds — the ticket's own number.
pub const BIG: usize = 10_000;

/// ADR-006 §5.2's picture, row for row.
#[must_use]
pub fn mock() -> Vec<Episode> {
    let mut rows = vec![
        row(
            "e8f1a2",
            "make the store epoch-safe",
            12 * MINUTE,
            AuthorKind::Agent,
            "pi",
            412,
            90,
            12,
            ReviewState::Unread,
            Some("4"),
            Some("12"),
        ),
        row(
            "9c4d10",
            "cache the epoch probe",
            41 * MINUTE,
            AuthorKind::Agent,
            "pi",
            58,
            12,
            4,
            ReviewState::Unread,
            Some("4"),
            Some("11"),
        ),
        row(
            "2be77f",
            "typo in the epoch docs",
            67 * MINUTE,
            AuthorKind::Human,
            "stahl",
            8,
            3,
            1,
            ReviewState::Read,
            None,
            None,
        ),
        row(
            "77aa0c",
            "rewrite the walk",
            25 * HOUR,
            AuthorKind::Agent,
            "pi",
            1_204,
            90,
            21,
            ReviewState::Accepted,
            Some("3"),
            Some("9"),
        ),
        row(
            "40c9b3",
            "add the schema stamp",
            28 * HOUR,
            AuthorKind::Agent,
            "pi",
            96,
            0,
            3,
            ReviewState::Unread,
            Some("3"),
            Some("8"),
        ),
        row(
            "c1f5e2",
            "unknown author, unknown why",
            32 * HOUR,
            AuthorKind::Unknown,
            "",
            22,
            22,
            2,
            ReviewState::Unread,
            None,
            None,
        ),
        row(
            "08d4b1",
            "clippy lint fix",
            26 * DAY,
            AuthorKind::Agent,
            "pi",
            3,
            1,
            1,
            ReviewState::Accepted,
            Some("2"),
            Some("4"),
        ),
        row(
            "5f2c99",
            "revert the revert",
            30 * DAY,
            AuthorKind::Agent,
            "pi",
            61,
            61,
            2,
            ReviewState::Dismissed,
            Some("1"),
            Some("2"),
        ),
        row(
            "a77bb0",
            "flagged: the migration",
            31 * DAY,
            AuthorKind::Human,
            "stahl",
            140,
            12,
            5,
            ReviewState::Flagged,
            None,
            None,
        ),
        row(
            "b31c00",
            "snoozed until the freeze lifts",
            33 * DAY,
            AuthorKind::Agent,
            "pi",
            18,
            6,
            2,
            ReviewState::Snoozed,
            Some("1"),
            Some("1"),
        ),
    ];
    // The digests the relink tiers read, shaped the way the store shapes them: the fallback is
    // author+subject, the shape is author+paths. Real digests are sha256; the rail only ever compares
    // them for equality, so a readable stand-in is the better fixture — a golden file whose identity
    // column is `sha256(…)` cannot be read by the person debugging it.
    for episode in &mut rows {
        episode.fingerprint = format!("fp:{}", episode.id);
        episode.fallback_fingerprint = format!("fb:{}:{}", episode.author_name, episode.summary);
        episode.shape_fingerprint =
            format!("sh:{}:{}", episode.author_name, episode.paths.join(","));
        episode.members = (0..(1 + episode.n_files / 4).max(1))
            .map(|i| format!("{}:{i}", &episode.id[..4]))
            .collect();
        episode.boundary_rule = Some("author".to_owned());
        episode.boundary_confidence = Some(0.75);
    }
    rows
}

/// One mock row, with the fields a real index would fill.
#[allow(clippy::too_many_arguments)]
#[must_use]
#[allow(clippy::similar_names)]
fn row(
    id: &str,
    summary: &str,
    ago: i64,
    author: AuthorKind,
    author_name: &str,
    add: u64,
    del: u64,
    files: u64,
    state: ReviewState,
    session: Option<&str>,
    turn: Option<&str>,
) -> Episode {
    let created = Timestamp::from_unix_micros(NOW.as_unix_micros() - ago);
    let mut episode = Episode::new(id, created, summary);
    episode.last = created;
    episode.author = author;
    episode.author_name = author_name.to_owned();
    episode.author_confidence = if author == AuthorKind::Unknown {
        None
    } else {
        Some(0.94)
    };
    episode.n_add = add;
    episode.n_del = del;
    episode.n_files = files;
    episode.state = state;
    episode.session = session.map(str::to_owned);
    episode.turn = turn.map(str::to_owned);
    // The file names carry the row's id so the shape digest (author + paths, ADR-003 §6.2) is unique
    // per row, the way two real episodes' path sets nearly always are. A corpus whose digests collide
    // would let a tier-2 relink pass for the wrong reason.
    let mut paths = vec![format!("src/store/epoch-{id}.rs")];
    for index in 1..files.min(9) {
        paths.push(format!("src/store/part{index}-{id}.rs"));
    }
    if files > 1 {
        paths.push(format!("tests/test_epoch-{id}.rs"));
    }
    episode.paths = paths;
    episode
}

/// A deterministic xorshift, so the big fixture is the same 10 000 rows on every machine and run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// The authors the big fixture spreads itself across, including the `unknown` the badge column exists
/// for.
const AUTHORS: [(&str, AuthorKind); 5] = [
    ("pi", AuthorKind::Agent),
    ("cora", AuthorKind::Agent),
    ("stahl", AuthorKind::Human),
    ("dana", AuthorKind::Human),
    ("", AuthorKind::Unknown),
];
/// Path areas, so the area filter has something to select.
const AREAS: [&str; 8] = [
    "src/store",
    "src/core",
    "src/tui",
    "crates/fathom-store",
    "tests",
    "docs",
    "tools",
    "schemas",
];
/// Subjects, repeated with a counter so the digest fixtures can name one.
const SUBJECTS: [&str; 6] = [
    "make the store epoch-safe",
    "cache the probe",
    "typo in the docs",
    "rewrite the walk",
    "add the schema stamp",
    "revert the revert",
];

/// The 10 000-episode index: newest first, spread over a year with the skew a real agent repository
/// has (many small commits, a few enormous agent turns).
#[must_use]
pub fn generated() -> Vec<Episode> {
    generated_from(BIG)
}

/// The same corpus at any size, so a constant-memory test can compare two sizes.
#[must_use]
pub fn generated_from(count: usize) -> Vec<Episode> {
    let mut rng = Lcg(0x2545_F491_4F6C_DD1D);
    let states = [
        ReviewState::Unread,
        ReviewState::Unread,
        ReviewState::Unread,
        ReviewState::Read,
        ReviewState::Accepted,
        ReviewState::Flagged,
        ReviewState::Dismissed,
        ReviewState::Snoozed,
    ];
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let index64 = u64::try_from(index).unwrap_or(u64::MAX);
        // Half an hour per row, so 10 000 of them cover about seven months of days rather than the
        // fifteen years a rounder number would have produced: a year's worth of group headers is what
        // a real index of this size holds.
        let step = u64::try_from(HOUR / 2).unwrap_or(1);
        let jitter = u64::try_from(HOUR / 8).unwrap_or(1);
        let ago = i64::try_from(index64 * step + rng.below(jitter)).unwrap_or(i64::MAX);
        let created = Timestamp::from_unix_micros(NOW.as_unix_micros() - ago);
        let (name, kind) = AUTHORS[usize::try_from(rng.below(5)).unwrap_or(0)];
        let big = rng.below(50) == 0;
        let (add, del) = if big {
            (1_000 + rng.below(4_000), 200 + rng.below(1_000))
        } else {
            (1 + rng.below(120), rng.below(40))
        };
        let files = 1 + rng.below(14);
        let area = AREAS[usize::try_from(rng.below(8)).unwrap_or(0)];
        let mut episode = Episode::new(
            format!("{index64:04x}e{:02x}", rng.below(256)),
            created,
            format!(
                "{} #{index}",
                SUBJECTS[usize::try_from(rng.below(6)).unwrap_or(0)]
            ),
        );
        episode.author = kind;
        episode.author_name = name.to_owned();
        episode.author_confidence = if kind == AuthorKind::Unknown {
            None
        } else {
            Some(0.9)
        };
        episode.n_add = add;
        episode.n_del = del;
        episode.n_files = files;
        episode.n_binary = if rng.below(40) == 0 { 1 } else { 0 };
        episode.state = states[usize::try_from(rng.below(8)).unwrap_or(0)];
        episode.session = (kind != AuthorKind::Human).then(|| format!("{}", 1 + rng.below(400)));
        episode.turn = episode.session.clone();
        episode.paths = (0..files).map(|i| format!("{area}/file{i}.rs")).collect();
        episode.members = (0..1 + rng.below(4))
            .map(|i| format!("{:07x}m{i}", rng.below(0x0fff_ffff)))
            .collect();
        episode.fingerprint = format!("fp{index64:05x}");
        episode.fallback_fingerprint = format!("fb:{name}:{}", episode.summary);
        episode.shape_fingerprint = format!("sh:{name}:{}", episode.paths.join(","));
        episode.boundary_rule = Some("time-gap".to_owned());
        episode.boundary_confidence = Some(0.6);
        episode.live = true;
        out.push(episode);
    }
    out
}

/// The same episodes after `git rebase -i` of the same content (ADR-003 I1): every member gets a new
/// SHA, the digests hold, and the dates move — so the row *order* changes under the cursor while the
/// identity does not.
#[must_use]
pub fn rebased(rows: &[Episode]) -> Vec<Episode> {
    rows.iter()
        .enumerate()
        .map(|(index, episode)| {
            let mut next = episode.clone();
            next.members = episode
                .members
                .iter()
                .enumerate()
                .map(|(i, _)| format!("new{index}{i}"))
                .collect();
            next.created = Timestamp::from_unix_micros(
                episode.created.as_unix_micros() - DAY / 3 + i64::try_from(index).unwrap_or(0),
            );
            next
        })
        .collect()
}

/// A reword: the id and the author+subject digest move, the author+paths digest does not (ADR-003
/// §6.2 tier 2).
#[must_use]
pub fn reworded(rows: &[Episode], id: &str, new_subject: &str) -> Vec<Episode> {
    rows.iter()
        .map(|episode| {
            if episode.id != id {
                return episode.clone();
            }
            let mut next = episode.clone();
            next.id = format!("rw{id}");
            next.summary = new_subject.to_owned();
            next.fingerprint = format!("fp:rw{id}");
            next.fallback_fingerprint = format!("fb:{}:{new_subject}", episode.author_name);
            next
        })
        .collect()
}

/// A squash of the two newest episodes into one that keeps *neither* digest but keeps a member commit
/// of the first (tier 3).
#[must_use]
pub fn squashed(rows: &[Episode]) -> Vec<Episode> {
    let mut out: Vec<Episode> = Vec::with_capacity(rows.len());
    let mut iterator = rows.iter();
    let mut first = true;
    while let Some(episode) = iterator.next() {
        if first {
            first = false;
            let mut merged = episode.clone();
            merged.id = "squash".to_owned();
            merged.summary = "squash the pair".to_owned();
            merged.fingerprint = "fp:squash".to_owned();
            merged.fallback_fingerprint = "fb:squashed:pair".to_owned();
            merged.shape_fingerprint = "sh:squashed:none".to_owned();
            merged.members = vec![
                episode
                    .members
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "m0".to_owned()),
            ];
            if let Some(next) = iterator.next() {
                merged.n_add += next.n_add;
                merged.n_del += next.n_del;
            }
            out.push(merged);
            continue;
        }
        out.push(episode.clone());
    }
    out
}

/// The rows minus one episode — the case where the reviewer's selection left the index, which must be
/// said out loud rather than shown as a cursor on somebody else's row.
#[must_use]
pub fn without(rows: &[Episode], id: &str) -> Vec<Episode> {
    rows.iter()
        .filter(|episode| episode.id != id)
        .cloned()
        .collect()
}
