//! `segment.rs` — the episode boundary knobs, and the one place allowed to go and fetch them.
//!
//! ADR-002 §4.5 is blunt about this: no crate below `fathom-cli` reads a config file, because a crate that
//! reads configuration to decide how to analyse is a crate whose cache keys are lying. So
//! [`fathom_ingest::segment::Rules`] arrives at the segmentation as a value, and this module is where the
//! value is made: read `[episode]` out of the repository's `fathom.toml`, turn it into `Rules`, and say
//! out loud what could not be used.
//!
//! # What is read, and what is deliberately not
//!
//! `time_gap_minutes`, `force_split` and `force_merge` are read. `boundaries` — the declared *priority
//! order* of the boundary sources — is recognised and reported as not-yet-honoured: the order in
//! `segment.rs` is the order ADR-003 will fix, and ADR-003 is still open (`reco-ubw`). Silently accepting a
//! key and ignoring its contents is the failure mode this repository's whole config surface exists to
//! prevent, so an unusual `boundaries` value produces a notice on the run that ignored it rather than a
//! quiet difference in the result.
//!
//! Layering is `recommender.config`'s business (precedence, user file, environment); this reads the repo
//! layer, which is the layer the ticket's criterion is about, and takes the resolved text as an argument in
//! [`rules_from_toml`] so the layer resolver can take it over without the parsing moving again.

use std::path::Path;

use fathom_ingest::segment::{Override, Rules};

/// What the configuration said about segmentation, and what could not be honoured.
#[derive(Debug, Default, Clone)]
pub struct Resolved {
    /// The rules to hand to the index run.
    pub rules: Rules,
    /// One line per key that was read and could not be used, in the file's own order.
    pub notices: Vec<String>,
}

impl Resolved {
    /// The default rules, for a repository with no `fathom.toml`.
    #[must_use]
    pub fn defaults() -> Self {
        Resolved {
            rules: Rules::default(),
            notices: Vec::new(),
        }
    }
}

/// Read `[episode]` from the repository's own `fathom.toml`.
///
/// A missing or unreadable file is the default rules with no notice: not every repository configures
/// Fathom, and `fathom index` in an unconfigured one is the normal case, not a degraded one. A file that
/// does not parse is a notice, because a broken config that silently analyses like an empty one is the
/// failure ADR-002 §7.1 refuses.
#[must_use]
pub fn from_repository(root: &Path) -> Resolved {
    let path = root.join("fathom.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => rules_from_toml(&text),
        Err(_) => Resolved::defaults(),
    }
}

/// Parse the `[episode]` table out of `fathom.toml` text.
///
/// # Errors
///
/// A one-line message when the TOML itself does not parse. A well-formed table whose *values* are unusable
/// is not an error: it is a notice, and the run continues on the parts that did parse.
pub fn rules_from_toml(text: &str) -> Resolved {
    let mut out = Resolved::defaults();
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => {
            out.notices.push(format!(
                "fathom.toml does not parse, so segmentation used its defaults: {err}"
            ));
            return out;
        }
    };
    let Some(episode) = document.get("episode").and_then(toml_edit::Item::as_table) else {
        return out;
    };

    if let Some(minutes) = episode
        .get("time_gap_minutes")
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_integer)
    {
        if minutes <= 0 {
            out.notices.push(format!(
                "episode.time_gap_minutes = {minutes} is not a gap; the default {} minutes were used",
                Rules::default().gap_millis / 60_000
            ));
        } else {
            out.rules.gap_millis = minutes.saturating_mul(60_000);
        }
    }

    for (key, into_split) in [("force_split", true), ("force_merge", false)] {
        let Some(entries) = episode.get(key).and_then(toml_edit::Item::as_array) else {
            continue;
        };
        let sink = if into_split {
            &mut out.rules.force_split
        } else {
            &mut out.rules.force_merge
        };
        for entry in entries.iter() {
            let Some(pattern) = entry.as_str() else {
                out.notices.push(format!(
                    "episode.{key} holds a non-string entry, which is not a pattern: {entry}"
                ));
                continue;
            };
            match override_for(pattern) {
                Some(found) => sink.push(found),
                None => out.notices.push(format!(
                    "episode.{key} entry `{pattern}` is not `oid:<prefix>` or `subject:<substring>`"
                )),
            }
        }
    }

    // `boundaries` is recognised, not honoured; see the module doc. The default spelling is a no-op so a
    // file that merely repeats the defaults is not nagged about it.
    if let Some(order) = episode
        .get("boundaries")
        .and_then(toml_edit::Item::as_array)
    {
        let declared: Vec<String> = order
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect();
        if !(declared == ["hook", "trailer", "author"].map(str::to_owned)) {
            out.notices.push(format!(
                "episode.boundaries = {:?} is recorded but not applied: the boundary order is \
                 ADR-003's, and `reco-ubw` is still open",
                declared
            ));
        }
    }

    out
}

