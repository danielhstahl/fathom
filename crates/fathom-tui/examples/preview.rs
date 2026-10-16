#![allow(clippy::print_stdout)] // it is a preview: stdout is the whole point.
//! Print one frame as text: `cargo run -p fathom-tui --example preview -- [view] [cols] [rows]`.
use fathom_tui::frame::{Options, render_at};
use fathom_tui::views::ViewId;

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
    let options = Options {
        repo_label: "fathom/pilot · main".to_owned(),
        ..Options::default()
    };
    for line in render_at(&options, view, (cols, rows)) {
        println!("{line}|");
    }
}
