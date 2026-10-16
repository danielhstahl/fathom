//! `generate.rs` — turn a [`Script`] into a repository on disk, and say what came out.
//!
//! One pass, in creation order: for each step, take the branch's tree, apply the step's ops, write the
//! blobs and the tree and the commit, move the ref, and record what happened. The record is the
//! `fixture.json` the generated directory carries, and it is the contract every consumer reads: the commit
//! ids in walk order, who wrote each one, which turn named it, which paths it moved, which refs exist.
//!
//! Two things are deliberately produced here rather than left to the consumer:
//!
//! * **The changed-path list per commit.** It is derived from the two tree states the generator already has,
//!   so it is exactly right and costs nothing. A consumer that recomputed it would be re-implementing part
//!   of `fathom-ingest` inside a fixture crate, which is the mistake ADR-002 §4.4 exists to prevent.
//! * **The digests.** Over every object written, over the checked-out work tree, and over the harness event
//!   log. Those three numbers are what "byte-identical on all three OSes" means when it is checked on
//!   someone else's machine instead of argued about.
//!
//! Line counts in `fixture.json` are a multiset difference over lines, not a diff algorithm: they answer
//! "did this commit delete the two tests the ground truth says it deleted", and they are explicitly *not*
//! the numbers `fathom-ingest` computes for the product, which come from the real diff engine.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::events::{self, EventsSummary};
use crate::git::{self, CommitSpec, File, GitDir};
use crate::policy::{self, Kind};
use crate::scenario::Scenario;
use crate::script::{self as sc, Companion, Extra, Gitlink, Location, Op, Script};
use crate::truth::GroundTruth;
use crate::verify;
use crate::{Error, Result};

/// A materialised scenario: the repository, the record of it, and whether it agrees with its ground truth.
#[derive(Debug)]
pub struct Fixture {
    /// The scenario that was built.
    pub scenario: Scenario,
    /// The directory the repository was written to.
    pub path: PathBuf,
    /// What was produced, exactly as written to `fixture.json`.
    pub manifest: Manifest,
    /// The ground truth it was checked against.
    pub ground_truth: GroundTruth,
    /// Every way the produced repository disagrees with its ground truth. Empty is the pass.
    pub failures: Vec<String>,
}

