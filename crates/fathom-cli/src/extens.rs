//! The `--detectors` / `--events` flags, resolved once (`e6.7`, `reco-raw`).
//!
//! Four global flags, one home. They are global because "which detectors run" and "is anything listening"
//! are properties of the run, not of the verb: `fathom why --disable-detector secret_string` and
//! `fathom export --disable-detector secret_string` have to mean the same thing, and a per-verb flag is
//! four places to forget.
//!
//! The resolution order is the config order the whole product follows (`docs/concepts/config.md` §3):
//! the built-in default set, then the repository's `fathom.toml`, then the command line. The last step is
//! [`Extensibility::apply_to`], and it exists so that a verb which has already read `fathom.toml` for
//! another reason can apply the flags to the text it read instead of reading the file twice.
//!
//! Nothing here reads a file. The verbs own their reads (ADR-002 §4.5); this owns the answer to "what did
//! the reviewer ask for on the command line".

use std::path::Path;

use fathom_analyze::detectors::policy::{Resolved, Selection, resolve};

/// What the command line asked for.
#[derive(Debug, Clone, Default)]
pub struct Extensibility {
    /// The detector selection the three detector flags resolved to.
    pub selection: Selection,
    /// `--events <file>`: write this run's event stream to `file`, whatever the repository's config says.
    pub events: Option<String>,
}

impl Extensibility {
    /// Resolve `--detectors`, `--enable-detector`, `--disable-detector` and `--events`.
    #[must_use]
    pub fn new(
        specs: &[String],
        enabled: &[String],
        disabled: &[String],
        events: Option<String>,
    ) -> Self {
        let selection = Selection::from_flags(specs, enabled, disabled);
        Extensibility { selection, events }
    }

    /// Whether the reviewer said nothing, which is the case in which no answer changes.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.selection.is_empty() && self.events.is_none()
    }

    /// Apply the flags to `fathom.toml` text, producing the policy a scan should run.
    #[must_use]
    pub fn apply_to(&self, text: &str) -> Resolved {
        resolve(text, &self.selection)
    }

    /// Read the repository's `fathom.toml` and apply the flags. A repository with no file gets the
    /// defaults, which is the same answer every other analysis gives.
    #[must_use]
    pub fn policy_for(&self, root: &Path) -> Resolved {
        let text = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();
        self.apply_to(&text)
    }

    /// What the flags did, in one line, for a report that prints its knobs.
    #[must_use]
    pub fn describe(&self) -> String {
        self.selection.describe()
    }

    /// One line per namespaced name the flags named, for a build that links no plugin detectors.
    ///
    /// The honest answer is not silence. A plugin detector runs inside the binary that registered it, and
    /// `fathom` registers none — the out-of-tree example reaches the same findings by reading
    /// `fathom analyze --json` instead (see `plugins/README.md`). Saying that beats a run that looks like it
    /// ran the detector and found nothing (`e6.7`).
    #[must_use]
    pub fn plugin_notices(&self) -> Vec<String> {
        self.selection
            .namespaced()
            .into_iter()
            .map(|id| {
                format!(
                    "`{id}` names a plugin detector, and this build links none: a plugin runs in the \
                     binary that registers it. `fathom analyze --json | <plugin>` is the way to run one \
                     against this repository"
                )
            })
            .collect()
    }
}
