//! The graph policy: the numbers that bound the lane, and where they come from.
//!
//! ADR-004 §8.5 is explicit about ownership here: `graph.chain_full_every` and
//! `graph.query.max_chain_walk` "are configuration, defaulted so that `query.graph_dependents.ms`
//! holds, and **both belong to `s3`**", and "the two keys are registered as configuration by the
//! ticket that reads them (`e3.2`)". This is that ticket, so this is where they are read — and the
//! defaults are `s3`'s numbers, not a fresh opinion:
//!
//! * `graph.chain_full_every = 32` — the smallest anchor period whose measured bytes fit the budget
//!   in force at measurement time, and therefore the shortest chain walk on offer. Reconstruction
//!   latency is flat across the whole sweep (209.5 ms p95 at N = 8 against 204.6 ms at N = 32), so
//!   bytes are the only thing that discriminates and the policy keeps as few anchors as the budget
//!   tolerates.
//! * `graph.query.max_chain_walk = 32` — deliberately the *same number*: a chain longer than the
//!   anchor period means an anchor went missing, and a missing anchor is a stated partial, not a
//!   slower answer.
//!
//! A rejected value is noted and ignored, never fatal, exactly as [`crate::structure::policy`] does:
//! a typo in a config file must not be able to lose an architecture view.

use toml_edit::Item as TomlItem;
use toml_edit::Table as TomlTable;

/// Every `chain_full_every` revisions get an anchor. `s3`'s number, for `s3`'s reason — see the
/// module docs. `graph.chain_full_every`.
pub const DEFAULT_CHAIN_FULL_EVERY: usize = 32;

/// How far a chain walk may go before it declares a partial. The same number as the anchor period,
/// on purpose. `graph.query.max_chain_walk`.
pub const DEFAULT_MAX_CHAIN_WALK: usize = 32;

/// How far a path query walks before the answer is a stated partial rather than an answer. Six hops
/// is `s3`'s prototype's cap, and it is generous: a real architectural dependency path longer than
/// six modules is a finding about the layering, not a path a pane should draw.
/// `graph.query.path_max_hops`.
pub const DEFAULT_PATH_MAX_HOPS: usize = 6;

/// Directory names excluded from the graph entirely. `graph.ignored_dirs`, and the same default the
/// config schema ships.
pub const DEFAULT_IGNORED_DIRS: [&str; 1] = ["node_modules"];

/// How many materialised revision graphs the lane may hold, in MiB. `graph.snapshot_cache_mib`, and
/// the ceiling ADR-004 §11 calls the per-analyzer working set when it is measured instead of assumed.
///
/// The number is small on purpose: `s3` measured 22 335 B of *stored* bytes per revision, and a
/// materialised graph is the folded form of that plus two adjacency indexes, so a few dozen revisions
/// fill a few megabytes. A snapshot that does not fit is re-materialised from the store on next use —
/// the durable delta chain is the spill — which is the 0.381 ms-to-204.6 ms difference `step` already
/// reports, paid again, rather than an unbounded resident set.
pub const DEFAULT_SNAPSHOT_CACHE_MIB: u64 = 32;

/// The knobs the graph lane runs under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPolicy {
    /// `graph.chain_full_every`.
    pub chain_full_every: usize,
    /// `graph.query.max_chain_walk`.
    pub max_chain_walk: usize,
    /// `graph.query.path_max_hops`.
    pub path_max_hops: usize,
    /// `graph.ignored_dirs`.
    pub ignored_dirs: Vec<String>,
    /// `graph.snapshot_cache_mib` — the byte ceiling on the lane's materialised-snapshot cache.
    pub snapshot_cache_bytes: u64,
}

impl Default for GraphPolicy {
    fn default() -> Self {
        Self {
            chain_full_every: DEFAULT_CHAIN_FULL_EVERY,
            max_chain_walk: DEFAULT_MAX_CHAIN_WALK,
            path_max_hops: DEFAULT_PATH_MAX_HOPS,
            ignored_dirs: DEFAULT_IGNORED_DIRS
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            snapshot_cache_bytes: DEFAULT_SNAPSHOT_CACHE_MIB * fathom_core::memory::MIB,
        }
    }
}