impl Fixture {
    /// Whether the fixture matched its ground truth.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The `fixture.json` document.
#[derive(Debug, Serialize)]
pub struct Manifest {
    /// See [`crate::policy::FIXTURE_SCHEMA`].
    pub schema: &'static str,
    /// The ticket that owns this corpus.
    pub generated_by: &'static str,
    /// Which scenario this is.
    pub scenario: &'static str,
    /// What the scenario is for, copied from the ground truth so the directory explains itself.
    pub purpose: String,
    /// The plan keys that consume it.
    pub consumers: Vec<String>,
    /// Where it was written. Not part of any digest: a fixture is portable, a path is not.
    pub destination: String,
    /// The epoch every commit date is measured from.
    pub base_epoch: i64,
    /// The branch `HEAD` is on.
    pub default_branch: String,
    /// The default branch's tip.
    pub head: String,
    /// Every ref in the repository, sorted.
    pub tips: BTreeMap<String, String>,
    /// Commits the fixture shows.
    pub commit_count: usize,
    /// The commits, in creation order — the canonical walk order (see [`Script::steps`]).
    pub commits: Vec<CommitEntry>,
    /// Branches, with the commit index each tip names.
    pub branches: Vec<RefEntry>,
    /// Tags.
    pub tags: Vec<RefEntry>,
    /// Companion repositories written alongside this one, if any.
    pub companions: Vec<CompanionEntry>,
    /// The harness event log.
    pub events: EventsSummary,
    /// Which commits `.git/shallow` names, empty for a repository that is not shallow.
    ///
    /// A consumer needs this rather than the file, because `.git/shallow` is a git implementation detail and
    /// this is the contract: an ingest that has to stop at a boundary needs to be told there is one, and a
    /// test that greps `.git/` is a test that breaks when git changes its mind.
    pub shallow: Vec<String>,
    /// Every object written.
    pub objects: DigestCount,
    /// The checked-out work tree.
    pub worktree: DigestCount,
    /// The digest over all three, and the number `--bless` stamps into the ground truth.
    pub digest: String,
    /// Whether the ground truth agrees, and if not, in which ways.
    pub verification: Verification,
    /// The ground truth itself, so a consumer needs one file.
    pub ground_truth: GroundTruth,
}

/// A count plus a digest over the things counted.
#[derive(Debug, Serialize)]
pub struct DigestCount {
    /// How many things were hashed.
    pub count: usize,
    /// Their digest.
    pub digest: String,
}

/// A named ref, and which commit index it names.
#[derive(Debug, Serialize)]
pub struct RefEntry {
    /// The ref name, e.g. `refs/heads/main`.
    pub r#ref: String,
    /// The commit index it points at, or `None` when it points at something that is not a commit of this
    /// fixture (a tag object, or a companion's commit).
    pub commit: Option<usize>,
    /// The sha.
    pub sha: String,
}

/// A companion repository, and where it went.
#[derive(Debug, Serialize)]
pub struct CompanionEntry {
    /// The name the script gave it.
    pub name: String,
    /// Where its git dir is, relative to the fixture (or absolute for a sibling).
    pub git_dir: String,
    /// Where its work tree is, relative to the fixture.
    pub work_tree: String,
    /// Its tip.
    pub head: String,
    /// Its commits, in creation order.
    pub commits: Vec<String>,
}

/// The verification result, as recorded in the manifest.
#[derive(Debug, Serialize)]
pub struct Verification {
    /// Whether it passed.
    pub ok: bool,
    /// Each disagreement, in the order found.
    pub failures: Vec<String>,
}

/// One commit, as generated.
#[derive(Debug, Clone, Serialize)]
pub struct CommitEntry {
    /// The creation index, which is the canonical walk order.
    pub index: usize,
    /// The commit id.
    pub sha: String,
    /// The branch this commit is the tip of.
    pub branch: String,
    /// Parent shas, first parent first.
    pub parents: Vec<String>,
    /// The committer key from [`crate::policy`].
    pub author: &'static str,
    /// `agent`, `human` or `bot`.
    pub author_kind: &'static str,
    /// Seconds after [`crate::policy::BASE_EPOCH`].
    pub at: i64,
    /// The same instant as an ISO-8601 UTC string.
    pub timestamp: String,
    /// The subject line.
    pub subject: String,
    /// The harness session, when a trailer named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The harness turn, when a trailer named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    /// The trailers git parses from the message, lower-cased keys.
    pub trailers: BTreeMap<String, String>,
    /// Whether this commit has more than one parent.
    pub merge: bool,
    /// What it changed.
    pub changes: Vec<Change>,
    /// Files touched.
    pub n_files: usize,
    /// Lines added — see the module docs for what this count is.
    pub n_add: usize,
    /// Lines removed.
    pub n_del: usize,
    /// Binary files touched.
    pub n_binary: usize,
    /// Whether the commit's tree equals its first parent's.
    pub empty: bool,
    /// Tags pointing at this commit.
    pub tags: Vec<String>,
    /// Gitlinks this commit added or moved.
    pub gitlinks: Vec<GitlinkEntry>,
}

/// A gitlink recorded on a commit.
#[derive(Debug, Clone, Serialize)]
pub struct GitlinkEntry {
    /// The mount point.
    pub path: String,
    /// The commit the gitlink names.
    pub sha: String,
    /// Which companion owns that commit.
    pub companion: String,
}

/// One path a commit changed.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// The path.
    pub path: String,
    /// `added`, `deleted`, `modified` or `renamed`.
    pub change: &'static str,
    /// For a rename, where it came from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Lines added.
    pub adds: usize,
    /// Lines removed.
    pub dels: usize,
    /// Bytes now.
    pub bytes: usize,
    /// Whether it reads as binary.
    pub binary: bool,
}

/// One branch's state while building: its tip, its tree, and where it forked.
#[derive(Clone)]
struct BranchState {
    head: Option<String>,
    files: BTreeMap<String, File>,
    fork: BTreeMap<String, File>,
}

/// A commit's recipe, kept so a later structure (a shallow clone) can reproduce exactly these objects.
///
/// Who signed it, when it was dated, what it said, and whom it points at: the four things that are not
/// re-derivable from the tree state, and which together fix the commit's id.
#[derive(Clone)]
pub(crate) struct Recipe {
    pub(crate) identity: &'static str,
    pub(crate) at: i64,
    pub(crate) message: String,
    pub(crate) parents: Vec<String>,
}

/// A script built into a directory.
#[derive(Clone)]
pub(crate) struct Build {
    /// The commits, in creation order.
    pub entries: Vec<CommitEntry>,
    /// The tree state after each commit, indexed like `entries`.
    pub(crate) maps: Vec<BTreeMap<String, File>>,
    /// The recipes, indexed like `entries`.
    pub(crate) recipes: Vec<Recipe>,
    /// Every ref written, sorted.
    pub tips: BTreeMap<String, String>,
    /// Which commit index each ref names, when it names one.
    pub(crate) tip_indices: BTreeMap<String, usize>,
    /// Companion builds by name.
    pub companions: BTreeMap<String, CompanionBuild>,
    /// The commits the fixture shows: all of them, except in a shallow clone.
    pub(crate) visible: Vec<usize>,
    /// The default branch.
    pub default_branch: String,
    /// Whether `.git/shallow` was written, and with what.
    pub(crate) shallow: Vec<String>,
}

impl Build {
    /// The commits the fixture shows, in walk order.
    pub(crate) fn visible(&self) -> impl Iterator<Item = &CommitEntry> {
        self.visible
            .iter()
            .filter_map(|index| self.entries.get(*index))
    }

    /// The tree state at one visible commit index, by position in [`Build::visible`].
    pub(crate) fn tree_at(&self, position: usize) -> Option<&BTreeMap<String, File>> {
        self.maps.get(*self.visible.get(position)?)
    }
}

