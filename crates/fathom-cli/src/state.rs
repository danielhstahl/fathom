//! The state the headless verbs report: the repository, the index, and what is wrong with either.
//!
//! `fathom stats`, `fathom show` and `fathom doctor` are three renderings of one read. That is the whole
//! reason this module exists: `fathom stats` is specified as "repo scale, index state, cache hit rate" and
//! `fathom doctor` as "diagnostics with remedies" — two verbs a bug report puts side by side, so they must
//! not be two implementations of "is the index OK?". ADR-004 §11 makes the same argument about the numbers
//! themselves, and ADR-006 §10.3 fixes the wording of the empty state for the same reason.
//!
//! # What "degraded" means, and what it costs
//!
//! Six states, in the order checked, each with the published exit code it maps to and the remedy the verb
//! prints. `docs/concepts/cli.md` reproduces this table; a test (`tests/test_cli_contract.py`) checks the
//! codes named here exist in `fathom_core::exit`:
//!
//! | State | Why it is a degradation | Code | Remedy |
//! |---|---|---|---|
//! | no store at all | every answer would be `unknown` | 12 | `fathom index` |
//! | store not at HEAD | the newest commit has no analysis, so the queue is yesterday's | 12 | `fathom index` |
//! | commits owed an analysis | a run was interrupted after it wrote commits | 12 | `fathom index` |
//! | a run never closed | the writer died holding the store | 12 | `fathom index` |
//! | epoch still `building` | a rebuild was started and not finished | 12 | `fathom index --rebuild` |
//! | store fails its checks | the file is not trustworthy | 4 | `fathom index --rebuild` |
//!
//! # The seventh state, which is not a degradation
//!
//! | State | Why it is not one | Code | Remedy |
//! |---|---|---|---|
//! | rows the history left behind | a rebase or force-push left commit rows for commits the object database has forgotten; the scale counted them and nothing said so | 12, under `--strict` only | `fathom index --prune` |
//!
//! [`Severity`] is the switch. The dead-row set is *untidy*: every answer the index gives is still the
//! answer — the queue is segmented over the reachable history, HEAD is indexed, the analysis is current —
//! so `fathom stats` prints the count, prints the verb that reclaims the rows, and leaves with 0 like the
//! healthy index it is. `fathom doctor --strict`, the pre-flight posture, is the one that says "fix this"
//! and exits 12. A rewrite that leaves rows behind is a normal thing a repository does; a CI job that has
//! decided its stores stay clean is entitled to say so.
//!
//! Note what is *not* here: an index with nothing in it because the repository has no commits is not
//! degraded, it is an empty repository, and the copy ADR-006 §10.3 fixes is the one printed.

use fathom_core::exit::ExitCode;
use fathom_ingest::repo::{Head, HealthNotice, ObjectHash, RefFilter, Repository};
use serde_json::{Value, json};
use std::path::Path;

use crate::sanitize::Redactor;

/// The remedy sentence, shared so every verb prints the same words (ADR-006 §10.3: a shrug is not a
/// remedy — and neither are three near-identical sentences that drift).
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
pub const REMEDY_INDEX: &str =
    "run `fathom index` in this repository, or `fathom index --rebuild` to throw the cache away";

/// The remedy for a store that cannot be trusted, which is the same sentence in both verbs that say it.
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
pub const REMEDY_REBUILD: &str = "the store is disposable (ADR-004 §12): `fathom index --rebuild`";

/// The remedy for a repository whose object database would not serve a commit.
///
/// It is not `fathom index`, and the difference is the whole finding: re-indexing a repository that is
/// missing an object produces the same missing object, which is exactly the loop a user of a damaged clone
/// was previously left in. Fathom reads repositories and never repairs or fetches them (ADR-001 §8.2), so
/// the thing to type is git's own.
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
pub const REMEDY_UNREADABLE: &str = "the object database is missing something fathom needs, and fathom neither fetches nor repairs: \
     `git fsck --full` in the repository (then `git fetch`, or re-clone, if it names a missing object), \
     then `fathom index`";

/// The remedy for rows the history has left behind.
///
/// It is the one verb that deletes (ADR-004 §8.1), and it is worth saying which: `fathom index` walks the
/// history and never removes anything, so a user who typed it — the obvious thing to type — was told nothing
/// happened, which is `reco-218`'s complaint. `--rebuild` is not the remedy either: an epoch bump makes
/// derived rows unreachable and leaves the spine's rows exactly where they were.
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
pub const REMEDY_PRUNE: &str = "`fathom index --prune` removes them (`--prune --dry-run` prints the list first); \
     a rewrite leaves rows behind by design and nothing else deletes them";

/// How much a finding costs the verb that printed it.
///
/// The distinction is not decoration: `fathom stats` and `fathom doctor` read the same snapshot and have
/// different jobs. One finding is a **degradation** — an answer is missing or wrong, so every verb that
/// reports the state of the index says so with its code. The other kind is **untidy**: every answer is
/// still the answer, and what the finding is asking for is housekeeping. `fathom doctor` prints both with
/// their remedies; only `--strict`, the pre-flight posture a CI job uses, exits for the second kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The index is missing, incomplete, stale or untrustworthy: the verb leaves with this code.
    Degraded,
    /// The index is whole and merely holding what the history no longer has: printed with its remedy, and
    /// it moves the exit code only under `fathom doctor --strict`.
    Untidy,
}

