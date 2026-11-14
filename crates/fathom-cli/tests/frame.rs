//! The drawn frame, read back as prose — shared with `tests/cli.rs` and `tests/tui_data.rs`.
//!
//! Both files ask a question about *which words a frame prints* ("does the unindexed pane name the verb
//! that works?", "does a finished empty index offer a remedy?"), and both have to read a sentence that the
//! renderer broke across rows. Since `e10.3` (`reco-ydek.3`) the home view is the timeline, whose rail
//! column is narrower than the attention queue's worklist was, so sentences that used to fit a line now
//! wrap mid-phrase — and an assertion written as one contiguous string would be a test of where the
//! renderer cut the line rather than of what the product said.
//!
//! Included as one module of two test binaries (`mod frame;`) rather than duplicated, the way
//! `tests/validator.rs` is: two copies of a de-wrapper is two chances to disagree about what a frame says.
#![allow(dead_code)]

/// The frame's first pane, folded open: hard wraps become single spaces.
///
/// Only the first column is read — the pane beside it is a different sentence, and joining whole rows
/// interleaves the two into nonsense. Border and separator glyphs fall out with the split.
pub fn unfolded(frame: &str) -> String {
    frame
        .lines()
        .map(|line| {
            let body = line.strip_prefix('│').unwrap_or(line);
            body.split(['│', '┬']).next().unwrap_or(body)
        })
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
