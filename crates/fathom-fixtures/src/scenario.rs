//! `scenario.rs` — the corpus: its names, their builders, and their ground truth.
//!
//! One row per scenario, and the row is all the registry needs: a name, the function that builds it, and the
//! ground-truth file that says what it means. `docs/concepts/fixtures.md` is the prose walk-through of
//! adding one; in code it is that same triple.
//!
//! The ground-truth text is `include_str!`-ed rather than read at run time, for one reason: a scenario whose
//! ground truth is missing is then a compile error instead of a test that passes by having nothing to say.

use std::path::Path;

use crate::script::Script;
use crate::truth::GroundTruth;

/// Every scenario the corpus ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// A module tree renamed twice in one turn, and a third rename that changed content on the way.
    RenameStorm,
    /// Direct, transitive, added, removed and lockfile-only dependency changes, one per turn.
    DependencyBumpStorm,
    /// A clean three-layer project in which one commit closes a cycle, and a later one opens a second.
    CycleIntroduced,
    /// Tests deleted, weakened to `#[ignore]`, and put back.
    TestDeletion,
    /// Reformat-only, reorder-only and lockfile-only churn, plus one episode that is only partly noise.
    ReformatOnly,
    /// An episode that touched a great deal more than the intent its trailer declared.
    ScopeCreep,
    /// The same change twice: the original chain and its rewritten self, kept side by side as two refs.
    RebaseRewrite,
    /// The same region of the same two files argued over across six turns, then reverted.
    ChurnLoop,
    /// A 9 MiB generated file, a 600 KB bundle, a binary blob, and a hand edit to the generated one.
    HugeFile,
    /// Paths that are not ASCII, and a rename between two of them.
    UnicodePaths,
    /// Five branches, five merges, one resolved conflict, one branch nobody merged.
    ManyMerges,
    /// Ten thousand commits in four hundred turns, for the performance budget and the scale cases.
    TenKCommits,
    /// A submodule: its own history, a gitlink, and a gitlink bump.
    Submodule,
    /// A depth-3 clone of a history whose ancestors are genuinely absent.
    ShallowClone,
    /// Nothing wrong with it at all: the control every detector is expected to stay quiet on.
    CleanBaseline,
}