impl GraphPolicy {
    /// Read the `[graph]` table out of a `fathom.toml` document, alongside the notices a surface
    /// should print about anything it refused.
    #[must_use]
    pub fn from_repo_config_text(text: &str) -> (Self, Vec<String>) {
        let mut policy = Self::default();
        let mut notices = Vec::new();
        let document = match text.parse::<toml_edit::DocumentMut>() {
            Ok(document) => document,
            Err(err) => {
                notices.push(format!(
                    "fathom.toml does not parse, so every graph knob stayed at its built-in \
                     default: {err}"
                ));
                return (policy, notices);
            }
        };
        let Some(table) = document.get("graph").and_then(TomlItem::as_table) else {
            return (policy, notices);
        };
        for (path, slot) in [
            ("chain_full_every", Slot(&mut policy.chain_full_every)),
            ("query.max_chain_walk", Slot(&mut policy.max_chain_walk)),
            ("query.path_max_hops", Slot(&mut policy.path_max_hops)),
        ] {
            let Some(value) = knob(table, path).and_then(TomlItem::as_value) else {
                continue;
            };
            let Some(number) = value.as_integer() else {
                notices.push(format!(
                    "graph.{path} is not a whole number, so it was not used"
                ));
                continue;
            };
            if number < 1 {
                notices.push(format!(
                    "graph.{path} is {number}, which would bound the lane to nothing; kept the \
                     built-in default"
                ));
                continue;
            }
            *slot.0 = usize::try_from(number).unwrap_or(usize::MAX);
        }
        // `graph.snapshot_cache_mib` is the one byte-valued knob here, so it is read on its own rather
        // than through the `usize` slots above: a memory ceiling has a floor (one MiB) and a roof
        // (`MAX_CEILING_BYTES`) that the shared clamp owns, and the notice has to say which bit.
        if let Some(value) = knob(table, "snapshot_cache_mib").and_then(TomlItem::as_value)
            && let Some(number) = value.as_integer()
        {
            let mut ceilings = fathom_core::memory::Ceilings::default();
            match ceilings.set("snapshot_cache_mib", number) {
                Ok(notice) => {
                    policy.snapshot_cache_bytes = ceilings.graph_snapshot_bytes;
                    if let Some(line) = notice {
                        notices.push(format!("graph.{line}"));
                    }
                }
                Err(err) => notices.push(format!("graph.snapshot_cache_mib: {err}")),
            }
        }
        if let Some(list) = table
            .get("ignored_dirs")
            .and_then(TomlItem::as_value)
            .and_then(|value| value.as_array())
        {
            let mut names = Vec::new();
            for entry in list.iter() {
                if let Some(name) = entry.as_str() {
                    names.push(name.to_string());
                } else {
                    notices.push(
                        "graph.ignored_dirs has an entry that is not a string; it was skipped"
                            .to_string(),
                    );
                }
            }
            if names.is_empty() {
                notices.push(
                    "graph.ignored_dirs is empty or unreadable, so the built-in list stayed"
                        .to_string(),
                );
            } else {
                // The schema declares `graph.ignored_dirs` as `merge = "extend"`: a repository that
                // names one more vendor directory has not asked to un-inherit `node_modules`.
                for name in names {
                    if !policy.ignored_dirs.contains(&name) {
                        policy.ignored_dirs.push(name);
                    }
                }
            }
        }
        (policy, notices)
    }

    /// Whether the path a file lives at is inside one of `graph.ignored_dirs`.
    ///
    /// A name, not a prefix: `node_modules` matches `web/node_modules/x.js` at any depth, which is
    /// what "excluded from the graph entirely" means for a vendored tree.
    #[must_use]
    pub fn ignores(&self, path: &str) -> bool {
        path.split('/').any(|segment| {
            self.ignored_dirs
                .iter()
                .any(|dir| dir == segment || segment.starts_with(dir.as_str()))
        })
    }

    /// What the two chain knobs imply for each other, as notices.
    ///
    /// `s3`'s policy is that the cap and the anchor period are the same number *because* a longer
    /// walk is evidence of a lost anchor. A config that sets the cap above the period turns the
    /// tripwire off; a config that sets it below makes ordinary intact chains partial. Both are
    /// worth saying out loud rather than discovering as a wrong answer.
    #[must_use]
    pub fn notices(&self) -> Vec<String> {
        let mut notices = Vec::new();
        if self.max_chain_walk > self.chain_full_every {
            notices.push(format!(
                "graph.query.max_chain_walk ({}) is above graph.chain_full_every ({}): a chain \
                 longer than the anchor period is a lost anchor, and the cap will not notice",
                self.max_chain_walk, self.chain_full_every
            ));
        }
        if self.max_chain_walk < self.chain_full_every {
            notices.push(format!(
                "graph.query.max_chain_walk ({}) is below graph.chain_full_every ({}): intact \
                 chains will answer as partials",
                self.max_chain_walk, self.chain_full_every
            ));
        }
        notices
    }
}

/// One integer knob, so a dotted path and a plain key write through the same checked path.
struct Slot<'a>(&'a mut usize);

/// Follow a dotted key path inside a TOML table, so `query.max_chain_walk` reads as one knob.
fn knob<'a>(table: &'a TomlTable, path: &str) -> Option<&'a TomlItem> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut item: &TomlItem = table.get(first)?;
    for part in parts {
        item = item.get(part)?;
    }
    Some(item)
}