impl Severity {
    /// The machine name, as it appears in a finding's JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Degraded => "degraded",
            Severity::Untidy => "untidy",
        }
    }
}

/// One line of "here is what is wrong, and here is what to do about it".
///
/// A finding always carries its remedy, which is the difference between a diagnostic and a shrug. `exit` is
/// the *published* code the verb leaves with if it decides this finding is worth exiting for — that decision
/// belongs to the verb (`doctor` reports and exits 0 unless `--strict`), never to this struct.
#[derive(Debug, Clone)]
pub struct Finding {
    /// The stable machine name, e.g. `index_missing`.
    pub code: &'static str,
    /// Which section found it.
    pub section: &'static str,
    /// The human line.
    pub message: String,
    /// What to type.
    pub remedy: &'static str,
    /// The published code this finding maps to.
    pub exit: ExitCode,
    /// Whether it degrades an answer or only untidies the store — see [`Severity`].
    pub severity: Severity,
}

impl Finding {
    #[must_use]
    pub fn new(
        code: &'static str,
        section: &'static str,
        message: impl Into<String>,
        remedy: &'static str,
        exit: ExitCode,
    ) -> Self {
        Finding {
            code,
            section,
            message: message.into(),
            remedy,
            exit,
            severity: Severity::Degraded,
        }
    }

    /// A finding worth fixing that is not a broken answer — see [`Severity::Untidy`].
    #[must_use]
    pub fn untidy(
        code: &'static str,
        section: &'static str,
        message: impl Into<String>,
        remedy: &'static str,
        exit: ExitCode,
    ) -> Self {
        Finding {
            severity: Severity::Untidy,
            ..Self::new(code, section, message, remedy, exit)
        }
    }

    /// The finding as data — the shape `schemas/common.schema.json#/$defs/finding` pins.
    #[must_use]
    pub fn json(&self) -> Value {
        json!({
            "code": self.code,
            "section": self.section,
            "message": self.message,
            "remedy": self.remedy,
            "exit_code": self.exit.as_i32(),
            "exit_name": self.exit.name(),
            "severity": self.severity.as_str(),
        })
    }

    /// The two-line form the text renderers print: the problem, then the remedy.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("{}: {}", self.section, self.message),
            format!("  remedy: {}", self.remedy),
        ]
    }
}

/// Everything one read of one repository produced.
pub struct Snapshot {
    /// The directory the verb was pointed at, redacted for printing.
    pub root_display: String,
    /// The repository section, or the reason there is none.
    pub repository: Value,
    /// The index section, or the reason there is none.
    pub index: Value,
    /// The cache section: `null` when there is no store to count.
    pub cache: Value,
    /// The memory section: the ceilings in force for this process and what they cost (`reco-93w`).
    /// Never `null` — a run always has ceilings, even a run with no store to measure against them.
    pub memory: Value,
    /// What is wrong, in the order checked.
    pub findings: Vec<Finding>,
}

impl Snapshot {
    /// The highest-numbered published code among the findings, or `OK`.
    ///
    /// "Worst" is by number rather than by an invented severity scale: ADR-002 §7.3's table is already
    /// ordered roughly by how much attention a code wants, and a second ordering would be a second contract
    /// to keep in sync.
    ///
    /// Only [`Severity::Degraded`] findings count. This is the code a verb that *reports the state of the
    /// index* leaves with — `fathom stats`, `fathom show`, `fathom analyze` — and an untidy store whose every
    /// answer is still the answer is not a store whose report should read as a failure.
    #[must_use]
    pub fn worst_code(&self) -> ExitCode {
        self.worst_code_with(Severity::Degraded)
    }

    /// The same, counting the [`Severity::Untidy`] findings too: what `fathom doctor --strict` leaves with.
    ///
    /// The pre-flight question is not "is the index answering correctly" but "is there anything here a
    /// someone should fix", which is exactly the question `reco-218`'s dead-row set asks.
    #[must_use]
    pub fn worst_code_strict(&self) -> ExitCode {
        self.worst_code_with(Severity::Untidy)
    }

    fn worst_code_with(&self, at_least: Severity) -> ExitCode {
        self.findings
            .iter()
            .filter(|finding| match at_least {
                Severity::Degraded => finding.severity == Severity::Degraded,
                Severity::Untidy => true,
            })
            .map(|finding| finding.exit.as_i32())
            .filter(|code| *code != 0)
            .max()
            .and_then(|code| ExitCode::try_from(code).ok())
            .unwrap_or(ExitCode::Ok)
    }

    /// The findings as data.
    #[must_use]
    pub fn findings_json(&self) -> Vec<Value> {
        self.findings.iter().map(Finding::json).collect()
    }

