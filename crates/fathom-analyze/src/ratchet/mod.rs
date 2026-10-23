//! The ratchet: the write path of `.fathom/ratchet.toml` (`e3.10`, `reco-5ht`).
//!
//! [`crate::cycles::debt`] and [`crate::layering::debt`] already answer *what does this baseline forgive*,
//! each for its own kind of violation, and each refuses to touch the file. This module is the half that
//! writes it, and the whole ticket is the difference between those two verbs: a tool that can only report a
//! baseline is a tool whose baseline is maintained by hand, and a baseline maintained by hand is a file that
//! drifts — rows nobody can delete, owners who left, dates nobody notices.
//!
//! # The one-way rule, which is the module
//!
//! ```text
//!   violation disappears  ───────────────────────────────►  row leaves the file   automatic
//!   violation appears     ── flag + reason + owner ─────►  row enters the file   deliberate
//! ```
//!
//! Tightening is automatic because it cannot lie: an allowance whose violation is gone forgives nothing, and
//! leaving it in the file is what makes a baseline unreadable — a reviewer who has to work out which of
//! forty rows still bite stops reading all forty. Widening is deliberate because it can: a run that quietly
//! absorbed a new violation is a ratchet that stopped being one, which is why [`Grant`] cannot be
//! constructed without a reason and why [`crate::layering::debt`] and [`crate::cycles::debt`] treat an
//! uncovered violation as news until a row says otherwise.
//!
//! The act is recorded in three places, so it cannot be deniable later:
//!
//! * **the file**, where every granted row carries `owner`, `expires`, `reason`, `granted` and `granted_by`;
//! * **the diff**, printed at the moment of the act, one `-`/`+` row per change, who and why in the line
//!   itself ([`Plan::diff`]) — this is what a reviewer reads in the pull request;
//! * **the review memory**, when the repository keeps one: [`crate::notes`] gets a `decision` note whose
//!   text is the reason, so "why is this cycle allowed" has an answer with an author and a date attached.
//!
//! # What a row forgives is decided by the engine that owns the kind
//!
//! Nothing here matches a key. [`crate::cycles::debt::pattern_for`] is the matcher for `cycle` rows and
//! [`crate::layering::debt::normalize_key`] the matcher for `layer` rows, and this module only ever asks an
//! [`Offence`] and a row whether they are the same fact. A ratchet writer with its own matching rule is a
//! second matching rule, and the two would disagree about what is forgiven within a release.
//!
//! # No clock, no filesystem, same as the engines
//!
//! [`Plan::build`] is a pure function of (baseline, current offences, the grant if any, `now`). The verb in
//! `fathom-cli` reads the file, walks the lane, calls [`inventory`] for the offences, and writes what
//! [`Plan::render`] returns — so every rule in this module, including the auto-tightening, is a test with
//! literals rather than a fixture repository with a clock in it.

#![deny(missing_docs)]

mod plan;

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
mod inventory;

pub use crate::cycles::debt::{Allowance, EXPIRY_WARNING_DAYS, Mode, Ratchet, parse_date};
pub use plan::{CONTRACT, DEFAULT_GRACE_DAYS, Evaluated, Grant, Ledger, Offence, Plan, Removal};

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub use inventory::{Inventory, inventory};