/// `oid:` matches a commit id prefix, `subject:` a substring of the first line — the two spellings the
/// schema's item pattern allows and the only two a reviewer can write from what `fathom show` prints.
fn override_for(pattern: &str) -> Option<Override> {
    if let Some(prefix) = pattern.strip_prefix("oid:") {
        let prefix = prefix.trim();
        if prefix.chars().count() < 2 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return Some(Override {
            oid: Some(prefix.to_ascii_lowercase()),
            subject: None,
        });
    }
    if let Some(needle) = pattern.strip_prefix("subject:") {
        let needle = needle.trim();
        if needle.is_empty() {
            return None;
        }
        return Some(Override {
            oid: None,
            subject: Some(needle.to_owned()),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn an_unconfigured_repository_gets_the_default_rules_and_says_nothing() {
        let resolved = rules_from_toml("[repo]\nsize_class = \"auto\"\n");
        assert_eq!(
            resolved.rules.gap_millis,
            Rules::default().gap_millis,
            "no [episode] table is not a request to change the gap"
        );
        assert!(
            resolved.notices.is_empty(),
            "and it is not a warning either"
        );
    }

    #[test]
    fn the_gap_and_the_forced_edges_come_from_the_file() {
        let resolved = rules_from_toml(
            "[episode]\n\
             time_gap_minutes = 90\n\
             force_split = [\"subject:address review\", \"oid:1a2b3c\"]\n\
             force_merge = [\"subject:typo\"]\n",
        );
        assert_eq!(resolved.rules.gap_millis, 90 * 60_000);
        assert_eq!(
            resolved.rules.force_split.len(),
            2,
            "{:?}",
            resolved.rules.force_split
        );
        assert_eq!(
            resolved.rules.force_split[0].subject.as_deref(),
            Some("address review")
        );
        assert_eq!(resolved.rules.force_split[1].oid.as_deref(), Some("1a2b3c"));
        assert_eq!(
            resolved.rules.force_merge[0].subject.as_deref(),
            Some("typo")
        );
        assert!(
            resolved.notices.is_empty(),
            "nothing here was ignored: {:?}",
            resolved.notices
        );
    }

    #[test]
    fn a_pattern_that_is_not_one_of_the_two_spellings_is_named_not_dropped() {
        let resolved =
            rules_from_toml("[episode]\nforce_split = [\"src/store.rs\", \"subject:\"]\n");
        assert!(
            resolved.rules.force_split.is_empty(),
            "neither entry is usable, so neither became a rule"
        );
        assert_eq!(
            resolved.notices.len(),
            2,
            "and both were said out loud: {:?}",
            resolved.notices
        );
        assert!(resolved.notices[0].contains("src/store.rs"));
    }

    #[test]
    fn a_gap_that_cannot_be_a_gap_is_refused_rather_than_obeyed() {
        let resolved = rules_from_toml("[episode]\ntime_gap_minutes = 0\n");
        assert_eq!(resolved.rules.gap_millis, Rules::default().gap_millis);
        assert_eq!(resolved.notices.len(), 1);
        assert!(resolved.notices[0].contains("time_gap_minutes"));
    }

    #[test]
    fn a_repeated_default_boundary_order_is_not_a_warning() {
        let plain =
            rules_from_toml("[episode]\nboundaries = [\"hook\", \"trailer\", \"author\"]\n");
        assert!(
            plain.notices.is_empty(),
            "restating the default is not a claim: {:?}",
            plain.notices
        );
        let reordered = rules_from_toml("[episode]\nboundaries = [\"author\", \"hook\"]\n");
        assert_eq!(
            reordered.notices.len(),
            1,
            "asking for a different order is: {:?}",
            reordered.notices
        );
        assert!(reordered.notices[0].contains("reco-ubw"));
    }

    #[test]
    fn a_file_that_does_not_parse_says_so_instead_of_being_empty() {
        let resolved = rules_from_toml("[episode\n time_gap_minutes = ");
        assert_eq!(resolved.notices.len(), 1);
        assert!(resolved.notices[0].contains("does not parse"));
    }
}