#[derive(Clone)]
/// A companion repository that was built.
pub(crate) struct CompanionBuild {
    /// The name the script used.
    pub name: String,
    /// Its commits.
    pub entries: Vec<CommitEntry>,
    /// Its tree states.
    pub(crate) maps: Vec<BTreeMap<String, File>>,
    /// Its recipes.
    pub(crate) recipes: Vec<Recipe>,
    /// Where its git dir ended up, relative to the fixture root (absolute for a sibling).
    pub git_dir: String,
    /// Where its work tree ended up.
    pub work_tree: String,
}

/// Materialise one scenario into `dest`, which is removed first if it exists.
///
/// # Errors
///
/// [`Error`] when the filesystem refuses, when a script asks for something impossible (a parent branch that
/// does not exist, deleting a file that is not there), or when a ground-truth file cannot be read.
pub fn generate(scenario: Scenario, dest: &Path) -> Result<Fixture> {
    let truth = scenario.ground_truth().map_err(|error| Error::Other {
        context: format!("reading the ground truth for `{}`", scenario.name()),
        detail: error.to_string(),
    })?;
    let script = scenario.script();
    generate_from(&script, scenario, dest, truth)
}

/// As [`generate`], with the script and ground truth supplied — the form `--bless` and the corpus walk use.
pub(crate) fn generate_from(
    script: &Script,
    scenario: Scenario,
    dest: &Path,
    truth: GroundTruth,
) -> Result<Fixture> {
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|source| git::Error::Io {
            context: dest.display().to_string(),
            source,
        })?;
    }
    std::fs::create_dir_all(dest).map_err(|source| git::Error::Io {
        context: dest.display().to_string(),
        source,
    })?;

    // Companions first: a gitlink names a commit that must exist before the commit naming it is written.
    let mut companions: BTreeMap<String, CompanionBuild> = BTreeMap::new();
    for companion in &script.companions {
        let built = build_companion(companion, dest)?;
        companions.insert(companion.name.clone(), built);
    }
    let git_dir = dest.join(".git");
    let mut build = build_script(
        script,
        dest,
        &git_dir,
        &format!("refs/heads/{}", script.default_branch),
        &[],
        &companions,
    )?;
    build.companions = companions;

    for extra in &script.extras {
        let Extra::ShallowClone {
            companion,
            depth,
            branch,
        } = extra;
        apply_shallow_clone(dest, &mut build, companion, *depth, branch)?;
    }

    // Check out the default branch, not whatever commit was created last: a scenario that ends on a feature
    // branch (`many_merges` does) would otherwise leave that branch's files in a work tree whose HEAD is
    // `main`, and `git status` would report a staged addition that nobody made.
    let head_ref = format!("refs/heads/{}", build.default_branch);
    let head_index = build
        .tip_indices
        .get(&head_ref)
        .copied()
        .ok_or_else(|| Error::Contract("the default branch has no tip".to_owned()))?;
    let worktree = build
        .maps
        .get(head_index)
        .cloned()
        .ok_or_else(|| Error::Contract("the fixture has no commits".to_owned()))?;
    let at = build
        .entries
        .get(head_index)
        .map(|entry| entry.at)
        .unwrap_or_default();
    let mut git = GitDir::open(dest, &git_dir)?;
    git.checkout(&worktree, policy::BASE_EPOCH + at)?;

    // The harness event log, from the commits the fixture shows.
    let events_summary = events::write(dest, &build, &script.events)?;
    // The sidecars are excluded, not tracked, so `git status` inside a fixture says nothing.
    git.append_exclude("/fixture.json\n")?;

    let objects = digest_objects(&git_dir)?;
    let worktree = digest_worktree(dest)?;
    let head = build.tips.get(&head_ref).cloned().unwrap_or_default();
    let digest = corpus_digest(
        script.scenario,
        build.visible.len(),
        &objects.digest,
        &build.tips,
        &worktree.digest,
        &events_summary.digest,
    );

    let failures = verify::verify(&build, &truth, &digest, events_summary.count);
    let refs_of = |prefix: &str| -> Vec<RefEntry> {
        build
            .tips
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, sha)| RefEntry {
                r#ref: name.clone(),
                commit: build.tip_indices.get(name).copied(),
                sha: sha.clone(),
            })
            .collect()
    };
    let manifest = Manifest {
        schema: crate::policy::FIXTURE_SCHEMA,
        generated_by: crate::policy::OWNED_BY,
        scenario: script.scenario,
        purpose: truth.purpose.clone(),
        consumers: truth.consumers.clone(),
        destination: dest.display().to_string(),
        base_epoch: policy::BASE_EPOCH,
        default_branch: build.default_branch.clone(),
        head,
        tips: build.tips.clone(),
        commit_count: build.visible.len(),
        commits: build.visible().cloned().collect(),
        branches: refs_of("refs/heads/"),
        tags: refs_of("refs/tags/"),
        companions: build
            .companions
            .values()
            .map(|companion| CompanionEntry {
                name: companion.name.clone(),
                git_dir: companion.git_dir.clone(),
                work_tree: companion.work_tree.clone(),
                head: companion
                    .entries
                    .last()
                    .map(|entry| entry.sha.clone())
                    .unwrap_or_default(),
                commits: companion
                    .entries
                    .iter()
                    .map(|entry| entry.sha.clone())
                    .collect(),
            })
            .collect(),
        events: events_summary,
        shallow: build.shallow.clone(),
        objects: DigestCount {
            count: objects.count,
            digest: objects.digest,
        },
        worktree: DigestCount {
            count: worktree.count,
            digest: worktree.digest,
        },
        digest,
        verification: Verification {
            ok: failures.is_empty(),
            failures: failures.clone(),
        },
        ground_truth: truth.clone(),
    };

    write_file(
        &dest.join("fixture.json"),
        format!("{}\n", pretty_json(&manifest)?).as_bytes(),
    )?;

    Ok(Fixture {
        scenario,
        path: dest.to_path_buf(),
        manifest,
        ground_truth: truth,
        failures,
    })
}

