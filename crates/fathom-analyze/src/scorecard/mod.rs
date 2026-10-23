//! The architecture scorecard: five dimensions, one hand-reproducible number, and the drift of that
//! number across a week of agent output (`e3.9`, `reco-al1`).
//!
//! The ticket's sentence, in full: *"Roll the architecture signals into something a human can track
//! across a week of agent output: a five-dimension scorecard (boundaries respected, coupling, acyclicity,
//! API hygiene, hotspot pressure) with the documented formula, per-episode and per-window scores, a trend
//! line across episodes, and 'since my last review' deltas so catching up after two days is one number.
//! Expose `fathom architecture --json` for CI and dashboards."* Every clause is a module here:
//!
//! | Clause | Where |
//! |---|---|
//! | the five dimensions and what each one measures | [`dimension`] |
//! | the formula, the weights, the arithmetic | [`score`], [`policy`] |
//! | per-window score, trend line, bounded memory | [`trend`] |
//! | "since my last review", correct across a rebase | [`review`] |
//! | the words and the JSON every surface reads | [`report`] |
//! | the store seam that produces a row per revision | [`lane`] (SQLite-gated) |
//!
//! # The formula, in full
//!
//! This is the document the acceptance criterion asks for: a reader holding the exported JSON can redo
//! every number in it with a calculator, and nothing else is used to produce a score.
//!
//! ```text
//!   For each dimension d of {boundaries, coupling, acyclicity, api, hotspots}:
//!
//!     share-curve   x_d = n_d / d_d                     (refused, not scored, when d_d = 0)
//!     knee-curve    x_d = n_d / (n_d + k_d)             (x_d = 0 when n_d = 0)
//!     h_d   = 1 − x_d                                   the dimension's health
//!
//!   W        = Σ w_d                        over the dimensions that were measured
//!   points_d = round(10000 · w_d · h_d / W)            integer hundredths
//!   total    = Σ points_d                              exact sum of the printed parts
//!   score    = clamp(round(total / 100), 0, 100)       0–100, higher is better
//! ```
//!
//! and the five curves, with the integers each one is made of:
//!
//! | dimension | curve | `n` | `d` / `k` | default weight |
//! |---|---|---|---|---|
//! | boundaries | share | layering violations with no live allowance | dependency rows judged by the layering engine | 0.30 |
//! | coupling | share | dependency edges in the graph | modules × `coupling_ceiling` (default 8 edges/module) | 0.15 |
//! | acyclicity | share | nodes inside a cycle **with no live allowance** | nodes in the graph | 0.25 |
//! | api | knee | exports this revision lost (`e3.7`'s breaking count when a caller has that diff) | `api_knee`, default 4 | 0.15 |
//! | hotspots | share | modules scoring at or above `hotspot_bar` (default 70/100) | modules scored by the hotspot ranking | 0.15 |
//!
//! Every one of those integers is exported twice — once inside the dimension's own `formula` line as
//! `3 / 412 = 0.0073` and once as a named field — so a reader can check one dimension without parsing the
//! rest. Rounded values are rounded **before** they are used, so what is printed is what divided.
//!
//! ## A worked example, end to end
//!
//! ```text
//! boundaries  3 / 412          = 0.0073  badness → h 0.9927 · w 0.30
//! acyclicity  18 / 640         = 0.0281  badness → h 0.9719 · w 0.25
//! coupling    340 / (80·8)     = 0.5313  badness → h 0.4687 · w 0.15
//! hotspots    7 / 80           = 0.0875  badness → h 0.9125 · w 0.15
//! api         not measured (no earlier revision in the index) → w 0 redistributed
//!
//! W        = 0.30 + 0.25 + 0.15 + 0.15                     = 0.85
//! boundaries = round(10000 · 0.30 · 0.9927 / 0.85)        = 3504   (35.04)
//! acyclicity = round(10000 · 0.25 · 0.9719 / 0.85)        = 2859   (28.59)
//! coupling   = round(10000 · 0.15 · 0.4687 / 0.85)        = 827    ( 8.27)
//! hotspots   = round(10000 · 0.15 · 0.9125 / 0.85)        = 1610   (16.10)
//! total    = 3504 + 2859 + 827 + 1610                    = 87    (8800 hundredths)
//! score    = round(8800 / 100)                           = 88
//! ```
//!
//! # What this module is not
//!
//! **Not a verdict about a repository's quality.** Four of the five dimensions describe a shape rather
//! than a mistake — a coupled graph can be a well-designed graph, and a hot module is usually the
//! load-bearing wall `e3.8` warned about. The scorecard says *where attention is owed*, and the same
//! sentence-evidence the source reports print is exported beside every term so a reviewer can disagree
//! with a dimension without disagreeing with the arithmetic.
//!
//! **Not a new analysis.** Each dimension reads a report another module already exports
//! ([`crate::layering::Report`], [`crate::cycles::Report`], [`crate::hotspots::Table`], [`crate::graph`])
//! and this module computes nothing those modules had to compute anyway (lint `M03`).
//!
//! **Not a baseline, a ratchet or a gate.** `.fathom/ratchet.toml` and its expiring allowances are
//! **reco-5ht**'s; `fathom check`'s exit code is **reco-ngk**'s. This module reports a score and, at most,
//! whether it sits below the reported `alert_below`.
//!
//! **Not a clock, a store or a git reader** — except in [`lane`], which is the one store-gated seam, same
//! as [`crate::cycles::episode`] and [`crate::layering::episode`].

pub mod dimension;
pub mod policy;
pub mod report;
pub mod review;
pub mod score;
pub mod trend;

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod lane;

pub use crate::scorecard::dimension::{ApiGrain, ApiSignal, Curve, Measurement, Origin};
pub use crate::scorecard::policy::{Policy, policy_from_config_text};
pub use crate::scorecard::report::Report;
pub use crate::scorecard::review::{
    Anchor, AnchorKind, Delta, Direction, Review, TermDelta, resolve,
};
pub use crate::scorecard::score::{MODEL, MODEL_VERSION, Scorecard, Term};
pub use crate::scorecard::trend::{Parse, Point, Row, Trend, Window};

/// The contract id the `--json` document carries, mirrored by [`crate::scorecard::report`] and documented
/// in [`docs/concepts/scorecard.md`](../../../docs/concepts/scorecard.md). A consumer that reads
/// `fathom-architecture/1` is promising to ignore unknown keys; a producer that bumps it is promising the
/// old keys still mean what they meant.
pub const CONTRACT: &str = "fathom-architecture/1";

/// The scorecard's own schema version, separate from [`MODEL_VERSION`]: the model's arithmetic may be
/// untouched while the *document* around it grows, and the two must be able to say so independently.
pub const SCHEMA_VERSION: u32 = 1;
