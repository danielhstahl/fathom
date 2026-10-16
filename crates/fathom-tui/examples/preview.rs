#![allow(clippy::print_stdout)] // it is a preview: stdout is the whole point.
//! Print one frame as text: `cargo run -p fathom-tui --example preview -- [view] [cols] [rows]`.
//!
//! For `timeline` the frame is drawn over the ADR-006 §5.2 mock's own episodes, so the picture in the
//! document and the picture this binary prints can be compared line for line.
use fathom_core::time::Timestamp;
use fathom_tui::frame::{Options, render_at};
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

/// The instant the mock is drawn for: Tue 14 Oct 2025, 12:00 UTC.
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let view = match args.first().map(String::as_str) {
        Some("timeline") => ViewId::Timeline,
        Some("diff") => ViewId::Diff,
        Some("ambient") => ViewId::Ambient,
        Some("search") => ViewId::Search,
        _ => ViewId::Queue,
    };
    let cols: u16 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(80);
    let rows: u16 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(24);
    let minutes = |ago: i64| Timestamp::from_unix_micros(NOW.as_unix_micros() - ago * 60_000_000);
    let options = Options {
        repo_label: "fathom/pilot · main".to_owned(),
        now: NOW,
        episodes: (view == ViewId::Timeline).then(|| {
            // The ADR's own rows, in the ADR's order: three days, two kinds, one unknown state, and
            // the mixture of read and unread the marker column exists for.
            let mut rows: Vec<Episode> = Vec::new();
            let mut add = |id: &str,
                           ago: i64,
                           kind: AuthorKind,
                           add: u64,
                           del: u64,
                           state: ReviewState,
                           summary: &str| {
                let mut episode = Episode::new(id, minutes(ago), summary);
                episode.author = kind;
                episode.author_name = match kind {
                    AuthorKind::Agent => "pi",
                    AuthorKind::Human => "stahl",
                    AuthorKind::Unknown => "",
                }
                .to_owned();
                episode.author_confidence = if kind == AuthorKind::Unknown {
                    None
                } else {
                    Some(0.94)
                };
                episode.n_add = add;
                episode.n_del = del;
                episode.n_files = if add > 300 { 12 } else { 3 };
                episode.state = state;
                episode.session = Some(if ago > 60 * 24 { "3" } else { "4" }.to_owned());
                episode.members = (0..2).map(|i| format!("{id}{i}")).collect();
                episode.paths = vec![
                    "src/store/epoch.rs".to_owned(),
                    "tests/test_epoch.rs".to_owned(),
                ];
                episode.boundary_rule = Some("author".to_owned());
                episode.boundary_confidence = Some(0.75);
                rows.push(episode);
            };
            use AuthorKind::{Agent, Human, Unknown};
            use ReviewState::{Accepted, Dismissed, Unread};
            add(
                "e8f1a2",
                12,
                Agent,
                412,
                90,
                Unread,
                "make the store epoch-safe",
            );
            add("9c4d10", 41, Agent, 58, 12, Unread, "cache the epoch probe");
            add("2be77f", 67, Human, 8, 3, Accepted, "typo: docs");
            add(
                "77aa0c",
                60 * 25,
                Agent,
                1_204,
                90,
                Accepted,
                "rewrite the walk",
            );
            add(
                "40c9b3",
                60 * 28,
                Agent,
                96,
                0,
                Unread,
                "add the schema stamp",
            );
            add("c1f5e2", 60 * 32, Unknown, 22, 22, Unread, "unknown author");
            add(
                "08d4b1",
                60 * 60 * 26,
                Agent,
                3,
                1,
                Accepted,
                "fix the clippy lint",
            );
            add(
                "5f2c99",
                60 * 60 * 30,
                Agent,
                61,
                61,
                Dismissed,
                "revert the revert",
            );
            Batch::new("preview fixture", rows)
        }),
        ..Options::default()
    };
    for line in render_at(&options, view, (cols, rows)) {
        println!("{line}|");
    }
}