impl Scenario {
    /// Every scenario, in the order the corpus lists and materialises them.
    pub const ALL: &'static [Scenario] = &[
        Scenario::RenameStorm,
        Scenario::DependencyBumpStorm,
        Scenario::CycleIntroduced,
        Scenario::TestDeletion,
        Scenario::ReformatOnly,
        Scenario::ScopeCreep,
        Scenario::RebaseRewrite,
        Scenario::ChurnLoop,
        Scenario::HugeFile,
        Scenario::UnicodePaths,
        Scenario::ManyMerges,
        Scenario::TenKCommits,
        Scenario::Submodule,
        Scenario::ShallowClone,
        Scenario::CleanBaseline,
    ];

    /// The scenario's name, which is its `--scenario` spelling, its ground-truth file's stem, and the name
    /// its directory gets. One string, three uses, no chance to disagree.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Scenario::RenameStorm => "rename_storm",
            Scenario::DependencyBumpStorm => "dependency_bump_storm",
            Scenario::CycleIntroduced => "cycle_introduced",
            Scenario::TestDeletion => "test_deletion",
            Scenario::ReformatOnly => "reformat_only",
            Scenario::ScopeCreep => "scope_creep",
            Scenario::RebaseRewrite => "rebase_rewrite",
            Scenario::ChurnLoop => "churn_loop",
            Scenario::HugeFile => "huge_file",
            Scenario::UnicodePaths => "unicode_paths",
            Scenario::ManyMerges => "many_merges",
            Scenario::TenKCommits => "ten_k_commits",
            Scenario::Submodule => "submodule",
            Scenario::ShallowClone => "shallow_clone",
            Scenario::CleanBaseline => "clean_baseline",
        }
    }

    /// The scenario a `--scenario` argument names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Scenario> {
        Scenario::ALL
            .iter()
            .copied()
            .find(|scenario| scenario.name() == name)
    }

    /// The scenario's history, as data. See `crate::scenarios`.
    #[must_use]
    pub fn script(self) -> Script {
        match self {
            Scenario::RenameStorm => crate::scenarios::rename_storm(),
            Scenario::DependencyBumpStorm => crate::scenarios::dependency_bump_storm(),
            Scenario::CycleIntroduced => crate::scenarios::cycle_introduced(),
            Scenario::TestDeletion => crate::scenarios::test_deletion(),
            Scenario::ReformatOnly => crate::scenarios::reformat_only(),
            Scenario::ScopeCreep => crate::scenarios::scope_creep(),
            Scenario::RebaseRewrite => crate::scenarios::rebase_rewrite(),
            Scenario::ChurnLoop => crate::scenarios::churn_loop(),
            Scenario::HugeFile => crate::scenarios::huge_file(),
            Scenario::UnicodePaths => crate::scenarios::unicode_paths(),
            Scenario::ManyMerges => crate::scenarios::many_merges(),
            Scenario::TenKCommits => crate::scenarios::ten_k_commits(),
            Scenario::Submodule => crate::scenarios::submodule(),
            Scenario::ShallowClone => crate::scenarios::shallow_clone(),
            Scenario::CleanBaseline => crate::scenarios::clean_baseline(),
        }
    }

    /// The checked-in ground-truth text.
    #[must_use]
    pub const fn ground_truth_text(self) -> &'static str {
        match self {
            Scenario::RenameStorm => include_str!("../corpus/rename_storm.json"),
            Scenario::DependencyBumpStorm => include_str!("../corpus/dependency_bump_storm.json"),
            Scenario::CycleIntroduced => include_str!("../corpus/cycle_introduced.json"),
            Scenario::TestDeletion => include_str!("../corpus/test_deletion.json"),
            Scenario::ReformatOnly => include_str!("../corpus/reformat_only.json"),
            Scenario::ScopeCreep => include_str!("../corpus/scope_creep.json"),
            Scenario::RebaseRewrite => include_str!("../corpus/rebase_rewrite.json"),
            Scenario::ChurnLoop => include_str!("../corpus/churn_loop.json"),
            Scenario::HugeFile => include_str!("../corpus/huge_file.json"),
            Scenario::UnicodePaths => include_str!("../corpus/unicode_paths.json"),
            Scenario::ManyMerges => include_str!("../corpus/many_merges.json"),
            Scenario::TenKCommits => include_str!("../corpus/ten_k_commits.json"),
            Scenario::Submodule => include_str!("../corpus/submodule.json"),
            Scenario::ShallowClone => include_str!("../corpus/shallow_clone.json"),
            Scenario::CleanBaseline => include_str!("../corpus/clean_baseline.json"),
        }
    }

    /// The parsed ground truth.
    ///
    /// # Errors
    ///
    /// [`crate::truth::Error::Truth`] when the file is not valid, or names a schema this build does not read.
    pub fn ground_truth(self) -> crate::truth::Result<GroundTruth> {
        GroundTruth::parse(
            self.ground_truth_text(),
            &format!("corpus/{}.json", self.name()),
        )
    }

    /// Where this scenario's ground-truth file lives in the source tree, for `--bless`.
    ///
    /// `CARGO_MANIFEST_DIR` is where a build-time tool finds the source of a crate that is being run from a
    /// build directory; it is exactly the thing a *fixture* may not read (its own repository must not depend
    /// on where it was built), which is why this is a path helper and not part of the generated bytes.
    #[must_use]
    pub fn ground_truth_path(self) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("corpus")
            .join(format!("{}.json", self.name()))
    }
}

impl Scenario {
    /// Materialise this scenario into `dest`, which is removed first if it exists.
    ///
    /// This is the entry point the consuming tickets use — `Scenario::CycleIntroduced.generate(dir)?` — and
    /// the reason it lives on the scenario rather than on the generator is that a test should read like the
    /// corpus: a named scenario, a directory, and the ground truth that came with it.
    ///
    /// # Errors
    ///
    /// [`crate::Error`] when the filesystem refuses, or when the ground-truth file cannot be read.
    pub fn generate(self, dest: &Path) -> crate::Result<crate::Fixture> {
        crate::generate::generate(self, dest)
    }
}

impl std::fmt::Display for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for Scenario {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Scenario::from_name(s.trim())
            .ok_or_else(|| format!("no scenario `{s}`; try one of {}", names().join(", ")))
    }
}

/// Every scenario name, sorted the way the corpus sorts everything.
#[must_use]
pub fn names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Scenario::ALL.iter().copied().map(Scenario::name).collect();
    crate::policy::sort_in_corpus_order(&mut names);
    names
}