/// The blessed `determinism` block for a fixture — what `--bless` writes into the ground truth.
#[must_use]
pub fn determinism_of(fixture: &Fixture) -> crate::truth::Determinism {
    let manifest = &fixture.manifest;
    crate::truth::Determinism {
        note: Some(
            "written by `fathom-fixtures --bless`; everything else in this file is hand-authored and is \
             not touched by that command"
                .to_owned(),
        ),
        commit_count: Some(manifest.commit_count),
        head: Some(manifest.head.clone()),
        tips: manifest.tips.clone(),
        objects_digest: Some(manifest.objects.digest.clone()),
        worktree_digest: Some(manifest.worktree.digest.clone()),
        events_digest: Some(manifest.events.digest.clone()),
        digest: Some(manifest.digest.clone()),
    }
}

fn pretty_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|source| Error::Other {
        context: "serialising fixture.json".to_owned(),
        detail: source.to_string(),
    })
}

/// Build one script — a fixture, or a companion's history — into a directory.
fn build_script(
    script: &Script,
    work_root: &Path,
    git_dir: &Path,
    head_ref: &str,
    extra_config: &[String],
    companions: &BTreeMap<String, CompanionBuild>,
) -> Result<Build> {
    let mut git = GitDir::create(work_root, git_dir, head_ref, extra_config)?;
    let mut branches: BTreeMap<String, BranchState> = BTreeMap::new();
    let mut build = Build {
        entries: Vec::new(),
        maps: Vec::new(),
        recipes: Vec::new(),
        tips: BTreeMap::new(),
        tip_indices: BTreeMap::new(),
        companions: BTreeMap::new(),
        visible: Vec::new(),
        default_branch: script.default_branch.clone(),
        shallow: Vec::new(),
    };

    for step in &script.steps {
        let mut state = match branches.get(&step.branch) {
            Some(existing) => existing.clone(),
            None => {
                let base = match &step.fork_from {
                    Some(from) => branches
                        .get(from)
                        .ok_or_else(|| {
                            Error::Contract(format!(
                                "`{}`: `{}` forks from `{}`, which has no commits yet",
                                script.scenario, step.branch, from
                            ))
                        })?
                        .files
                        .clone(),
                    None => BTreeMap::new(),
                };
                BranchState {
                    head: None,
                    files: base.clone(),
                    fork: base,
                }
            }
        };
        let before = state.files.clone();

        // A step that names no parents is an ordinary commit on its branch, and its parent is wherever that
        // branch currently is. Without this every scenario would be a pile of root commits, which is the
        // single most important thing a history has: `git rev-list HEAD` over a fixture must walk the whole
        // fixture.
        let parents: Vec<String> = if step.parents.is_empty() {
            match branches
                .get(&step.branch)
                .and_then(|state| state.head.clone())
            {
                Some(head) => vec![head],
                None => Vec::new(),
            }
        } else {
            step.parents
                .iter()
                .map(|branch| {
                    branches
                        .get(branch)
                        .and_then(|state| state.head.clone())
                        .ok_or_else(|| {
                            Error::Contract(format!(
                                "`{}`: `{}` names parent `{branch}`, which has no tip yet",
                                script.scenario, step.subject
                            ))
                        })
                })
                .collect::<Result<Vec<_>>>()?
        };

        let mut gitlinks: Vec<GitlinkEntry> = Vec::new();
        for op in &step.ops {
            match op {
                Op::Write { path, blob } => {
                    state.files.insert(path.clone(), File::text(blob.clone()));
                }
                Op::Append { path, blob } => {
                    state
                        .files
                        .entry(path.clone())
                        .or_insert_with(|| File::text(String::new()))
                        .blob
                        .extend_from_slice(blob.as_bytes());
                }
                Op::Delete { path } => {
                    if state.files.remove(path).is_none() {
                        return Err(Error::Contract(format!(
                            "`{}`: `{}` deletes `{path}`, which is not in the tree",
                            script.scenario, step.subject
                        )));
                    }
                }
                Op::Rename { from, to } => {
                    let file = state.files.remove(from).ok_or_else(|| {
                        Error::Contract(format!(
                            "`{}`: `{}` renames `{from}`, which is not in the tree",
                            script.scenario, step.subject
                        ))
                    })?;
                    state.files.insert(to.clone(), file);
                }
                Op::Copy { from, to } => {
                    let file = state.files.get(from).cloned().ok_or_else(|| {
                        Error::Contract(format!(
                            "`{}`: `{}` copies `{from}`, which is not in the tree",
                            script.scenario, step.subject
                        ))
                    })?;
                    state.files.insert(to.clone(), file);
                }
                Op::Merge { branch } => {
                    let other = branches.get(branch).ok_or_else(|| {
                        Error::Contract(format!(
                            "`{}`: merging `{branch}` into `{}`, and `{branch}` has no commits",
                            script.scenario, step.branch
                        ))
                    })?;
                    merge_trees(&mut state.files, &other.files, &other.fork);
                }
                Op::Submodule {
                    path,
                    url,
                    companion,
                    at,
                } => {
                    let built = companions.get(companion).ok_or_else(|| {
                        Error::Contract(format!(
                            "`{}`: submodule `{path}` names companion `{companion}`, which this script \
                             does not build",
                            script.scenario
                        ))
                    })?;
                    let position = match at {
                        Gitlink::Tip => built.entries.len().saturating_sub(1),
                        Gitlink::Index(index) => *index,
                    };
                    let sha = built
                        .entries
                        .get(position)
                        .map(|entry| entry.sha.clone())
                        .ok_or_else(|| {
                            Error::Contract(format!(
                                "`{}`: submodule `{path}` names companion commit {position}, which does \
                                 not exist",
                                script.scenario
                            ))
                        })?;
                    let mut modules = state
                        .files
                        .get(".gitmodules")
                        .map(|file| String::from_utf8_lossy(&file.blob).into_owned())
                        .unwrap_or_default();
                    modules.push_str(&format!(
                        "[submodule \"{path}\"]\n\tpath = {path}\n\turl = {url}\n"
                    ));
                    state
                        .files
                        .insert(".gitmodules".to_owned(), File::text(modules));
                    state.files.insert(path.clone(), File::gitlink(sha.clone()));
                    gitlinks.push(GitlinkEntry {
                        path: path.clone(),
                        sha,
                        companion: companion.clone(),
                    });
                }
            }
        }

        let tree = git.tree(&state.files)?;
        let identity = policy::identity(step.identity).ok_or_else(|| {
            Error::Contract(format!(
                "`{}`: unknown committer `{}`",
                script.scenario, step.identity
            ))
        })?;
        let message = step.message();
        let at = policy::BASE_EPOCH + step.at;
        let sha = git.commit(&CommitSpec {
            tree: tree.clone(),
            parents: parents.clone(),
            identity,
            at,
            message: message.clone(),
        })?;
        let refname = format!("refs/heads/{}", step.branch);
        git.set_ref(&refname, &sha)?;
        state.head = Some(sha.clone());
        branches.insert(step.branch.clone(), state.clone());

        let index = build.entries.len();
        build.tips.insert(refname.clone(), sha.clone());
        build.tip_indices.insert(refname, index);
        let mut tags = Vec::new();
        if let Some(tag) = &step.tag {
            let tag_ref = format!("refs/tags/{tag}");
            git.set_ref(&tag_ref, &sha)?;
            build.tips.insert(tag_ref.clone(), sha.clone());
            build.tip_indices.insert(tag_ref, index);
            tags.push(format!("refs/tags/{tag}"));
        }

        let changes = diff(&before, &state.files, &step.ops);
        build.entries.push(CommitEntry {
            index,
            sha,
            branch: step.branch.clone(),
            parents: parents.clone(),
            author: identity.key,
            author_kind: match identity.kind {
                Kind::Agent => "agent",
                Kind::Human => "human",
                Kind::Bot => "bot",
            },
            at: step.at,
            timestamp: policy::iso_utc(at, 0),
            subject: step.subject.clone(),
            session: step.session.clone(),
            turn: step.turn.clone(),
            trailers: trailers_of(&message),
            merge: parents.len() > 1,
            n_files: changes.len(),
            n_add: changes.iter().map(|change| change.adds).sum(),
            n_del: changes.iter().map(|change| change.dels).sum(),
            n_binary: changes.iter().filter(|change| change.binary).count(),
            empty: before == state.files,
            changes,
            tags,
            gitlinks,
        });
        build.recipes.push(Recipe {
            identity: step.identity,
            at,
            message,
            parents,
        });
        build.maps.push(state.files.clone());
        build.visible.push(index);
    }

    git.set_head(&format!("refs/heads/{}", script.default_branch))?;
    Ok(build)
}

