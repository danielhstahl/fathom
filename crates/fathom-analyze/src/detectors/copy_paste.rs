//! `copy_paste_growth.rs` — new text arrived as a copy of other new text (`e4.3`).
//!
//! **Ships disabled**, and it is the most honestly-culpable of the four: a twelve-line block of new code
//! that matches another twelve-line block of new code is a *fact* this detector computes exactly, and the
//! fact is nearly worthless on its own because so many legitimate things look like it — a table-driven test,
//! two implementations of one interface, a match arm per variant, a struct of column definitions. The
//! `e4.3a` disposition set is what says whether the ratio is anywhere near the `G3` alpha bar; until then
//! this runs when a repository asks for it and not by default.
//!
//! **What it computes.** Over the *added* lines of the episode — folded by
//! [`crate::detectors::text::normalise`], blank lines dropped — every window of
//! `detectors.copy.min_block_lines` (default 12) consecutive lines is hashed, and a hash that turns up in
//! two distinct places is a repeated block. Two distinct places are two different files, or the same file at
//! starts at least a block apart, so a block never matches its own shifted copy. Overlapping windows that
//! belong to one run are merged, so "how much is repeated" counts lines and not windows.
//!
//! **Four refusals.**
//!
//! 1. **not code, not read**: prose, configuration, generated and vendored paths;
//! 2. **tests are not read**: a table-driven test *is* repeated structure by design, and `e2.9`'s
//!    test-surface delta is the ticket that knows what a test case looks like;
//! 3. **punctuation and comments are not substance**: [`crate::detectors::text::substantive`] requires the
//!    window to be mostly letters, because `}` `,` `}` `,` repeats everywhere and means nothing;
//! 4. **import runs are not structure**: a twelve-line block of `use` statements is what the top of a big
//!    file looks like, and two files with the same imports is not duplication.
//!
//! **One finding per file**, at `low` severity and `0.5` confidence — the two together are "look at this",
//! not "change this". A repository that has decided duplication is not its problem silences the whole
//! detector with `detectors.suppress = ["copy_paste_growth"]`.

use std::collections::{BTreeMap, BTreeSet};

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text::{self, Dialect};

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let block = policy.copy_min_block_lines.max(2);

    // Every code file's added lines, folded, in file order.
    let mut files: Vec<(&FileChange, Vec<String>)> = Vec::new();
    for file in &episode.files {
        if !reads(file) {
            continue;
        }
        let lines: Vec<String> = file
            .added()
            .into_iter()
            .map(text::normalise)
            .filter(|line| !line.is_empty())
            .collect();
        if lines.len() >= block {
            files.push((file, lines));
        }
    }
    if files.is_empty() || (files.len() == 1 && files[0].1.len() < block.saturating_mul(2)) {
        // One file with room for a single block cannot contain a repeated one, and hashing it would be
        // the expensive way to find that out.
        return Vec::new();
    }

    // window digest → the places it turns up, plus the text for the evidence line.
    let mut windows: BTreeMap<u64, Vec<(usize, usize)>> = BTreeMap::new();
    let mut texts: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for (index, (file, lines)) in files.iter().enumerate() {
        let dialect = text::dialect(&file.path).unwrap_or(Dialect::Unknown);
        for start in 0..=lines.len().saturating_sub(block) {
            let window = &lines[start..start + block];
            if !text::substantive(window, dialect) || imports_only(window) {
                continue;
            }
            windows
                .entry(fingerprint(window))
                .or_default()
                .push((index, start));
            texts
                .entry(fingerprint(window))
                .or_insert_with(|| window.to_vec());
        }
    }

    // Per file: which line ranges participate in a repeat, who the partner is, and one quoted sample.
    let mut runs: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    let mut partners: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let mut sample: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (digest, places) in &windows {
        let mut distinct: Vec<(usize, usize)> = Vec::new();
        for place in places {
            let shifted = distinct.iter().any(|(file, start)| {
                *file == place.0 && (*start as i64 - place.1 as i64).abs() < block as i64
            });
            if !shifted {
                distinct.push(*place);
            }
        }
        if distinct.len() < 2 {
            continue;
        }
        for (index, start) in &distinct {
            runs.entry(*index)
                .or_default()
                .push((*start, start + block - 1));
            for other in distinct.iter().filter(|other| other.0 != *index) {
                partners
                    .entry(*index)
                    .or_default()
                    .insert(files[other.0].0.path.clone());
            }
            sample.entry(*index).or_insert_with(|| {
                texts
                    .get(digest)
                    .map_or_else(Vec::new, |lines| lines[..2.min(lines.len())].to_vec())
            });
        }
    }

    let mut findings = Vec::new();
    for (index, ranges) in runs {
        let (file, _) = files[index];
        let merged = merge(ranges);
        let lines: usize = merged.iter().map(|(start, end)| end - start + 1).sum();
        let largest = merged
            .iter()
            .map(|(start, end)| end - start + 1)
            .max()
            .unwrap_or(0);
        let others: Vec<String> = partners
            .remove(&index)
            .map_or_else(Vec::new, |set| set.into_iter().collect());
        let where_text = if others.is_empty() {
            "again in this file".to_owned()
        } else {
            format!(
                "in {}",
                crate::detectors::named(
                    &others.iter().map(String::as_str).collect::<Vec<_>>(),
                    policy.name_cap
                )
            )
        };
        findings.push(
            Finding::build(
                "copy_paste_growth",
                1,
                &episode.id,
                file.path.clone(),
                format!(
                    "{}: {} of its {} new line(s) sit in {} repeated {} of ≥{block} consecutive lines \
                     (longest {largest}); the same text appears {where_text}",
                    file.path,
                    lines,
                    file.add,
                    merged.len(),
                    if merged.len() == 1 { "run" } else { "runs" }
                ),
            )
            .severity(Severity::Low)
            .confidence(0.5)
            .evidence(Evidence::observed(
                EvidenceKind::Hunk,
                format!(
                    "{} {}",
                    file.path,
                    file.hunks.first().map_or_else(String::new, |hunk| hunk.header.clone())
                ),
                crate::detectors::quote(
                    &sample.remove(&index).unwrap_or_default().iter().map(String::as_str).collect::<Vec<_>>(),
                    policy.evidence_lines
                ),
            ))
            .jump_path(&file.path)
            .finish(),
        );
    }
    findings.sort_by(|a, b| a.subject.cmp(&b.subject));
    findings
}