    /// The findings as printed lines.
    #[must_use]
    pub fn finding_lines(&self) -> Vec<String> {
        self.findings.iter().flat_map(Finding::lines).collect()
    }
}

/// How hard a verb looks for a damaged store (ADR-004 §9.6).
///
/// Three levels because there are three verbs that ask three different questions, and the distinction is
/// a cost, not a preference: `PRAGMA quick_check` walks every b-tree page in the store, and
/// `pragma_foreign_key_check` walks every foreign-keyed row, so a verb that runs them pays for the size
/// of the repository on every invocation. Measured on a 190 MiB store built from `aws/aws-sdk-go` that
/// was ~0.5 s and ~0.35 s, spent before the verb did anything it was asked to do — `reco-66q`, in the
/// course of measuring `query.card.ms`, which is budgeted at 50 ms for the whole read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// The checks whose cost does not depend on how big the store is: page geometry, the schema this
    /// build can read, and the version stamps. Every read verb (`stats`, `show`, `analyze`, `export`,
    /// `compare`, `why`, the daemon's snapshot) runs these and only these.
    Structural,
    /// `fathom doctor`: the structural checks, `PRAGMA quick_check` and the foreign-key scan.
    Quick,
    /// `fathom doctor --deep`: `PRAGMA integrity_check` as well.
    Full,
}

impl Check {
    /// The name the report prints, so that "no problems" always says which problems were looked for.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Check::Structural => "structural",
            Check::Quick => "structural+quick_check+foreign_keys",
            Check::Full => "structural+quick_check+foreign_keys+integrity_check",
        }
    }
}

/// Read the repository at `root` and the index beside it, with the checks a read verb runs.
///
/// `deep` is `fathom doctor --deep`'s full `integrity_check`; the other verbs pass `false`, because a
/// whole-database check on every `fathom stats` is a check nobody will run twice — which is a sentence
/// this function only started telling the truth about in `reco-66q`, where `deep == false` meant
/// "run `quick_check` anyway". `fathom doctor` asks for more through [`gather_with`].
#[must_use]
pub fn gather(root: &Path, deep: bool) -> Snapshot {
    gather_with(root, if deep { Check::Full } else { Check::Structural })
}

