//! Write a queue-era act log into a store that a queue-era `fathom index` already filled in.
//!
//! This file is **not compiled by this workspace**. `tools/freeze_queue_era_store.py` copies it into a
//! pinned checkout of the tree that still shipped the attention queue — at or before `81eba3b`, epic 10's
//! decision commit — and builds it there as an `examples/` target of `fathom-store`:
//!
//! ```text
//! cargo run --locked -p fathom-store --features sqlite-bundled \
//!     --example freeze_queue_era_store -- <repo> <acts.json>
//! ```
//!
//! # Why it has to be built there
//!
//! The artefact this produces is only worth having because of *who wrote it*. ADR-006 §5.1.2 §7 and
//! `docs/concepts/review-ledger.md` ask for a store written by the build that still had the queue, read
//! back by a build that no longer has the pane, so that a later migration cannot quietly change what an old
//! store means. A file written by today's `fathom-store` could not test that: it would agree with today's
//! reader because it was written by today's writer. So the acts go in through the pinned crate's own
//! `Store::record_triage` — the same door the `x`/`f`/`-`/`z` keys reached, and the only door that keeps
//! the act log and the `review_state` marker in one transaction. Nothing here invents a row: no hand-written
//! `INSERT`, no `sqlite3` invocation, no today's-code shortcut.
//!
//! # What it reads and what it writes
//!
//! `<repo>` is a repository the driver has already indexed with the pinned binary; `<acts.json>` is the act
//! table the driver assembled — the authored dispositions/reasons/notes/conditions from
//! `tools/freeze_queue_era_store.py`, with the episode ids taken from the store and the risk stamp taken
//! from the pinned build's own `fathom analyze --json`. Each act becomes one `record_triage` call.
//!
//! Then it reads every act back with the pinned reader and prints the read-back as JSON on stdout, which the
//! driver records in the fixture's `manifest.json`. That is the half of the ground truth that is *not* the
//! generator's opinion: it is what the queue-era write path said it stored, and it is what the current
//! build's read path is held to by `crates/fathom-store/tests/queue_era_store.rs`.
//!
//! ADR-002 §4.4 keeps `fathom-fixtures` out of the shipped binary; this file is not in it either. It is a
//! one-shot tool that runs once, on one machine, in a checkout nobody builds for release, and its output is
//! the committed file.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::ExitCode;

use fathom_store::episode::{ReviewState, TriageEvent};
use fathom_store::spine::Store;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(root), Some(acts_path)) = (args.next(), args.next()) else {
        eprintln!("usage: freeze_queue_era_store <repo> <acts.json>");
        return ExitCode::from(2);
    };
    let store_path = Path::new(&root).join(".fathom").join("index.sqlite3");
    let text = std::fs::read_to_string(&acts_path)
        .unwrap_or_else(|err| panic!("{}: {err}", Path::new(&acts_path).display()));
    let document: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("{}: {err}", Path::new(&acts_path).display()));
    let acts = document["acts"]
        .as_array()
        .expect("<acts.json> has an `acts` array");

    let mut store = Store::open(&store_path)
        .unwrap_or_else(|err| panic!("{}: {err}", store_path.display()));

    let mut written = Vec::with_capacity(acts.len());
    for act in acts {
        let field = |key: &str| act[key].as_str().unwrap_or_default().to_owned();
        let event = TriageEvent {
            episode: field("episode"),
            // The store re-reads the fingerprint from the episode row rather than trusting this (`record_triage`),
            // which is the point: an act cannot be signed against a digest the writer was never shown.
            fingerprint: String::new(),
            disposition: ReviewState::parse(&field("disposition")),
            reason: field("reason"),
            note: field("note"),
            until: field("until"),
            actor: field("actor"),
            risk: act["risk"].as_i64(),
            risk_model: field("risk_model"),
            utc: act["utc"].as_i64().expect("an act carries its instant"),
        };
        let seq = store
            .record_triage(&event)
            .unwrap_or_else(|err| panic!("recording an act on {}: {err}", event.episode));
        written.push(seq);
    }

    // Read it back with the pinned reader, in the order the pinned retrospective reads it, so the manifest
    // records the store's own answer rather than the input file's.
    let read_back = store
        .triage_events(5_000)
        .expect("the act log reads back");
    let markers: Vec<serde_json::Value> = read_back
        .iter()
        .map(|act| {
            let marker = store
                .last_triage(&act.episode)
                .expect("the marker reads back")
                .expect("every act leaves a marker");
            serde_json::json!({
                "episode": act.episode,
                "marker_state": marker.disposition.as_str(),
            })
        })
        .collect();
    let acts: Vec<serde_json::Value> = read_back
        .iter()
        .rev()
        .map(|act| {
            serde_json::json!({
                "episode": act.episode,
                "fingerprint": act.fingerprint,
                "disposition": act.disposition.as_str(),
                "reason": act.reason,
                "note": act.note,
                "until": act.until,
                "actor": act.actor,
                "risk": act.risk,
                "risk_model": act.risk_model,
                "utc": act.utc,
            })
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "fathom.freeze-queue-era-store/1",
            "store": store_path,
            "acts_written": written.len(),
            "triage_event_count": store.triage_event_count().expect("the act log counts"),
            "acts": acts,
            "markers": markers,
        }))
        .expect("the read-back serialises")
    );
    ExitCode::SUCCESS
}
