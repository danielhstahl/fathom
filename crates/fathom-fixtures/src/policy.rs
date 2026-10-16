//! The determinism policy every fixture is built under (ADR-002 §4.4).
//!
//! A golden file is only evidence if two machines agree on what it means. So every fixture is built from a
//! fixed clock, a fixed set of committers, a fixed ordering rule and nothing else: no `SystemTime`, no
//! `HashMap` iteration order, no locale, no `git` subprocess whose behaviour would depend on whichever
//! `~/.gitconfig` the runner happens to have. When a golden diff appears it means a behaviour changed, and
//! not that the test ran on a Tuesday.
//!
//! This module is the only place in the crate allowed to name a constant that could otherwise be read from
//! the environment, and it names each one out loud so that a scenario function never has to improvise.

use serde::Serialize;

/// A commit author that is never a real person, on a domain that can never resolve (RFC 2606).
///
/// `Serialize` only: the fields are `&'static str` on purpose (a fixture is a constant, not a parse), so
/// a fixture that arrived over a wire is not what this type is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Author {
    /// The display name put in `author` and `committer`.
    pub name: &'static str,
    /// The address put in both; `fathom.invalid` cannot receive mail.
    pub email: &'static str,
}

/// The author a fixture falls back to when a caller needs one without naming a persona.
///
/// The corpus itself commits as [`crate::policy::Identity`] values — an agent, a bot and two people, because
/// a corpus with one author cannot test author attribution — and this constant is what older consumers (the
/// TUI's fixed-clock tests) use. What it shares with the identities is the domain: nothing in this crate
/// commits as a real address.
pub const FIXED_AUTHOR: Author = Author {
    name: "Fathom Fixture",
    email: "fixture@fathom.invalid",
};

/// The instant every fixture timestamps with — the same value `fathom-core` publishes, so a fixture and a
/// fixture-derived golden file cannot disagree about what "the epoch of the corpus" means.
pub const FIXED_TIMESTAMP: fathom_core::time::Timestamp =
    fathom_core::time::Timestamp::FIXED_FIXTURE;

/// The collation every fixture list is written in.
///
/// `LC_ALL=C` is byte order, which is what Rust's `Ord` on `str` already is; the name exists so the
/// claim is stated and tested instead of being an accident of which comparator a caller happened to pick.
pub const COLLATION: &str = "C";

/// The ticket that owns the corpus this crate holds (`reco-whw`, `e0.10`).
///
/// Exported so that a stub message and a placeholder in a downstream test can point at the plan instead
/// of at this file.
pub const OWNED_BY: &str = "e0.10";

/// The schema name of a checked-in ground-truth file. Bumping it is the one way to say "every ground-truth
/// file in `corpus/` means something different now"; `--bless` refuses to touch a file that names another.
pub const GROUND_TRUTH_SCHEMA: &str = "fathom.fixture-ground-truth/1";

/// The schema name of the `fixture.json` a generated repository carries.
pub const FIXTURE_SCHEMA: &str = "fathom.fixture/1";

/// The Unix seconds [`FIXED_TIMESTAMP`] falls on, so a scenario can say "`+ 90`" instead of doing calendar
/// arithmetic — and so the corpus shares its epoch with `fathom-ingest`'s own fixtures by number, not by
/// coincidence.
pub const BASE_EPOCH: i64 = 1_735_689_600;

/// The UTC offset every fixture signature carries, in minutes: `+0000`.
///
/// One offset everywhere means one answer to "what time is this commit", and the one place that breaks the
/// rule ([`HUMAN_REVIEWER`], who lives in `+0100`) says so in the table below.
pub const UTC_OFFSET_MINUTES: i32 = 0;

/// Sort a list the way the corpus is sorted: by UTF-8 bytes, no locale, no case folding.
pub fn sort_in_corpus_order<T: AsRef<str>>(items: &mut [T]) {
    items.sort_by(|left, right| left.as_ref().cmp(right.as_ref()));
}

/// Whether a committer is a machine or a person — which is what ADR-003 §8's `author_kind` is about, and
/// what the harness event stream is evidence for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An agent: it has a session, turns, and a tool name in its trailers.
    Agent,
    /// A person: no session, no turns, and usually a non-UTC offset.
    Human,
    /// A machine that is not an agent loop (a dependency bot, a vendoring job). Treated as an agent by
    /// `author_kind` — the marker table is what prices it, not this enum — but named separately because
    /// "an agent wrote this" and "a bot bumped this" are different review questions.
    Bot,
}