/// Three-way-ish merge: take the other side's change where our side did not also move it.
///
/// The simple rule is on purpose. A fixture's merge needs to be *explainable*, not to be
/// `git merge-recursive`; a conflict is expressed by the step that follows the [`Op::Merge`], which is what
/// a human leaves behind after resolving one.
fn merge_trees(
    ours: &mut BTreeMap<String, File>,
    theirs: &BTreeMap<String, File>,
    base: &BTreeMap<String, File>,
) {
    let paths: BTreeSet<&String> = theirs.keys().chain(base.keys()).collect();
    for path in paths {
        let was = base.get(path);
        let mine = ours.get(path);
        let their_file = theirs.get(path);
        let they_moved = their_file != was;
        let we_moved = mine != was;
        if !they_moved || we_moved {
            continue;
        }
        match their_file {
            Some(file) => {
                ours.insert(path.clone(), file.clone());
            }
            None => {
                ours.remove(path);
            }
        }
    }
}

/// Build a companion repository and describe where it went.
fn build_companion(companion: &Companion, dest: &Path) -> Result<CompanionBuild> {
    let (work, git_dir, extra_config) = match companion.location {
        Location::Sibling => {
            let root = sibling(dest).join(&companion.name);
            (root.clone(), root.join(".git"), vec![String::new()])
        }
        Location::ModulesDir => {
            let git_dir = dest.join(".git").join("modules").join(&companion.name);
            let work = dest.join(&companion.name);
            let back = relative(&git_dir, &work);
            (work, git_dir, vec![format!("\tworktree = {back}")])
        }
    };
    std::fs::create_dir_all(&work).map_err(|source| git::Error::Io {
        context: work.display().to_string(),
        source,
    })?;
    let build = build_script(
        &companion.script,
        &work,
        &git_dir,
        &format!("refs/heads/{}", companion.script.default_branch),
        &extra_config,
        &BTreeMap::new(),
    )?;
    if let Some(tip) = build.entries.last() {
        let mut git = GitDir::open(&work, &git_dir)?;
        git.checkout(
            build.maps.last().ok_or_else(|| {
                Error::Contract(format!("companion `{}` has no tree", companion.name))
            })?,
            policy::BASE_EPOCH + tip.at,
        )?;
    }
    if companion.location == Location::ModulesDir {
        // The gitfile that makes the mount point a submodule checkout.
        let back = relative(&dest.join(&companion.name), &git_dir);
        write_file(
            &dest.join(&companion.name).join(".git"),
            format!("gitdir: {back}\n").as_bytes(),
        )?;
    }
    Ok(CompanionBuild {
        name: companion.name.clone(),
        entries: build.entries.clone(),
        maps: build.maps.clone(),
        recipes: build.recipes.clone(),
        git_dir: display_relative(dest, &git_dir),
        work_tree: display_relative(dest, &work),
    })
}