/// Merge overlapping or touching line ranges, so one 24-line run counts as one and not as thirteen windows.
fn merge(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match out.last_mut() {
            Some(last) if start <= last.1 + 1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// Code a person wrote, and not the test surface: a table-driven test repeats structure on purpose.
fn reads(file: &FileChange) -> bool {
    !file.binary
        && !file.noisy()
        && !file.is_test()
        && !file.is_delete()
        && text::dialect(&file.path).is_some_and(Dialect::is_code)
}

/// Every line of the window is an import or a module open: the top of a file, not a duplicated idea.
fn imports_only(window: &[String]) -> bool {
    window.iter().all(|line| {
        let folded = line.trim_start();
        folded.starts_with("use ")
            || folded.starts_with("pub use ")
            || folded.starts_with("import ")
            || folded.starts_with("from ")
            || folded.starts_with("require(")
            || folded.starts_with("#include")
            || folded.starts_with("export ")
    })
}

/// FNV-1a over the window. Not a security hash: a deterministic, cheap identity for "these lines are the
/// same", which is all the detector asks of it.
fn fingerprint(window: &[String]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for line in window {
        for byte in line.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, Hunk, change};

    fn block(prefix: &str) -> Vec<String> {
        (0..12)
            .map(|n| format!("{prefix} step {n} = compute({n}) * factor;"))
            .collect()
    }

    fn file(path: &str, lines: &[String]) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: change::ADDED.to_owned(),
            add: lines.len() as i64,
            hunks: vec![Hunk::new(
                format!("@@ -0,0 +1,{} @@", lines.len()),
                lines.to_vec(),
                Vec::<String>::new(),
            )],
            ..Default::default()
        }
    }

    fn episode(files: Vec<FileChange>) -> Episode {
        Episode {
            id: "ep-1".to_owned(),
            files,
            content: Content::Full,
            ..Episode::new("ep-1")
        }
    }

    fn detect_one(files: Vec<FileChange>, policy: &Policy) -> Vec<Finding> {
        let episode = episode(files);
        detect(&episode, &Corpus::default(), policy)
    }

    #[test]
    fn twelve_new_lines_that_appear_twice_are_growth() {
        let block = block("let a");
        let mut lines = block.clone();
        lines.extend(block);
        let findings = detect_one(vec![file("src/a.rs", &lines)], &Policy::all_enabled());
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "copy_paste_growth");
        assert_eq!(
            finding.severity,
            Severity::Low,
            "a note about shape, not a demand"
        );
        assert!((finding.confidence - 0.5).abs() < f64::EPSILON);
        assert!(
            finding
                .message
                .contains("24 of its 24 new line(s) sit in 1 repeated run"),
            "the original and its copy touch, so they are one 24-line run: {}",
            finding.message
        );
        assert!(
            finding.message.contains("again in this file"),
            "{}",
            finding.message
        );
        assert!(
            finding.evidence[0].detail.contains("compute(0)"),
            "{}",
            finding.evidence[0].detail
        );
    }

    #[test]
    fn a_copy_across_two_files_names_the_partner() {
        let lines = block("let a");
        let findings = detect_one(
            vec![file("src/a.rs", &lines), file("src/b.rs", &lines)],
            &Policy::all_enabled(),
        );
        assert_eq!(findings.len(), 2);
        assert!(
            findings[0].message.contains("in src/b.rs"),
            "{}",
            findings[0].message
        );
        assert!(
            findings[1].message.contains("in src/a.rs"),
            "{}",
            findings[1].message
        );
        assert!(
            findings[0].message.contains("12 of its 12 new line(s)"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn a_block_shorter_than_the_knob_is_a_coincidence() {
        let short: Vec<String> = (0..8)
            .map(|n| format!("let v{n} = compute{n}();"))
            .collect();
        let mut lines = short.clone();
        lines.extend(short);
        assert!(detect_one(vec![file("src/a.rs", &lines)], &Policy::all_enabled()).is_empty());
        let lowered = Policy {
            copy_min_block_lines: 4,
            ..Policy::all_enabled()
        };
        assert_eq!(
            detect_one(vec![file("src/a.rs", &lines)], &lowered).len(),
            1,
            "and it was the knob that moved, not a mood"
        );
    }

    #[test]
    fn punctuation_imports_and_comments_are_not_substance() {
        let braces: Vec<String> = (0..24)
            .map(|n| {
                if n % 4 == 0 {
                    "    }".to_owned()
                } else {
                    "        },".to_owned()
                }
            })
            .collect();
        assert!(detect_one(vec![file("src/a.rs", &braces)], &Policy::all_enabled()).is_empty());

        let imports: Vec<String> = (0..12)
            .map(|n| format!("use crate::thing{n}::Item;"))
            .collect();
        let mut both = imports.clone();
        both.extend(imports);
        assert!(detect_one(vec![file("src/a.rs", &both)], &Policy::all_enabled()).is_empty());
    }

    #[test]
    fn the_test_surface_is_not_read() {
        let mut lines = block("let a");
        lines.extend(block("let a"));
        assert!(
            detect_one(vec![file("tests/api.rs", &lines)], &Policy::all_enabled()).is_empty(),
            "a table-driven test repeats on purpose"
        );
    }

    #[test]
    fn ordinary_distinct_code_is_silent() {
        let lines: Vec<String> = (0..40)
            .map(|n| format!("let unique{n} = other{n}();"))
            .collect();
        assert!(detect_one(vec![file("src/a.rs", &lines)], &Policy::all_enabled()).is_empty());
    }

    #[test]
    fn indentation_and_blank_lines_do_not_hide_a_copy() {
        let mut a = block("let a");
        a.insert(3, String::new());
        let b: Vec<String> = block("let a")
            .iter()
            .map(|line| format!("    {line}"))
            .collect();
        let findings = detect_one(
            vec![file("src/a.rs", &a), file("src/b.rs", &b)],
            &Policy::all_enabled(),
        );
        assert!(
            !findings.is_empty(),
            "the comparison is on folded lines: {findings:?}"
        );
    }

    #[test]
    fn a_run_is_counted_once_not_once_per_window() {
        // A 13-line duplication: thirteen overlapping windows, one run of 13 lines.
        let mut lines: Vec<String> = (0..13)
            .map(|n| format!("let long{n} = compute{n}();"))
            .collect();
        lines.extend((0..13).map(|n| format!("let long{n} = compute{n}();")));
        let findings = detect_one(vec![file("src/a.rs", &lines)], &Policy::all_enabled());
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .message
                .contains("26 of its 26 new line(s) sit in 1 repeated run"),
            "one run of 26 lines, not thirteen windows: {}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("longest 26"),
            "{}",
            findings[0].message
        );
    }
}
