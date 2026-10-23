//! `memory.rs` — the run's memory ceilings, resolved from the layers (ADR-002 §4.5, `reco-93w`).
//!
//! One job: turn `fathom.toml`'s budget keys into [`fathom_core::memory::Ceilings`] values and hand them
//! downhill. No crate below `fathom-cli` opens the file, so this is the only place the numbers are
//! read, and the only place a repository can attempt to make this process allocate.
//!
//! # Which key is which ceiling, and why those
//!
//! The keys are ADR-004 §11's, restated — not a new vocabulary. Every one of them already existed as a
//! budget name; what `reco-93w` adds is that they now *bind* something:
//!
//! | Key | Ceiling | Enforced by |
//! |---|---|---|
//! | `rss.analyzer_working_set.mib` | the in-process analysis payload cache | `Store::set_ceilings`, `BoundedCache` |
//! | `graph.snapshot_cache_mib` | materialised revision graphs held by one lane | `graph::Lane`'s snapshot cache |
//! | `ingest.size_cap_mib` | one decode buffer | `normalize::Limits::blob_memory_cap` |
//! | `write.queue_batches` | batches in flight between normalize and write | `ingest::run`'s hand-off |
//! | `cache.budget.mib` | the durable `cache` table | `Store::cache_enforce` (ADR-004 §8.6) |
//! | `cache.protect_recent_minutes` | §8.6's protection set | the same |
//!
//! Env wins over the file, like every other layer (`FATHOM_WRITE__QUEUE_BATCHES`, and friends), because
//! the CI job that needs a smaller run must not have to edit a repository it does not own.
//!
//! # Hostile input
//!
//! A cloned `fathom.toml` is reviewed like code and treated like an untrusted event. A ceiling is no
//! exception: values outside what this build honours are clamped and the clamp is *named* in the notices
//! this module returns, which `fathom index` prints and `fathom config explain` would too. Refusing to
//! start would be worse — a repository that cannot index because its budget is silly helps nobody — and
//! silently obeying it would be the one outcome where a clone can take a runner down.

use fathom_core::memory::{Ceilings, MIB};

/// What was read, and what could not be honoured.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    /// The ceilings in force for this process.
    pub ceilings: Ceilings,
    /// One line per key that was read and changed on the way in. Never silently empty, never silently
    /// ignored.
    pub notices: Vec<String>,
    /// One line per key this module looked for and did not find, in the shape `fathom config explain`
    /// wants: the default that won, named, so "the knob did nothing" and "the knob was never turned"
    /// are different sentences.
    pub defaults_used: Vec<String>,
}

/// The `(config key, ceiling slot)` pairs this module reads, in the order they are documented.
///
/// The left column is the key **as the key spec ships it** — `cache.budget.mib`, not a flattened
/// `cache.budget_mib` — because a knob that is read under a name nobody can write is a knob that is
/// silently ignored, which is the one failure this module's own notes call out. The right column is
/// `Ceilings::set`'s slot name, which is flatter because `mib` alone would be ambiguous.
const KEYS: [(&str, &str); 6] = [
    ("rss.analyzer_working_set.mib", "analyzer_working_set_mib"),
    ("graph.snapshot_cache_mib", "snapshot_cache_mib"),
    ("ingest.size_cap_mib", "size_cap_mib"),
    ("write.queue_batches", "queue_batches"),
    ("cache.budget.mib", "cache_budget_mib"),
    ("cache.protect_recent_minutes", "protect_recent_minutes"),
];

/// The environment variable that names a dotted key: segments joined by `__`, like every other key
/// (`cache.budget.mib` → `FATHOM_CACHE__BUDGET__MIB`).
fn env_name(key: &str) -> String {
    format!("FATHOM_{}", key.to_uppercase().replace('.', "__"))
}

/// Follow a dotted key through the document's tables: `cache.budget.mib` is `mib` inside the `budget`
/// table inside the `cache` table.
fn lookup(document: &toml_edit::DocumentMut, key: &str) -> Option<i64> {
    let mut parts = key.split('.').peekable();
    let mut table = document.as_table();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return table.get(part)?.as_value()?.as_integer();
        }
        table = table.get(part)?.as_table()?;
    }
    None
}

/// Resolve the ceilings for one repository: the built-in defaults, then the file, then the environment.
#[must_use]
pub fn resolve(root: &std::path::Path) -> Resolved {
    resolve_text(
        &std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default(),
        &std::env::vars().collect::<Vec<_>>(),
    )
}

/// The same resolution with both inputs supplied, so a test (and `fathom config explain`) can ask what
/// a document means without being the repository it is describing.
#[must_use]
pub fn resolve_text(text: &str, env: &[(String, String)]) -> Resolved {
    let mut out = Resolved {
        ceilings: Ceilings::default(),
        notices: Vec::new(),
        defaults_used: Vec::new(),
    };
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => Some(document),
        Err(err) => {
            out.notices.push(format!(
                "fathom.toml does not parse, so every memory ceiling is a default: {err}"
            ));
            None
        }
    };
    for (dotted, slot) in KEYS {
        // Layer order, highest first: env, then the repository's file. A flag for these exists in
        // `--set` form already and is applied by whoever parses it, not here.
        let wanted = env_name(dotted);
        let from_env = env
            .iter()
            .find(|(name, _)| name == &wanted)
            .and_then(|(_, value)| value.parse::<i64>().ok());
        let from_file = document
            .as_ref()
            .and_then(|document| lookup(document, dotted));
        let Some(value) = from_env.or(from_file) else {
            out.defaults_used
                .push(format!("{dotted} came from the built-in default"));
            continue;
        };
        match out.ceilings.set(slot, value) {
            Ok(Some(notice)) => out.notices.push(format!("{dotted}: {notice}")),
            Ok(None) => {}
            Err(err) => out.notices.push(format!("{dotted}: {err}")),
        }
    }
    out
}