/// The same, with the check level a verb means: this is `fathom doctor`'s entry point.
#[must_use]
pub fn gather_with(root: &Path, check: Check) -> Snapshot {
    let redactor = Redactor::for_root(root);
    let mut findings = Vec::new();
    let repository = describe_repository(root, &redactor, &mut findings);
    #[allow(unused_mut)]
    let mut head_oid = repository
        .get("head")
        .and_then(|head| head.get("oid"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    #[cfg(feature = "sqlite-bundled")]
    let (index, cache, index_findings) =
        describe_index(root, &redactor, head_oid.as_deref(), check);
    #[cfg(not(feature = "sqlite-bundled"))]
    let (index, cache, index_findings) = {
        let _ = (&check, &mut head_oid);
        (
            json!({ "present": false, "reason": "no SQLite backend in this build" }),
            Value::Null,
            vec![Finding::new(
                "feature_absent",
                "index",
                "this binary was built without a SQLite backend, so it cannot read an index",
                "rebuild with `--features sqlite-bundled`, or use the shipped default binary",
                ExitCode::FeatureAbsent,
            )],
        )
    };

    findings.extend(index_findings);
    // The memory section is the second half of the claim the ticket makes: the ceilings are not only
    // enforced, they are *printable*, with the key that set each one. Read here rather than passed in
    // because `fathom stats` is a CLI verb and the CLI is the crate that reads configuration.
    let ceilings = crate::memory::resolve(root);
    let memory = json!({
        "ceilings": ceilings.ceilings.observe(),
        "lines": crate::memory::lines(&ceilings.ceilings),
        "notices": ceilings.notices,
    });
    Snapshot {
        root_display: redactor.path(root),
        repository,
        index,
        cache,
        memory,
        findings,
    }
}

/// The remedy for a repository that is whole but truncated from the origin's point of view.
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
pub const REMEDY_SHALLOW: &str =
    "the clone was made with `--depth`: `git fetch --unshallow` (fathom never fetches for you)";

/// A commit id the way a terminal prints it: enough hex to be unique in a screenful, whatever the object
/// hash the repository was created with (sha1 gives 40, sha256 64).
fn short_oid(oid: &str) -> &str {
    &oid[..oid.len().min(12)]
}

/// What a degraded repository costs: `INDEX_DEGRADED`, because the published table describes 12 as "the
/// index is missing, incomplete or stale" and a history the clone never fetched produces exactly an
/// incomplete index. Adding a row to ADR-002 §7.3's table is a change of published contract a script
/// branches on; reading a shallow clone as an incomplete index is not.
const REPO_EXIT: ExitCode = ExitCode::IndexDegraded;

/// The finding a repository health notice earns, or `None` when the notice is a *state* rather than a
/// degradation.
///
/// A detached `HEAD`, an unborn branch, a bare repository and a missing index are ways for a repository to
/// be perfectly healthy and simply not be the shape `git status` assumes — ADR-006 §10.3 refuses to print a
/// warning for an empty repository, and the same rule covers these. What is left is degradation, and each
/// one carries the sentence to type, which is the difference between a diagnostic and a shrug.
fn notice_finding(notice: &HealthNotice) -> Option<Finding> {
    Some(match notice {
        HealthNotice::Shallow { boundary } => Finding::new(
            "history_shallow",
            "repository",
            format!(
                "history stops at {boundary} shallow graft(s): everything older than the graft is absent, \
                 not empty",
            ),
            REMEDY_SHALLOW,
            REPO_EXIT,
        ),
        HealthNotice::PartialClone { filter } => Finding::new(
            "partial_clone",
            "repository",
            format!(
                "the clone was filtered ({}), so the objects it never fetched cannot be read and the commits \
                 that need them cannot be indexed",
                filter.as_deref().unwrap_or("no filter recorded"),
            ),
            "re-clone without `--filter`, or fetch what is missing with git (fathom never fetches for you), \
             then `fathom index`",
            REPO_EXIT,
        ),
        HealthNotice::WorktreeMissing { expected } => Finding::new(
            "worktree_missing",
            "repository",
            format!(
                "the working tree this repository points at is not there: {}",
                expected.display()
            ),
            "restore the directory or drop the stale registration with `git worktree prune`; the commits are \
             still indexable from a bare or linked checkout",
            ExitCode::NotARepository,
        ),
        HealthNotice::BrokenSymbolicRef { name, to } => Finding::new(
            "broken_symref",
            "repository",
            format!("the ref {name} points at {to}, which does not exist"),
            "aim it at something real with `git symbolic-ref`, or remove it with `git update-ref -d`",
            ExitCode::NotARepository,
        ),
        HealthNotice::Untrusted => Finding::new(
            "repository_untrusted",
            "repository",
            "the repository belongs to another user, so it was read at arm's length",
            "if you trust it: `git config --global --add safe.directory <path>`",
            ExitCode::NotARepository,
        ),
        // States, not degradations: said in the repository section, never as a finding.
        HealthNotice::DetachedHead { .. }
        | HealthNotice::UnbornHead { .. }
        | HealthNotice::Bare
        | HealthNotice::NoIndex
        // Not degradations either: a branch pile-up and an object store on another filesystem are both
        // said, with their remedy, in the `notes` array.
        | HealthNotice::ManyBranches { .. }
        | HealthNotice::SeparateFilesystemStore { .. } => return None,
    })
}

/// The line a notice that is *not* a degradation earns: what it is, and what to do about it, in one
/// sentence a status line can carry.
///
/// `None` for the notices that are findings, because those print with their own remedy already and twice is
/// not more informative.
fn notice_note(notice: &HealthNotice) -> Option<String> {
    Some(match notice {
        HealthNotice::DetachedHead { .. }
        | HealthNotice::UnbornHead { .. }
        | HealthNotice::Bare
        | HealthNotice::NoIndex => return None,
        HealthNotice::ManyBranches { branches } => format!(
            "{branches} local branches is a list, not a state you hold in your head — `git branch \
             --merged <yours>` is how to find the ones to delete; fathom indexes what the refs reach, and \
             an old branch is not a wrong one"
        ),
        HealthNotice::SeparateFilesystemStore { .. } => format!(
            "{notice} — `fathom watch` cannot trust inotify across it, so run the watcher with \
             `--backend poll` (ADR-004 §7.8)"
        ),
        other => other.to_string(),
    })
}

/// Everything the repository says about itself that a reviewer should know before trusting an answer,
/// one line each, each with what to do about it.
///
/// This is what the TUI's notice list and any other caller that only has lines prints; `fathom doctor` and
/// `fathom stats` print the same facts as findings (with their remedies) and as notes, from the same
/// [`HealthNotice`] values, so the three renderings cannot drift.
#[must_use]
pub fn repository_notices(root: &Path) -> Vec<String> {
    let Ok(repository) = Repository::discover(root) else {
        return Vec::new();
    };
    let Ok(health) = repository.health() else {
        return Vec::new();
    };
    health
        .notices
        .iter()
        .filter_map(|notice| {
            notice_finding(notice).map_or_else(
                || notice_note(notice),
                |finding| Some(format!("{} — {}", finding.message, finding.remedy)),
            )
        })
        .collect()
}

/// The commits this store could not read, as notice lines, for a caller that has the repository but not the
/// store — which is the TUI's shape, and the reason this is a function rather than part of
/// [`repository_notices`]: the object database's failures are recorded in the store, so reading them means
/// opening it, and the shell already has it open.
///
/// Same words, same remedy as the `object_unreadable` finding, because a reviewer who meets the sentence in
/// the frame and again in `fathom doctor` must not have to work out whether they are the same problem.
#[must_use]
pub fn unreadable_notices(root: &Path) -> Vec<String> {
    use fathom_store::spine::Store;

    let path = fathom_analyze::index::default_store_path(root);
    if !path.is_file() {
        return Vec::new();
    }
    let Ok(store) = Store::open(&path) else {
        return Vec::new();
    };
    let Ok(holes) = store.unreadable_commits() else {
        return Vec::new();
    };
    if holes.is_empty() {
        return Vec::new();
    }
    let redactor = Redactor::for_root(root);
    let finding = object_unreadable_finding(&holes, &redactor);
    vec![format!("{} — {}", finding.message, finding.remedy)]
}

/// The finding for commits the object database would not serve.
///
/// One builder because the sentence is said in three places — `fathom stats`, `fathom doctor`, and the TUI's
/// notice list — and a reviewer who meets a shorter version of it in the frame would read the missing part
/// as "nothing else is wrong". Up to three are named with their reasons, because the reason is usually the
/// same object and the fourth copy of a path teaches nothing; the count is the whole store's.
pub fn object_unreadable_finding(holes: &[(String, String, i64)], redactor: &Redactor) -> Finding {
    let named = holes
        .iter()
        .take(3)
        .map(|(commit, reason, _)| format!("{} — {}", short_oid(commit), redactor.text(reason)))
        .collect::<Vec<_>>()
        .join(" · ");
    Finding::new(
        "object_unreadable",
        "index",
        format!(
            "{} commit(s) could not be read out of the object database and are missing from the index: \
             {named}",
            holes.len()
        ),
        REMEDY_UNREADABLE,
        ExitCode::IndexDegraded,
    )
}

/// The finding for commit rows the history has left behind (`reco-218`).
///
/// Untidy, not degraded, and the distinction is the finding's whole claim: the index describes HEAD, every
/// episode is segmented over the reachable history, and the review queue does not show the ghosts — so no
/// answer is wrong. What is wrong is that the store is counted as holding a repository that no longer
/// exists, which is what `fathom stats`' `scale` line said and what nothing used to say. The named oids are
/// the newest few: the commits a reviewer is likeliest to remember typing.
pub fn dead_rows_finding(dead: &fathom_store::inventory::DeadRows) -> Finding {
    let named = dead
        .sample
        .iter()
        .take(3)
        .map(String::as_str)
        .map(short_oid)
        .collect::<Vec<_>>()
        .join(" · ");
    Finding::untidy(
        "index_dead_rows",
        "index",
        format!(
            "{count} of the {retained} commit row(s) in the index are rows for commits no ref can reach: \
             the history was rewritten and the objects are gone — {named}",
            count = dead.dead,
            retained = dead.retained,
        ),
        REMEDY_PRUNE,
        ExitCode::IndexDegraded,
    )
}

/// The repository section, built from `fathom-ingest`'s own health report.
fn describe_repository(root: &Path, redactor: &Redactor, findings: &mut Vec<Finding>) -> Value {
    let repo = match Repository::discover(root) {
        Ok(repo) => repo,
        Err(err) => {
            findings.push(Finding::new(
                "not_a_repository",
                "repository",
                redactor.text(&format!(
                    "{} is not a git repository: {err}",
                    redactor.path(root)
                )),
                "run this inside a git repository, or point --root at one",
                ExitCode::NotARepository,
            ));
            return json!({ "present": false, "error": redactor.text(&err.to_string()) });
        }
    };
    let health = match repo.health() {
        Ok(health) => health,
        Err(err) => {
            findings.push(Finding::new(
                "repository_unreadable",
                "repository",
                redactor.text(&format!("the repository could not be read: {err}")),
                "check that `.git` is readable, or point --root at the work tree",
                ExitCode::NotARepository,
            ));
            return json!({ "present": false, "error": redactor.text(&err.to_string()) });
        }
    };
    let head = &health.head;
    let head_oid = head.oid().map(|oid| oid.hex());
    // The repository's own degradations, as findings. A shallow clone is not a broken repository and a
    // filter clone is not a corrupt one — each is a whole repository that is smaller than it looks — but
    // every answer fathom gives about the missing half is an absence, and ADR-001 §7.6's rule is that an
    // absence is said. This is where it gets said with a remedy, in the same list the index's problems are
    // in, so `fathom stats`, `fathom show` and `fathom doctor` cannot disagree about it.
    for notice in &health.notices {
        if let Some(finding) = notice_finding(notice) {
            findings.push(finding);
        }
    }
    let refs = repo.refs(RefFilter::All).unwrap_or_default();
    let mut remote_names: Vec<String> = refs
        .iter()
        .filter_map(|reference| reference.name.strip_prefix("refs/remotes/"))
        .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned())
        .collect();
    remote_names.sort();
    remote_names.dedup();
    let remotes: Vec<Value> = remote_names
        .into_iter()
        .map(|name| {
            // A remote URL is the one git config value worth printing, and the one most likely to carry a
            // credential, so it goes through the redactor rather than onto the page.
            let url = repo
                .config_string(&format!("remote.{name}.url"))
                .map(|url| redactor.secret(&url));
            json!({ "name": name, "url": url })
        })
        .collect();

    json!({
        "present": true,
        "root": redactor.path(root),
        "kind": repo.kind().as_str(),
        "bare": repo.is_bare(),
        "work_dir": repo.work_dir().map(|path| redactor.path(path)),
        "git_dir": redactor.path(repo.git_dir()),
        "object_hash": match repo.object_hash() {
            ObjectHash::Sha1 => "sha1",
            ObjectHash::Sha256 => "sha256",
            ObjectHash::Other(_) => "other",
        },
        "head": {
            "kind": match head {
                Head::Branch { .. } => "branch",
                Head::Unborn { .. } => "unborn",
                Head::Detached { .. } => "detached",
            },
            "branch": head.branch_name(),
            "oid": head_oid,
            "summary": redactor.text(&head.summary()),
        },
        "refs": {
            "total": refs.len(),
            "branches": health.refs.branches,
            "remote_branches": health.refs.remote_branches,
            "tags": health.refs.tags,
            "other": health.refs.other,
        },
        "remotes": remotes,
        "shallow": health.shallow.is_shallow,
        "partial_clone": health.partial.is_partial,
        "submodules": health.submodules,
        "notices": health
            .notices
            .iter()
            .map(|notice| redactor.text(&notice.to_string()))
            .collect::<Vec<_>>(),
        "notes": health
            .notices
            .iter()
            .filter_map(notice_note)
            .map(|note| redactor.text(&note))
            .collect::<Vec<_>>(),
    })
}

/// The index and cache sections, plus the findings they earned.
///
/// Opening the store is the only side effect, and the `exists()` check is why `fathom stats` on an
/// unindexed repository reports *that* instead of minting an empty store and reporting "indexed, 0 commits".
#[cfg(feature = "sqlite-bundled")]
#[allow(clippy::too_many_lines)]
fn describe_index(
    root: &Path,
    redactor: &Redactor,
    head_oid: Option<&str>,
    check: Check,
) -> (Value, Value, Vec<Finding>) {
    use fathom_store::spine::Store;

    let mut findings = Vec::new();
    let path = fathom_analyze::index::default_store_path(root);
    if !path.exists() {
        findings.push(Finding::new(
            "index_missing",
            "index",
            format!("there is no index at {} yet", redactor.path(&path)),
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
        return (
            json!({ "present": false, "path": redactor.path(&path) }),
            Value::Null,
            findings,
        );
    }

    let store = match Store::open(&path) {
        Ok(store) => store,
        Err(err) => {
            findings.push(Finding::new(
                "store_unusable",
                "store",
                redactor.text(&err.to_string()),
                REMEDY_REBUILD,
                ExitCode::StoreCorrupt,
            ));
            return (
                json!({
                    "present": true,
                    "path": redactor.path(&path),
                    "error": redactor.text(&err.to_string()),
                }),
                Value::Null,
                findings,
            );
        }
    };

    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let wal = std::fs::metadata(path.with_extension("sqlite3-wal"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    // One read, one walk: the scale the report prints and the dead set it must not silently exclude are two
    // halves of the same judgement, and `scale()` on its own would have to walk again to say which rows it
    // left out.
    let inventory = store.inventory().ok();
    let scale = inventory.as_ref().map(|inventory| inventory.scale.clone());
    let dead = inventory.as_ref().map(|inventory| inventory.dead.clone());
    let runs = store.run_summary().ok();
    let inventory = store.cache_inventory().ok();
    let cache_footprint = store.cache_footprint().ok();
    let counters = store.counters("cache.").unwrap_or_default();
    let pending = store.pending_derivation().unwrap_or_default();
    let integrity = match check {
        Check::Structural => store.structural_check().unwrap_or_default(),
        Check::Quick => store.integrity_check(false).unwrap_or_default(),
        Check::Full => store.integrity_check(true).unwrap_or_default(),
    };
    let tips = store.ref_tips().unwrap_or_default();
    let head_indexed = match head_oid {
        Some(oid) => store
            .indexed_oids(&[oid.to_owned()])
            .map(|found| !found.is_empty())
            .unwrap_or(true),
        None => true,
    };
    let epoch = store.epoch().unwrap_or(0);
    // The commits the object database would not serve, as the index last discovered them. Read before the
    // staleness findings, because it is usually their cause: a HEAD that cannot be read is not a HEAD that
    // has not been indexed yet, and telling somebody to run the command that just failed is how a diagnostic
    // becomes folklore.
    let unreadable = store.unreadable_commits().unwrap_or_default();
    let head_unreadable =
        head_oid.is_some_and(|oid| unreadable.iter().any(|(commit, _, _)| commit == oid));
    let epoch_status = store
        .meta("epoch_status")
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_owned());
    let commits = scale.as_ref().map_or(0, |scale| scale.commits);

    if !integrity.is_empty() {
        findings.push(Finding::new(
            "store_corrupt",
            "store",
            format!(
                "the store failed its integrity checks: {}",
                redactor.text(&integrity.join("; "))
            ),
            REMEDY_REBUILD,
            ExitCode::StoreCorrupt,
        ));
    }
    if epoch_status == "building" {
        findings.push(Finding::new(
            "index_building",
            "index",
            format!("epoch {epoch} is still marked `building`"),
            "finish the rebuild: `fathom index --rebuild`",
            ExitCode::IndexDegraded,
        ));
    }
    if !unreadable.is_empty() {
        findings.push(object_unreadable_finding(&unreadable, redactor));
    }
    // The ghosts of a rewrite, said as untidy rather than degraded: every answer is the answer, but the
    // store is holding rows for commits that do not exist and nothing else in the report would say so.
    if let Some(dead) = dead.as_ref().filter(|dead| dead.dead > 0) {
        findings.push(dead_rows_finding(dead));
    }
    if !head_indexed {
        findings.push(Finding::new(
            "index_stale",
            "index",
            format!(
                "HEAD ({}) has no commit row, so the newest work is in no answer{}",
                head_oid.unwrap_or("unknown"),
                if head_unreadable {
                    " — it is one of the commits the object database would not serve"
                } else {
                    ""
                }
            ),
            if head_unreadable {
                REMEDY_UNREADABLE
            } else {
                REMEDY_INDEX
            },
            ExitCode::IndexDegraded,
        ));
    }
    if !pending.is_empty() {
        findings.push(Finding::new(
            "index_incomplete",
            "index",
            format!("{} commit(s) still owe an analysis", pending.len()),
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
    }
    let unclean = runs
        .as_ref()
        .map_or(0, |runs| runs.open + runs.crashed + runs.aborted);
    if unclean > 0 {
        let counts = runs.as_ref();
        findings.push(Finding::new(
            "index_run_unclean",
            "index",
            format!(
                "{} index run(s) did not finish cleanly ({} still open, {} crashed, {} aborted)",
                unclean,
                counts.map_or(0, |runs| runs.open),
                counts.map_or(0, |runs| runs.crashed),
                counts.map_or(0, |runs| runs.aborted)
            ),
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
    }
    if commits == 0 && head_oid.is_some() {
        findings.push(Finding::new(
            "index_empty",
            "index",
            "the repository has commits and the index has none",
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
    }

    let index = json!({
        "present": true,
        "path": redactor.path(&path),
        "size_bytes": size,
        "wal_bytes": wal,
        "schema_version": store.meta("schema_version").ok().flatten(),
        "record_schema": store.meta("key_schema").ok().flatten(),
        "store_id": store.meta("store_id").ok().flatten(),
        "epoch": epoch,
        "epoch_status": epoch_status,
        "integrity": if integrity.is_empty() { "ok" } else { "failed" },
        // Which checks were run. Without this a shallow pass and a deep one print the same `"ok"`, and a
        // reader who is told a store is sound is entitled to know what was actually looked at.
        "integrity_checks": check.label(),
        "integrity_problems": integrity,
        "unreadable_commits": unreadable
            .iter()
            .map(|(commit, reason, seen_utc)| {
                json!({"commit": commit, "reason": redactor.text(reason), "seen_utc": seen_utc})
            })
            .collect::<Vec<_>>(),
        "commits": commits,
        "commits_retained": dead.as_ref().map_or(0, |dead| dead.retained),
        "dead_commits": dead.as_ref().map_or(0, |dead| dead.dead),
        "dead_files": dead.as_ref().map_or(0, |dead| dead.dead_files),
        "dead_unjudged": dead.as_ref().map_or(0, |dead| dead.unjudged),
        "dead_unjudged_reason": dead
            .as_ref()
            .and_then(|dead| dead.reason.clone())
            .map(|reason| redactor.text(&reason)),
        "dead_sample": dead
            .as_ref()
            .map(|dead| {
                dead.sample
                    .iter()
                    .map(|oid| json!({"commit": oid}))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        "head_indexed": head_indexed,
        "pending_derivation": pending.len(),
        "ref_tips": tips
            .iter()
            .map(|tip| json!({"name": tip.name, "oid": tip.oid, "kind": tip.kind}))
            .collect::<Vec<_>>(),
        "last_run": runs
            .as_ref()
            .and_then(|runs| runs.last.as_ref())
            .map(|last| json!({
                "id": last.id,
                "kind": last.kind,
                "trigger": last.trigger,
                "status": last.status,
                "started_utc": last.started_utc,
                "finished_utc": last.finished_utc,
                "commits_seen": last.commits_seen,
                "commits_added": last.commits_added,
                "rows_written": last.rows_written,
                "interrupted": last.interrupted,
                "error": last.error,
            })),
        "runs": runs.as_ref().map(|runs| json!({"total": runs.total, "unclean": unclean})),
        "scale": scale.map(|scale| json!({
            "commits": scale.commits,
            "files": scale.files,
            "authors": scale.authors,
            "lines_added": scale.lines_added,
            "lines_removed": scale.lines_removed,
            "binary_files": scale.binary_files,
            "oversized_commits": scale.oversized_commits,
            "first_commit_utc": scale.first_commit_utc,
            "last_commit_utc": scale.last_commit_utc,
            "noise": scale.noise,
            "kinds": scale.kinds,
            "top_authors": scale
                .top_authors
                .iter()
                .map(|(author, count)| json!({"author": author, "commits": count}))
                .collect::<Vec<_>>(),
        })),
    });

    (
        index,
        cache_json(&counters, inventory.as_ref(), cache_footprint),
        findings,
    )
}

/// The cache section: the counters the store keeps, in the shape `fathom index --stats` prints.
///
/// [`fathom_analyze::index::stats_of`] is the only reader of the counter names, which is what lets
/// `fathom index --stats`, `fathom stats` and `fathom doctor --json` quote one hit rate rather than three
/// approximations of it (ADR-004 §11's argument for a single set of numbers).
#[cfg(feature = "sqlite-bundled")]
#[must_use]
pub fn cache_json(
    counters: &std::collections::BTreeMap<String, i64>,
    inventory: Option<&fathom_store::inventory::CacheInventory>,
    footprint: Option<(i64, i64)>,
) -> Value {
    use fathom_analyze::index::{ANALYZED, stats_of};

    let rows: Vec<Value> = ANALYZED
        .iter()
        .map(|id| {
            let stats = stats_of(counters, id);
            json!({
                "id": id,
                "lookups": stats.lookups,
                "hits": stats.hits,
                "negative_hits": stats.negative_hits,
                "cold": stats.cold,
                "bumped": stats.bumped,
                "epoch_stale": stats.epoch_stale,
                "puts": stats.puts,
                "computes": (stats.lookups - stats.hits - stats.negative_hits).max(0),
                "bytes": stats.bytes,
                "cost_ms": stats.cost_ms,
                "hit_rate": stats.hit_rate(),
            })
        })
        .collect();
    let lookups: i64 = rows.iter().filter_map(|row| row["lookups"].as_i64()).sum();
    let hits: i64 = rows
        .iter()
        .filter_map(|row| {
            row["hits"]
                .as_i64()
                .map(|hit| hit + row["negative_hits"].as_i64().unwrap_or(0))
        })
        .sum();
    // `None` when nothing was asked, not `0%`: `AnalyzerStats::hit_rate` makes that distinction, and a
    // status command that flattened it would make a warm cache look broken.
    let hit_rate = if lookups > 0 {
        Some(
            f64::from(u32::try_from(hits).unwrap_or(u32::MAX)) * 100.0
                / f64::from(u32::try_from(lookups).unwrap_or(u32::MAX)),
        )
    } else {
        None
    };

    json!({
        "hit_rate": hit_rate,
        "lookups": lookups,
        "hits": hits,
        // `computed` is the misses: the work this repository made the analyzers do over its life, which is
        // the number a slow warm run actually explains. Summing `puts` would not be it — a `--refresh`
        // recompute writes a row after a hit, so `puts` counts writes and not misses (§5.6).
        "computed": lookups - hits,
        "analyzers": rows,
        "rows": inventory.map_or(0, |inventory| inventory.rows),
        "stale_rows": inventory.map_or(0, |inventory| inventory.stale_epoch_rows),
        "stored_failures": inventory.map_or(0, |inventory| inventory.stored_failures),
        "registered_analyzers": inventory.map_or(0, |inventory| inventory.analyzers),
        "unknown_analyzer_rows": inventory.map_or(0, |inventory| inventory.unknown_analyzers),
        "blobs": inventory.map_or(0, |inventory| inventory.blobs),
        "blob_bytes": inventory.map_or(0, |inventory| inventory.blob_bytes),
        "blobs_inline": inventory.map_or(0, |inventory| inventory.blobs_inline),
        "blobs_on_disk": inventory.map_or(0, |inventory| inventory.blobs_on_disk),
        "served_bytes": inventory.map_or(0, |inventory| inventory.served_bytes),
        // The §8.6 budget's own ledger (`reco-93w`): what the `cache` table holds now, and what the
        // ceiling has already thrown away to keep it there. Without these two lines a bounded cache and
        // a cache that is quietly full look identical, which is the whole thing an observable bound has
        // to avoid.
        "cache_rows": footprint.map_or(0, |(rows, _)| rows),
        "cache_payload_bytes": footprint.map_or(0, |(_, bytes)| bytes),
        "evicted_rows": counters.get("cache.evict").copied().unwrap_or(0),
        "evicted_bytes": counters.get("cache.evict.bytes").copied().unwrap_or(0),
        "evicted_by_analyzer": counters
            .iter()
            .filter(|(name, _)| {
                name.starts_with("cache.evict.")
                    && name.as_str() != "cache.evict.bytes"
                    && name.as_str() != "cache.evict.blobs_dangling"
            })
            .map(|(name, value)| {
                (
                    name.trim_start_matches("cache.evict.").to_owned(),
                    Value::from(*value),
                )
            })
            .collect::<serde_json::Map<String, Value>>(),
        "blobs_left_unreachable": counters
            .get("cache.evict.blobs_dangling")
            .copied()
            .unwrap_or(0),
        "payload_evictions": counters.get("cache.payload.evict").copied().unwrap_or(0),
        "payload_evicted_bytes": counters
            .get("cache.payload.evict.bytes")
            .copied()
            .unwrap_or(0),
        "payload_refusals": counters.get("cache.payload.refused").copied().unwrap_or(0),
    })
}