/// One fixed committer: an identity, what kind of actor it is, and the clock skew it is dated with.
#[derive(Debug, Clone, Copy)]
pub struct Identity {
    /// The stable key the corpus and the ground-truth files use to name this committer.
    pub key: &'static str,
    /// The name and address that go into both `author` and `committer`.
    pub author: Author,
    /// See [`Kind`].
    pub kind: Kind,
    /// The `Assisted-by:` value for an agent or bot; empty for a person, and a person gets no such trailer.
    pub tool: &'static str,
    /// Minutes east of UTC on every signature. Not cosmetic: the offset is part of the commit bytes, so
    /// it is part of the SHA, and a fixture that inherited the runner's timezone would not reproduce.
    pub offset_minutes: i32,
}

impl Identity {
    /// The `Agent-Session:`/harness identity this committer uses when a scenario gives it a session.
    ///
    /// Session ids are derived from the key so that two scenarios using the same actor produce ids a
    /// reader can tell apart, and so no scenario ever invents an id that collides with another's.
    #[must_use]
    pub fn session(&self, n: u32) -> Session {
        Session {
            id: format!("{}:s{n}", self.key),
        }
    }

    /// The harness product name recorded in `harness:` for this committer, or `human` for a person.
    #[must_use]
    pub fn harness(&self) -> &'static str {
        match self.kind {
            Kind::Agent => "claude_code",
            Kind::Bot => "generic_jsonl",
            Kind::Human => "human",
        }
    }
}

/// A harness session: the value of the `Agent-Session:` trailer and of `session_id` in the event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The id itself, e.g. `pi-agent:s1`.
    pub id: String,
}

/// The agent every scenario commits as by default.
pub const PI: Identity = Identity {
    key: "pi-agent",
    author: Author {
        name: "pi-agent",
        email: "pi-agent@fathom.invalid",
    },
    kind: Kind::Agent,
    tool: "pi-agent",
    offset_minutes: UTC_OFFSET_MINUTES,
};

/// A second agent, for the cases where two agent sessions interleave and the boundary is the point.
pub const CODEX: Identity = Identity {
    key: "codex-agent",
    author: Author {
        name: "codex-agent",
        email: "codex-agent@fathom.invalid",
    },
    kind: Kind::Agent,
    tool: "codex-cli",
    offset_minutes: UTC_OFFSET_MINUTES,
};

/// The dependency bot. `bot` is on `fathom-ingest`'s agent-marker table, so this identity is priced as an
/// agent by evidence rather than by declaration — which is the point of putting it on the table at all.
pub const DEPS_BOT: Identity = Identity {
    key: "deps-bot",
    author: Author {
        name: "deps-bot",
        email: "deps-bot@fathom.invalid",
    },
    kind: Kind::Bot,
    tool: "dependabot",
    offset_minutes: UTC_OFFSET_MINUTES,
};

/// A person who reviews and fixes things by hand, in `+0100`.
pub const HUMAN_REVIEWER: Identity = Identity {
    key: "dana",
    author: Author {
        name: "Dana Reviewer",
        email: "dana@example.invalid",
    },
    kind: Kind::Human,
    tool: "",
    offset_minutes: 60,
};

/// A second person, for the interleaving and the "whose episode is this" questions.
pub const HUMAN_MAINTAINER: Identity = Identity {
    key: "sam",
    author: Author {
        name: "Sam Maintainer",
        email: "sam@example.invalid",
    },
    kind: Kind::Human,
    tool: "",
    offset_minutes: UTC_OFFSET_MINUTES,
};

/// Every committer the corpus uses, in the order the manifest lists them.
pub const IDENTITIES: &[Identity] = &[PI, CODEX, DEPS_BOT, HUMAN_REVIEWER, HUMAN_MAINTAINER];

/// The committer named `key`, if the corpus knows one.
#[must_use]
pub fn identity(key: &str) -> Option<&'static Identity> {
    IDENTITIES.iter().find(|identity| identity.key == key)
}

/// An ISO-8601 UTC timestamp with milliseconds, the shape ADR-005 §2 gives `ts`.
///
/// Written by division rather than by `chrono` (which is not in the tree) and always in UTC, because a
/// fixture whose event timestamps depend on the runner's `TZ` is a fixture with a flake in it.
#[must_use]
pub fn iso_utc(unix_seconds: i64, millis: u32) -> String {
    let stamp = fathom_core::time::Timestamp::from_unix_micros(unix_seconds * 1_000_000);
    let (year, month, day) = stamp.civil();
    // Seconds into the UTC day: `civil()` already handles the calendar, so the time of day is a remainder.
    let secs_of_day = stamp.as_unix_seconds().rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs_of_day / 3_600,
        (secs_of_day % 3_600) / 60,
        secs_of_day % 60
    )
}

/// The git-style `+HHMM`/`-HHMM` form of an offset in minutes.
#[must_use]
pub fn offset_string(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let abs = offset_minutes.abs();
    format!("{sign}{:02}{:02}", abs / 60, abs % 60)
}