/// The ceilings as lines for a status screen: the number, and the key that set it.
///
/// Unindented on purpose: the section that prints them owns its own indentation, so a caller that
/// nests this under a heading gets one column of indent rather than two.
#[must_use]
pub fn lines(ceilings: &Ceilings) -> Vec<String> {
    vec![
        format!(
            "working set {:>5} MiB   analysis payloads held in memory (rss.analyzer_working_set.mib)",
            ceilings.analysis_cache_bytes / MIB
        ),
        format!(
            "snapshots   {:>5} MiB   materialised graphs held in memory (graph.snapshot_cache_mib)",
            ceilings.graph_snapshot_bytes / MIB
        ),
        format!(
            "decode cap  {:>5} MiB   one blob inflated at a time (ingest.size_cap_mib)",
            ceilings.decode_bytes / MIB
        ),
        format!(
            "in flight   {:>5}       batches between normalize and write (write.queue_batches)",
            ceilings.queue_batches
        ),
        format!(
            "cache       {:>5} MiB   durable cache budget (cache.budget.mib)",
            ceilings.durable_cache_bytes / MIB
        ),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn an_absent_file_leaves_every_ceiling_at_its_default_and_says_so() {
        let resolved = resolve_text("", &[]);
        assert_eq!(resolved.ceilings, Ceilings::default());
        assert!(resolved.notices.is_empty(), "{:?}", resolved.notices);
        assert_eq!(
            resolved.defaults_used.len(),
            KEYS.len(),
            "every ceiling that was not set has to say which default won"
        );
    }

    #[test]
    fn a_file_moves_the_ceilings_it_names_and_only_those() {
        let resolved = resolve_text(
            "[graph]\nsnapshot_cache_mib = 4\n[write]\nqueue_batches = 7\n",
            &[],
        );
        assert_eq!(resolved.ceilings.graph_snapshot_bytes, 4 * MIB);
        assert_eq!(resolved.ceilings.queue_batches, 7);
        assert_eq!(
            resolved.ceilings.analysis_cache_bytes,
            Ceilings::default().analysis_cache_bytes
        );
        assert_eq!(resolved.defaults_used.len(), KEYS.len() - 2);
    }

    #[test]
    fn a_nested_key_is_read_under_the_name_the_key_spec_ships() {
        // `cache.budget.mib` and `rss.analyzer_working_set.mib` are tables all the way down, and this
        // resolver is the code that turns the file into a budget. Read them under a flattened name and
        // the knob is not broken, it is *unheard* — `fathom stats` would print a default while a
        // repository insists it set 64. So the spelling here is the spec's, dotted, table by table.
        let resolved = resolve_text(
            "[cache.budget]\nmib = 64\n[rss.analyzer_working_set]\nmib = 96\n",
            &[],
        );
        assert_eq!(resolved.ceilings.durable_cache_bytes, 64 * MIB);
        assert_eq!(resolved.ceilings.analysis_cache_bytes, 96 * MIB);
        assert!(resolved.notices.is_empty(), "{:?}", resolved.notices);
        assert_eq!(resolved.defaults_used.len(), KEYS.len() - 2);
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        // The variable name is the key with `__` for a separator, which is the layer table's rule and is
        // what `FATHOM_CACHE__BUDGET__MIB` in `docs/concepts/config.md` promises.
        let resolved = resolve_text(
            "[cache.budget]\nmib = 64\n",
            &[("FATHOM_CACHE__BUDGET__MIB".to_owned(), "128".to_owned())],
        );
        assert_eq!(resolved.ceilings.durable_cache_bytes, 128 * MIB);
    }

    #[test]
    fn a_repository_cannot_ask_for_more_than_this_build_will_honour() {
        let resolved = resolve_text("[rss.analyzer_working_set]\nmib = 999999999\n", &[]);
        assert!(
            resolved.ceilings.analysis_cache_bytes <= fathom_core::memory::MAX_CEILING_BYTES,
            "the clamp did not happen"
        );
        assert_eq!(resolved.notices.len(), 1, "{:?}", resolved.notices);
        assert!(
            resolved.notices[0].contains("capped"),
            "{:?}",
            resolved.notices
        );
    }

    #[test]
    fn a_key_of_the_wrong_shape_is_reported_not_applied() {
        let resolved = resolve_text("[write]\nqueue_batches = 0\n", &[]);
        assert_eq!(
            resolved.ceilings.queue_batches,
            Ceilings::default().queue_batches
        );
        assert_eq!(resolved.notices.len(), 1, "{:?}", resolved.notices);
    }

    #[test]
    fn the_lines_name_the_key_a_person_has_to_edit() {
        let printed = lines(&Ceilings::default()).join("\n");
        for key in [
            "rss.analyzer_working_set.mib",
            "graph.snapshot_cache_mib",
            "ingest.size_cap_mib",
            "write.queue_batches",
            "cache.budget.mib",
        ] {
            assert!(printed.contains(key), "{key} is not printed");
        }
    }
}