/// Make `dest` a shallow clone of one of its companions: the same commit ids, `depth` of them, a
/// `.git/shallow`, an `origin`, and the history below the cut genuinely absent.
fn apply_shallow_clone(
    dest: &Path,
    build: &mut Build,
    companion: &str,
    depth: usize,
    branch: &str,
) -> Result<()> {
    let source = build.companions.get(companion).cloned().ok_or_else(|| {
        Error::Contract(format!(
            "shallow clone of `{companion}`, which was not built"
        ))
    })?;
    if source.entries.is_empty() {
        return Err(Error::Contract(format!(
            "shallow clone of `{companion}`, which has no commits"
        )));
    }
    let depth = depth.max(1).min(source.entries.len());
    let window = source.entries.len() - depth;

    let git_dir = dest.join(".git");
    let mut git = GitDir::open(dest, &git_dir)?;
    let mut clone = Build {
        entries: Vec::new(),
        maps: Vec::new(),
        recipes: Vec::new(),
        tips: BTreeMap::new(),
        tip_indices: BTreeMap::new(),
        companions: std::mem::take(&mut build.companions),
        visible: Vec::new(),
        default_branch: branch.to_owned(),
        shallow: Vec::new(),
    };
    for (offset, position) in (window..source.entries.len()).enumerate() {
        let recipe = source.recipes[position].clone();
        let state = source
            .maps
            .get(position)
            .cloned()
            .ok_or_else(|| Error::Contract("companion tree missing".to_owned()))?;
        let tree = git.tree(&state)?;
        let identity = policy::identity(recipe.identity)
            .ok_or_else(|| Error::Contract(format!("unknown committer `{}`", recipe.identity)))?;
        // The parents are kept exactly as they were, including the boundary commit's parent, which names an
        // object this repository does not have. That is not sloppiness on git's part and will not be fixed
        // here: `git clone --depth` leaves a parent line pointing at nothing, `.git/shallow` names the
        // commit it stops at, and a history walker that only copes with reachable parents fails on the only
        // case it was supposed to test.
        let parents = recipe.parents.clone();
        let sha = git.commit(&CommitSpec {
            tree: tree.clone(),
            parents: parents.clone(),
            identity,
            at: recipe.at,
            message: recipe.message.clone(),
        })?;
        let mut entry = source
            .entries
            .get(position)
            .cloned()
            .ok_or_else(|| Error::Contract("companion commit missing".to_owned()))?;
        entry.index = offset;
        entry.sha = sha.clone();
        entry.parents = parents.clone();
        entry.branch = branch.to_owned();
        let refname = format!("refs/heads/{branch}");
        git.set_ref(&refname, &sha)?;
        let remote = format!("refs/remotes/origin/{branch}");
        git.set_ref(&remote, &sha)?;
        clone.tips.insert(refname.clone(), sha.clone());
        clone.tips.insert(remote, sha);
        clone.tip_indices.insert(refname, offset);
        clone.entries.push(entry);
        clone.maps.push(state);
        clone.recipes.push(Recipe {
            identity: recipe.identity,
            at: recipe.at,
            message: recipe.message.clone(),
            parents: parents.clone(),
        });
        clone.visible.push(offset);
    }
    // The boundary: the shallowest commit that came over, and the one git records as its stopping place.
    let tip = clone
        .entries
        .first()
        .map(|entry| entry.sha.clone())
        .ok_or_else(|| Error::Contract("empty shallow clone".to_owned()))?;
    git.set_shallow(std::slice::from_ref(&tip))?;
    git.set_head(&format!("refs/heads/{branch}"))?;
    let origin = sibling(dest).join(companion).display().to_string();
    git.append_config(&format!(
        "[remote \"origin\"]\n\turl = {origin}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
         [branch \"{branch}\"]\n\tremote = origin\n\tmerge = refs/heads/{branch}\n"
    ))?;
    clone.shallow.push(tip);
    *build = clone;
    Ok(())
}

/// `<dest>.companion`, where the sibling repositories go.
#[must_use]
pub fn sibling(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fixture".to_owned());
    dest.with_file_name(format!("{name}.companion"))
}

fn display_relative(dest: &Path, path: &Path) -> String {
    path.strip_prefix(dest)
        .map(|rest| rest.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string().replace('\\', "/"))
}

/// A relative path from `from` to `to`, `/` separated as every git file insists on.
fn relative(from: &Path, to: &Path) -> String {
    let from_parts: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let mut common = 0;
    while common < from_parts.len()
        && common < to_parts.len()
        && from_parts[common] == to_parts[common]
    {
        common += 1;
    }
    let mut out = String::new();
    for _ in common..from_parts.len() {
        out.push_str("../");
    }
    let rest: Vec<String> = to_parts[common..]
        .iter()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    out.push_str(&rest.join("/"));
    out.trim_end_matches('/').to_owned()
}

/// The digest "byte-identical on all three OSes" is checked against.
#[must_use]
pub fn corpus_digest(
    scenario: &str,
    commits: usize,
    objects: &str,
    tips: &BTreeMap<String, String>,
    worktree: &str,
    events: &str,
) -> String {
    let mut text = format!("scenario\0{scenario}\x01commits\0{commits}\x01objects\0{objects}\x01",);
    for (name, sha) in tips {
        text.push_str(name);
        text.push(' ');
        text.push_str(sha);
        text.push('\x01');
    }
    text.push_str(&format!("worktree\0{worktree}\x01events\0{events}\x01"));
    crate::digest::sha256_hex(text.as_bytes())
}

/// Hash every loose object in a git dir: every object the fixture wrote, and nothing else.
pub(crate) fn digest_objects(git_dir: &Path) -> Result<DigestCount> {
    let root = git_dir.join("objects");
    let mut lines: Vec<String> = Vec::new();
    walk(&root, &mut |path, is_dir| {
        if is_dir {
            return;
        }
        let Some(rel) = path.strip_prefix(&root).ok() else {
            return;
        };
        let flat: String = rel
            .display()
            .to_string()
            .replace('\\', "/")
            .chars()
            .filter(|c| *c != '/')
            .collect();
        if flat.len() == 40 && flat.chars().all(|c| c.is_ascii_hexdigit()) {
            lines.push(flat.to_ascii_lowercase());
        }
    })?;
    lines.sort();
    let count = lines.len();
    Ok(DigestCount {
        count,
        digest: crate::digest::sha256_hex(lines.join("\n").as_bytes()),
    })
}

/// Hash the checked-out work tree, excluding the git dirs and this crate's own sidecars.
pub(crate) fn digest_worktree(dest: &Path) -> Result<DigestCount> {
    let mut lines: Vec<String> = Vec::new();
    walk(dest, &mut |path, is_dir| {
        if is_dir {
            return;
        }
        let rel = path
            .strip_prefix(dest)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace('\\', "/");
        let first = rel.split('/').next().unwrap_or_default();
        if first == ".git"
            || first == ".fathom"
            || rel.ends_with("fixture.json")
            || rel.ends_with("/.git")
        {
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        lines.push(format!("{rel}\0{}", crate::digest::sha256_hex(&bytes)));
    })?;
    lines.sort();
    let count = lines.len();
    Ok(DigestCount {
        count,
        digest: crate::digest::sha256_hex(lines.join("\n").as_bytes()),
    })
}

/// Walk a directory tree in a fixed order (sorted by name), calling `visit` for every entry.
pub(crate) fn walk(dir: &Path, visit: &mut dyn FnMut(&Path, bool)) -> Result<()> {
    let Ok(read) = std::fs::read_dir(dir) else {
        // A missing directory is an empty one: a fixture with no work tree yet is not an error here.
        return Ok(());
    };
    let mut entries: Vec<PathBuf> = read
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for entry in entries {
        let is_dir = entry.is_dir();
        visit(&entry, is_dir);
        if is_dir {
            walk(&entry, visit)?;
        }
    }
    Ok(())
}

/// The diff between two tree states, with renames taken from the step that made them.
fn diff(
    before: &BTreeMap<String, File>,
    after: &BTreeMap<String, File>,
    ops: &[Op],
) -> Vec<Change> {
    let renames: Vec<(String, String)> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Rename { from, to } => Some((from.clone(), to.clone())),
            _ => None,
        })
        .collect();
    let mut changes: Vec<Change> = Vec::new();
    let mut removed: BTreeSet<&String> = BTreeSet::new();
    for (path, file) in before {
        match after.get(path) {
            Some(now) if now.blob == file.blob && now.mode == file.mode && now.oid == file.oid => {}
            Some(now) => {
                let (adds, dels) = line_delta(&file.blob, &now.blob);
                changes.push(Change {
                    path: path.clone(),
                    change: "modified",
                    from: None,
                    adds,
                    dels,
                    bytes: now.blob.len(),
                    binary: now.looks_binary() || file.looks_binary(),
                });
            }
            None => {
                removed.insert(path);
            }
        }
    }
    let added: BTreeSet<&String> = after
        .keys()
        .filter(|path| !before.contains_key(*path))
        .collect();
    for (from, to) in &renames {
        if removed.contains(from) && added.contains(to) {
            let (Some(before_file), Some(after_file)) = (before.get(from), after.get(to)) else {
                continue;
            };
            // A rename that also rewrote the file reports its line delta, because "moved here" and "moved
            // here and changed" are two different findings and the ground truth has to be able to say which.
            let (adds, dels) = line_delta(&before_file.blob, &after_file.blob);
            changes.push(Change {
                path: to.clone(),
                change: "renamed",
                from: Some(from.clone()),
                adds,
                dels,
                bytes: after_file.blob.len(),
                binary: after_file.looks_binary() || before_file.looks_binary(),
            });
        }
    }
    for path in &removed {
        if renames
            .iter()
            .any(|(from, _)| from.as_str() == path.as_str())
        {
            continue;
        }
        let file = &before[*path];
        changes.push(Change {
            path: (*path).clone(),
            change: "deleted",
            from: None,
            adds: 0,
            dels: if file.looks_binary() {
                0
            } else {
                count_lines(&file.blob)
            },
            bytes: 0,
            binary: file.looks_binary(),
        });
    }
    for path in &added {
        if renames.iter().any(|(_, to)| to.as_str() == path.as_str()) {
            continue;
        }
        let file = &after[*path];
        changes.push(Change {
            path: (*path).clone(),
            change: "added",
            from: None,
            adds: if file.looks_binary() {
                0
            } else {
                count_lines(&file.blob)
            },
            dels: 0,
            bytes: file.blob.len(),
            binary: file.looks_binary(),
        });
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    changes
}

/// Lines added and removed between two contents, as a multiset difference over lines.
///
/// Not a diff algorithm, and not presented as one: see the module docs.
fn line_delta(before: &[u8], after: &[u8]) -> (usize, usize) {
    let counted = |bytes: &[u8]| -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for line in String::from_utf8_lossy(bytes).lines() {
            *out.entry(line.to_owned()).or_insert(0) += 1;
        }
        out
    };
    let (was, now) = (counted(before), counted(after));
    let mut adds = 0;
    let mut dels = 0;
    for (line, seen) in &now {
        adds += seen.saturating_sub(was.get(line).copied().unwrap_or_default());
    }
    for (line, seen) in &was {
        dels += seen.saturating_sub(now.get(line).copied().unwrap_or_default());
    }
    (adds, dels)
}

fn count_lines(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    String::from_utf8_lossy(bytes).lines().count()
}

/// The trailer block of a message: lower-cased keys, last value wins.
///
/// The same rule `fathom-ingest::record::Trailers` implements — the last paragraph, every line of it
/// `Key: value`, a key of alphanumerics and hyphens — which is what makes a fixture's claim about its own
/// trailers and the ingest's reading of them two names for one fact rather than two chances to disagree.
pub(crate) fn trailers_of(message: &str) -> BTreeMap<String, String> {
    let body = message.trim_end_matches(['\n', '\r']);
    let paragraph = match body.rsplit_once("\n\n") {
        Some((_, tail)) => tail,
        None => body,
    };
    let mut out = BTreeMap::new();
    let mut seen = 0;
    for line in paragraph.lines() {
        let Some((key, value)) = line.split_once(':') else {
            return BTreeMap::new();
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return BTreeMap::new();
        }
        out.insert(key.to_lowercase(), value.trim().to_owned());
        seen += 1;
    }
    if seen == 0 {
        return BTreeMap::new();
    }
    out
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|source| {
        Error::Git(git::Error::Io {
            context: path.display().to_string(),
            source,
        })
    })
}

// The script's event policy is re-exported through `events`, which is where it is interpreted; this keeps
// the two names from drifting apart.
const _: fn(sc::Events) -> sc::Events = |events| events;
