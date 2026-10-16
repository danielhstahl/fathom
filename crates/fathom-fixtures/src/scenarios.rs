//! `scenarios.rs` — the corpus itself: one function per scenario, each one the whole story.
//!
//! Every function here returns a [`Script`]: data, not side effects. Nothing in this module writes a file or
//! runs a process — `crate::generate` does that, once, the same way for every scenario. That split is what
//! makes a scenario reviewable: the diff of a scenario change is a diff of a history, and the ground-truth
//! file next to it is the diff of what that history means.
//!
//! Reading order for a new scenario, which is also the order the functions are in: what happens, who did it,
//! and which turn claimed it. The `--scenario <name>` spelling, the ground-truth filename, and the entry in
//! [`crate::scenario::Scenario::ALL`] all come from the same name.
//!
//! Two conventions run through all of them. Every scenario starts with a scaffold commit that makes the
//! repository a project rather than a pile of paths — a detector that ignores non-code files should be able
//! to prove it ignores them, and that needs the non-code files to exist. And every scenario whose shape is
//! about *when* says so with an explicit [`crate::script::StepBuilder::at`]; everything else takes the
//! default quarter-hour spacing and does not pretend to be testing time.

use crate::content::{
    self, Dep, api_mod, baseline_deps, binary_blob, bundle_min, cargo_lock, cargo_toml,
    cargo_toml_plus, ci_yml, core_mod, debug_log_txt, doc_page, gitignore, hand_edited, main_rs,
    migration_md, module_with_hint, notebook_ipynb, notes_md, openapi_json, package_json,
    package_lock, parse_rs, readme, render_rs, report_pandas, report_py, requirements,
    rust_toolchain, scheduler_rs, scheduler_toml, scheduler_with_policy, scheduler_with_retry,
    scratch_rs, store_mod, tests_api, tests_core, tests_core_ignored, tests_for, unicode_names,
};
use crate::policy::{CODEX, DEPS_BOT, HUMAN_MAINTAINER, HUMAN_REVIEWER, Identity, PI};
use crate::script::{Companion, Events, Extra, Gitlink, Location, Script};

/// The opening commit of every scenario: the manifest, the ignore file, the pinned toolchain and the README.
///
/// It is a human commit on purpose. An agent session that begins mid-repository is the common case, and the
/// corpus should not have to test "what happens when the first commit in the history is an agent's" in every
/// scenario at once.
fn scaffold(script: &mut Script) {
    script
        .commit("Scaffold the lumen workspace")
        .by(&HUMAN_MAINTAINER)
        .write("Cargo.toml", cargo_toml(baseline_deps()))
        .write("Cargo.lock", cargo_lock(&[]))
        .write(".gitignore", gitignore())
        .write("rust-toolchain.toml", rust_toolchain())
        .write("README.md", readme("api::query"))
        .write(".github/workflows/ci.yml", ci_yml(false))
        .done();
}

/// The module tree most scenarios publish before they start misbehaving.
fn publish(script: &mut Script, round: u32) {
    script
        .commit("Publish the module tree")
        .by(&PI)
        .turn("publish", "1")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/main.rs", main_rs())
        .write("src/core/mod.rs", core_mod(false))
        .write("src/store/mod.rs", store_mod(false))
        .write("src/api/mod.rs", api_mod("routes"))
        .write("src/api/routes.rs", content::routes(round))
        .write("src/parse.rs", parse_rs(0))
        .write("src/render.rs", render_rs(0))
        .write("tests/api.rs", tests_api(true))
        .done();
}

/// A ten-thousand-fold bump of one dependency, for the scenario that needs the number rather than the story.
fn bump(name: &str, version: &str) -> String {
    let owned: Vec<(String, String, String)> = baseline_deps()
        .iter()
        .map(|(n, v, f)| ((*n).to_owned(), (*v).to_owned(), (*f).to_owned()))
        .map(|(n, v, f)| {
            if n == name {
                (n, version.to_owned(), f)
            } else {
                (n, v, f)
            }
        })
        .collect();
    let deps: Vec<Dep<'_>> = owned
        .iter()
        .map(|(n, v, f)| (n.as_str(), v.as_str(), f.as_str()))
        .collect();
    cargo_toml(&deps)
}

/// The same manifest with `name` gone, for the episode that removes a dependency.
fn without(name: &str) -> String {
    let owned: Vec<(String, String, String)> = baseline_deps()
        .iter()
        .filter(|(n, _, _)| *n != name)
        .map(|(n, v, f)| ((*n).to_owned(), (*v).to_owned(), (*f).to_owned()))
        .collect();
    let deps: Vec<Dep<'_>> = owned
        .iter()
        .map(|(n, v, f)| (n.as_str(), v.as_str(), f.as_str()))
        .collect();
    cargo_toml(&deps)
}

/// Rename a whole module tree in one commit, and a third rename that also changed the file.
///
/// What the scenario is testing is the *difference between the two*: four paths moved with identical bytes
/// (a rename a rename-detector must report, and a deletion detector must not) and one path moved with its
/// content rewritten (a rename the rename-detector still has to report, with the content change kept). Then
/// a reformat of a file nobody moved, to make sure a whitespace-only change is not mistaken for a move, and
/// one rename with no content change at all in a later turn.
pub fn rename_storm() -> Script {
    let mut script = Script::new("rename_storm");
    scaffold(&mut script);
    script
        .commit("Publish the module tree")
        .by(&PI)
        .turn("rename", "1")
        .trailer("Agent-Intent", "publish the module tree")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/core/mod.rs", core_mod(false))
        .write("src/core/store.rs", store_mod(false))
        .write("src/api/routes.rs", content::routes(0))
        .write("src/queue.rs", scheduler_rs(0))
        .write("tests/api.rs", tests_api(true))
        .done();
    script
        .commit("Rename the modules to what they do")
        .by(&PI)
        .turn("rename", "1")
        .trailer("Agent-Intent", "rename the modules to what they do")
        .rename("src/core/store.rs", "src/store/mod.rs")
        .rename("src/api/routes.rs", "src/api/handlers.rs")
        .rename("src/queue.rs", "src/scheduler.rs")
        .rename("tests/api.rs", "tests/query.rs")
        .done();
    script
        .commit("Rename the api router and fold the cache in")
        .by(&PI)
        .turn("rename", "2")
        .trailer("Agent-Intent", "rename the api router")
        .rename("src/api/handlers.rs", "src/http/handlers.rs")
        .write("src/http/handlers.rs", content::routes(2))
        .done();
    script
        .commit("Point the docs and the crate root at the new names")
        .by(&PI)
        .turn("rename", "3")
        .trailer("Agent-Intent", "point the docs at the new names")
        .write("README.md", readme("http::handlers"))
        .write("src/lib.rs", content::lib_rs("http"))
        .done();
    script
        .commit("Refresh the CI image")
        .by(&HUMAN_REVIEWER)
        .write(".github/workflows/ci.yml", ci_yml(true))
        .done();
    script
        .commit("Reformat the handlers module")
        .by(&PI)
        .turn("rename", "4")
        .trailer("Agent-Intent", "reformat the handlers module")
        .write(
            "src/http/handlers.rs",
            content::reformatted(&content::routes(2)),
        )
        .done();
    script
        .commit("Name the scheduler module after its job")
        .by(&PI)
        .turn("rename", "4")
        .trailer("Agent-Intent", "name the scheduler module after its job")
        .rename("src/scheduler.rs", "src/scheduler/mod.rs")
        .done();
    script
}

/// Direct, transitive, added, removed, lockfile-only and npm dependency changes, one story per turn.
///
/// A dependency-bump detector has four separate failure modes and a corpus that only exercises one of them
/// will ship the other three: it can miss a bump that only moves the lockfile, it can call a lockfile-only
/// commit a code change, it can miss a transitive-only bump, and it can call a bot's bump a human episode.
pub fn dependency_bump_storm() -> Script {
    let mut script = Script::new("dependency_bump_storm");
    scaffold(&mut script);
    script
        .commit("Bump regex to 1.11.0")
        .by(&PI)
        .turn("deps", "1")
        .trailer("Agent-Intent", "bump regex")
        .write("Cargo.toml", bump("regex", "1.11.0"))
        .done();
    script
        .commit("Update the lockfile for the regex bump")
        .by(&PI)
        .turn("deps", "1")
        .trailer("Agent-Intent", "bump regex")
        .write(
            "Cargo.lock",
            cargo_lock(&[
                ("serde", "1.0.219"),
                ("serde_json", "1.0.140"),
                ("regex", "1.11.0"),
                ("thiserror", "2.0.11"),
            ]),
        )
        .done();
    script
        .commit("Bump express to 5.1.0 in the front end")
        .by(&PI)
        .turn("deps", "2")
        .trailer("Agent-Intent", "bump express")
        .write("web/package.json", package_json("5.1.0", false))
        .done();
    script
        .commit("Update the npm lockfile, express and its body-parser")
        .by(&PI)
        .turn("deps", "2")
        .trailer("Agent-Intent", "bump express")
        .write("web/package-lock.json", package_lock("5.1.0", "2.0.2"))
        .done();
    script
        .commit("Pin the reporter's requirements")
        .by(&HUMAN_REVIEWER)
        .write("services/reporter/requirements.txt", requirements(true))
        .write("services/reporter/report.py", report_py(0))
        .done();
    script
        .commit("Add quick-xml for the export path")
        .by(&PI)
        .turn("deps", "3")
        .trailer("Agent-Intent", "add quick-xml")
        .write(
            "Cargo.toml",
            cargo_toml_plus(("quick-xml", "0.37.0", "\"serialize\"")),
        )
        .done();
    script
        .commit("Drop thiserror, nothing uses it")
        .by(&PI)
        .turn("deps", "3")
        .trailer("Agent-Intent", "drop thiserror")
        .write("Cargo.toml", without("thiserror"))
        .done();
    script
        .commit("chore(deps): bump serde to 1.0.228")
        .by(&DEPS_BOT)
        .turn("deps", "4")
        .write("Cargo.toml", bump("serde", "1.0.228"))
        .done();
    script
        .commit("Revert the serde bump, it breaks the build")
        .by(&HUMAN_MAINTAINER)
        .at(6 * 3_600)
        .write(
            "Cargo.toml",
            cargo_toml_plus(("quick-xml", "0.37.0", "\"serialize\"")),
        )
        .done();
    script
        .commit("Regenerate the lockfile after the revert")
        .by(&PI)
        .turn("deps", "5")
        .trailer("Agent-Intent", "regenerate the lockfile")
        .write(
            "Cargo.lock",
            cargo_lock(&[
                ("serde", "1.0.219"),
                ("serde_json", "1.0.140"),
                ("regex", "1.11.0"),
                ("quick-xml", "0.37.0"),
            ]),
        )
        .done();
    script
}

/// A clean three-layer project in which one commit closes a cycle, and later ones open two more.
///
/// The layering is stated in the content (`core` → `store` → `api`, the api may import anything below it) and
/// the back edges are single deliberate functions whose doc comments say why they are wrong. Two kinds of
/// cycle are present, because a detector that finds one will not necessarily find the other: an import cycle
/// that exists in the current tree, and one that only exists because a file the module still declares was
/// deleted.
pub fn cycle_introduced() -> Script {
    let mut script = Script::new("cycle_introduced");
    scaffold(&mut script);
    script
        .commit("Publish the core and the store")
        .by(&PI)
        .turn("layers", "1")
        .trailer("Agent-Intent", "publish the core and the store")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/core/mod.rs", core_mod(false))
        .write("src/store/mod.rs", store_mod(false))
        .done();
    script
        .commit("Expose the query surface")
        .by(&PI)
        .turn("layers", "1")
        .trailer("Agent-Intent", "expose the query surface")
        .write("src/api/mod.rs", api_mod("routes"))
        .write("src/api/routes.rs", content::routes(0))
        .done();
    script
        .commit("Add the scheduler and its queue")
        .by(&CODEX)
        .turn("layers", "2")
        .trailer("Agent-Intent", "add the scheduler")
        .write("src/scheduler.rs", scheduler_rs(0))
        .write("src/queue.rs", content::queue_rs(0))
        .done();
    script
        .commit("Tell the api when the model moved")
        .by(&PI)
        .turn("layers", "3")
        .trailer("Agent-Intent", "tell the api when the model moved")
        .write("src/core/mod.rs", core_mod(true))
        .done();
    script
        .commit("Invalidate cached answers on a notice")
        .by(&PI)
        .turn("layers", "3")
        .trailer("Agent-Intent", "invalidate cached answers on a notice")
        .write("src/store/mod.rs", store_mod(true))
        .done();
    script
        .commit("Drop the duplicate query helpers")
        .by(&PI)
        .turn("layers", "4")
        .trailer("Agent-Intent", "drop the duplicate query helpers")
        .delete("src/api/routes.rs")
        .done();
    script
}

/// Tests deleted, tests weakened to `#[ignore]`, tests put back, and tests added beside the module.
///
/// The four shapes have to be told apart and only two of them are deletions: five tests became three, three
/// became `#[ignore]`d stubs whose assertions are gone, a file disappeared entirely, and a human restored
/// what the agent removed. A `test_deletion` finding that fires on the restoration is a false positive the
/// ground truth marks as forbidden.
pub fn test_deletion() -> Script {
    let mut script = Script::new("test_deletion");
    scaffold(&mut script);
    script
        .commit("Publish the core and its tests")
        .by(&PI)
        .turn("tests", "1")
        .trailer("Agent-Intent", "publish the core and its tests")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/core/mod.rs", core_mod(false))
        .write("tests/core.rs", tests_core(5))
        .write("tests/api.rs", tests_api(true))
        .done();
    script
        .commit("Trim the core suite to what passes")
        .by(&PI)
        .turn("tests", "2")
        .trailer("Agent-Intent", "make the suite green")
        .write("tests/core.rs", tests_core(3))
        .done();
    script
        .commit("Skip the flaky ones instead of fixing them")
        .by(&PI)
        .turn("tests", "3")
        .trailer("Agent-Intent", "make the suite green")
        .write("tests/core.rs", tests_core_ignored(3))
        .done();
    script
        .commit("Delete the api integration tests")
        .by(&CODEX)
        .turn("tests", "4")
        .trailer("Agent-Intent", "delete the failing api tests")
        .delete("tests/api.rs")
        .done();
    script
        .commit("Put back the coverage that was removed")
        .by(&HUMAN_REVIEWER)
        .at(5 * 3_600)
        .write("tests/core.rs", tests_core(5))
        .write("tests/api.rs", tests_api(true))
        .done();
    script
        .commit("Add a store test beside the store")
        .by(&PI)
        .turn("tests", "5")
        .trailer("Agent-Intent", "add a store test")
        .write(
            "src/store/tests.rs",
            tests_for("store", "keeps_the_segment"),
        )
        .done();
    script
}

/// Reformat-only, reorder-only and lockfile-only churn, plus one episode that is only *partly* noise.
///
/// The partly case is the one that decides whether a noise detector is usable: the same turn that reformatted
/// two files also fixed a real parsing bug in a third, and an episode that calls that noise is an episode
/// that hides a fix. The gap between the two halves is five hours, which is where the time-gap rule is
/// supposed to draw a line the message-based rules do not.
pub fn reformat_only() -> Script {
    let mut script = Script::new("reformat_only");
    scaffold(&mut script);
    publish(&mut script, 0);
    script
        .commit("Run the formatter across the crate")
        .by(&PI)
        .turn("format", "1")
        .trailer("Agent-Intent", "run the formatter")
        .write("src/core/mod.rs", content::reformatted(&core_mod(false)))
        .write("src/api/mod.rs", content::reformatted(&api_mod("routes")))
        .write(
            "src/api/routes.rs",
            content::reformatted(&content::routes(0)),
        )
        .write("src/parse.rs", content::reformatted(&parse_rs(0)))
        .write("src/render.rs", content::reformatted(&render_rs(0)))
        .done();
    script
        .commit("Reorder the items in parse")
        .by(&PI)
        .turn("format", "1")
        .trailer("Agent-Intent", "run the formatter")
        .write("src/parse.rs", content::reordered(&parse_rs(0)))
        .done();
    script
        .commit("Regenerate the lockfile")
        .by(&PI)
        .turn("format", "1")
        .trailer("Agent-Intent", "run the formatter")
        .write(
            "Cargo.lock",
            cargo_lock(&[
                ("serde", "1.0.219"),
                ("serde_json", "1.0.140"),
                ("regex", "1.10.6"),
                ("thiserror", "2.0.11"),
            ]),
        )
        .done();
    script
        .commit("Fix the typo in the README")
        .by(&HUMAN_MAINTAINER)
        .at(5 * 3_600)
        .write("README.md", readme("api::handlers"))
        .done();
    script
        .commit("Reformat and fix the trailing comma at the same time")
        .by(&PI)
        .turn("format", "2")
        .at(6 * 3_600)
        .trailer("Agent-Intent", "fix the trailing comma the review found")
        .write("src/parse.rs", parse_rs(1))
        .write("src/main.rs", content::reformatted(&main_rs()))
        .done();
    script
}

/// An episode that touched a great deal more than the intent its trailer declared.
///
/// `Agent-Intent:` is the only place a fixture can state what the agent was *asked* for, and scope creep is
/// the gap between that and what arrived. The first offender changes one parser and then, in the same turn,
/// reindents an unrelated renderer, adds two hundred and forty lines of notes, leaves a four-hundred-line
/// debug log behind, turns CI benching on, and edits `.envrc`. The second re-cleans the reporter and commits
/// a notebook. Both are the shape of an agent that kept going.
pub fn scope_creep() -> Script {
    let mut script = Script::new("scope_creep");
    scaffold(&mut script);
    script
        .commit("Publish parse and render")
        .by(&PI)
        .turn("creep", "1")
        .trailer("Agent-Intent", "publish parse and render")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/parse.rs", parse_rs(0))
        .write("src/render.rs", render_rs(0))
        .done();
    script
        .commit("Fix the trailing comma the review found")
        .by(&PI)
        .turn("creep", "2")
        .trailer("Agent-Intent", "fix the trailing comma in parse")
        // A hook the contract has no meaning for, so the log carries it with a null projection and the
        // ingest counts it (§2: counted, not dropped). This is the only place in the corpus where one
        // appears, which is why it is commented.
        .trailer("fathom-hook", "WorktreeCreate")
        .write("src/parse.rs", parse_rs(1))
        .write("src/render.rs", render_rs(1))
        .write("docs/notes.md", notes_md(240))
        .write("debug.log", debug_log_txt(400))
        .write(".envrc", content::envrc(1))
        .write(".github/workflows/ci.yml", ci_yml(true))
        .done();
    script
        .commit("Update the reporter for the new format")
        .by(&CODEX)
        .turn("creep", "3")
        .trailer("Agent-Intent", "update the nightly report")
        // The turn that ran out of budget: the failure is on the record, so a claim about an abandoned
        // episode has evidence that is not a commit message.
        .trailer("fathom-hook", "StopFailure")
        .write("services/reporter/report.py", report_pandas())
        .write("services/reporter/requirements.txt", requirements(false))
        .write("notebooks/exploration.ipynb", notebook_ipynb())
        .write("src/scheduler.rs", scheduler_rs(1))
        .write("config/scheduler.toml", scheduler_toml(1))
        .done();
    script
        .commit("Drop the files that should never have been committed")
        .by(&HUMAN_REVIEWER)
        .at(4 * 3_600)
        .delete("debug.log")
        .delete("notebooks/exploration.ipynb")
        .delete("docs/notes.md")
        .done();
    script
}

/// The same change twice: the branch that was merged, and the rewritten copy of it that is also still there.
///
/// Both lineages exist because both are reachable — the original on its own ref, the rewrite on `main` — which
/// is the state a repository is really in immediately after somebody force-pushes and the merge lands anyway.
/// The commits are different objects with the same effect: same tree content, same subjects, different parents
/// and different SHAs. Everything a duplicate-detector does wrong happens here: hash-compare misses it, patch-
/// id compare finds it, and an episode that reports both as separate work double-counts the turn.
pub fn rebase_rewrite() -> Script {
    let mut script = Script::new("rebase_rewrite");
    scaffold(&mut script);
    script
        .commit("Add the retry budget")
        .on("orig")
        .fork("orig", "main")
        .by(&PI)
        .turn("orig", "1")
        .trailer("Agent-Intent", "add a retry budget")
        .write("src/scheduler.rs", scheduler_with_retry())
        .done();
    script
        .commit("Add the retry policy")
        .on("orig")
        .by(&PI)
        .turn("orig", "1")
        .trailer("Agent-Intent", "add a retry budget")
        .write("src/scheduler.rs", scheduler_with_policy())
        .done();
    script
        .commit("Document the retries")
        .on("orig")
        .by(&PI)
        .turn("orig", "2")
        .trailer("Agent-Intent", "document the retries")
        .write("docs/retries.md", doc_page("Retries"))
        .done();
    script
        .commit("Add an architecture note")
        .by(&HUMAN_MAINTAINER)
        .write("docs/architecture.md", doc_page("Architecture"))
        .done();
    script
        .commit("Add the retry budget")
        .by(&PI)
        .turn("rewrite", "3")
        .trailer("Agent-Intent", "add a retry budget")
        .write("src/scheduler.rs", scheduler_with_retry())
        .done();
    script
        .commit("Add the retry policy")
        .by(&PI)
        .turn("rewrite", "3")
        .trailer("Agent-Intent", "add a retry budget")
        .write("src/scheduler.rs", scheduler_with_policy())
        .done();
    script
        .commit("Document the retries")
        .by(&PI)
        .turn("rewrite", "4")
        .trailer("Agent-Intent", "document the retries")
        .write("docs/retries.md", doc_page("Retries"))
        .done();
    script
        .commit("Merge the original branch after all")
        .by(&HUMAN_MAINTAINER)
        .merge_of("main", "orig")
        .done();
    script
}

/// The same region of the same two files argued over across six commits in one turn, then reverted.
///
/// The line counts are the point: each round edits the two lines that decide the window size and the spin
/// behaviour, so the gross churn across the turn is large and the net is nothing — the last round puts back
/// what the first round had. A churn detector that looks at net change sees a clean turn; one that looks at
/// gross change sees six edits to the same four lines. Both are true, and the ground truth says which finding
/// belongs to which.
pub fn churn_loop() -> Script {
    let mut script = Script::new("churn_loop");
    scaffold(&mut script);
    script
        .commit("Add the scheduler and the queue")
        .by(&PI)
        .turn("churn", "1")
        .trailer("Agent-Intent", "add the scheduler")
        .write("src/scheduler.rs", scheduler_rs(0))
        .write("src/queue.rs", content::queue_rs(0))
        .done();
    for round in 1..=6 {
        script
            .commit(&format!("Round {round}: settle the window and the spin"))
            .by(&PI)
            .turn("churn", "2")
            .trailer("Agent-Intent", "settle the drain loop")
            .write("src/scheduler.rs", scheduler_rs(round))
            .write("src/queue.rs", content::queue_rs(round))
            .done();
    }
    script
        .commit("Settle on the original window")
        .by(&PI)
        .turn("churn", "3")
        .trailer("Agent-Intent", "put the original window back")
        .write("src/scheduler.rs", scheduler_rs(0))
        .write("src/queue.rs", content::queue_rs(0))
        .done();
    script
        .commit("Write down what the drain loop decided")
        .by(&HUMAN_REVIEWER)
        .write("docs/drain.md", doc_page("The drain loop"))
        .done();
    script
}

/// A nine-megabyte generated JSON file, a 600 KB bundle, a binary blob, a hand edit, and a regeneration.
///
/// Four things are being tested at once. The big file is over `fathom-ingest`'s default in-memory blob cap,
/// so a fixture exists where the cap actually applies; the bundle and the blob are files no text pipeline may
/// read; the hand edit and the regeneration are the pair a generated-file detector has to tell apart, because
/// only one of them is the generator's output; and the whole thing gets `Events::First`, because nobody wants
/// a nine-megabyte file's worth of events in every run.
pub fn huge_file() -> Script {
    let mut script = Script::new("huge_file");
    script.events = Events::First(3);
    scaffold(&mut script);
    let description = openapi_json(80_000);
    script
        .commit("Vendor the generated API description")
        .by(&PI)
        .turn("vendor", "1")
        .trailer("Agent-Intent", "vendor the API description")
        .write("openapi/openapi.json", description.clone())
        .done();
    script
        .commit("Vendor the dashboard bundle and its logo")
        .by(&PI)
        .turn("vendor", "2")
        .trailer("Agent-Intent", "vendor the dashboard")
        .write("web/dist/app.min.js", bundle_min(600_000))
        .write("web/assets/logo.bin", binary_blob(64_000))
        .done();
    script
        .commit("chore(deps): record the release checksums")
        .by(&DEPS_BOT)
        .turn("vendor", "3")
        .write(
            "Cargo.lock",
            cargo_lock(&[
                ("serde", "1.0.219"),
                ("serde_json", "1.0.140"),
                ("regex", "1.10.6"),
                ("thiserror", "2.0.11"),
            ]),
        )
        .done();
    script
        .commit("Fix one summary in the API description by hand")
        .by(&CODEX)
        .turn("vendor", "4")
        .trailer("Agent-Intent", "fix one summary in the API description")
        .write("openapi/openapi.json", hand_edited(&description))
        .done();
    script
        .commit("Regenerate the API description")
        .by(&CODEX)
        .turn("vendor", "5")
        .trailer("Agent-Intent", "regenerate the API description")
        .write("openapi/openapi.json", openapi_json(80_000))
        .done();
    script
}

/// Paths that are not ASCII, and a rename between two of them that differ only by normalisation form.
///
/// The two café pages are spelled with escapes in `content`, never as literals, because the difference between
/// them is one Unicode form and no editor, filesystem or git filter should get to decide which one the fixture
/// meant. They are never both in the tree at once — on a normalising filesystem they are the same file, and a
/// fixture that only works on filesystems that preserve bytes is a fixture with a platform-specific test in
/// it. The rename from the composed form to the decomposed one is what a rename detector has to report as a
/// rename and not as a deletion plus an addition.
pub fn unicode_paths() -> Script {
    let names = unicode_names();
    let (composed, decomposed, japanese, dashes, cyrillic) =
        (names[0], names[1], names[2], names[3], names[4]);
    let mut script = Script::new("unicode_paths");
    scaffold(&mut script);
    script
        .commit("Add the translated docs")
        .by(&PI)
        .turn("i18n", "1")
        .trailer("Agent-Intent", "add the translated docs")
        .write(composed, doc_page("Café"))
        .write(japanese, doc_page("日本語"))
        .done();
    script
        .commit("Add the Cyrillic-named module")
        .by(&PI)
        .turn("i18n", "2")
        .trailer("Agent-Intent", "add the Cyrillic-named module")
        .write("src/lib.rs", content::lib_rs("api"))
        .write(
            cyrillic,
            module_with_hint("cyrillic", "A module named in Cyrillic."),
        )
        .done();
    script
        .commit("Rename the accented page to the decomposed form")
        .by(&PI)
        .turn("i18n", "3")
        .trailer("Agent-Intent", "rename the accented page")
        .rename(composed, decomposed)
        .done();
    script
        .commit("Add the note with the en dash in its name")
        .by(&HUMAN_REVIEWER)
        .write(dashes, doc_page("Naïve notes"))
        .done();
    script
        .commit("Rewrite the Japanese page for the new layout")
        .by(&CODEX)
        .turn("i18n", "4")
        .trailer("Agent-Intent", "rewrite the Japanese page")
        .write(japanese, doc_page("日本語 — 新版"))
        .done();
    script
}

/// Five branches, five merges, one resolved conflict, and one branch nobody merged.
///
/// Merge commits are where an episode's author is least trustworthy: a merge's author is whoever typed
/// `git merge`, not whoever wrote the code, and a merge of an agent's branch by a human can steal a whole
/// turn's work if the segmentation is naive about it. The conflict resolution is the opposite case — a human
/// genuinely wrote those lines, in the merge commit, and they appear nowhere else. The unmerged branch is the
/// case a history walk silently drops.
pub fn many_merges() -> Script {
    let mut script = Script::new("many_merges");
    scaffold(&mut script);
    publish(&mut script, 0);
    script
        .commit("Add the retry budget to the scheduler")
        .fork("feat/a", "main")
        .by(&PI)
        .turn("feat-a", "1")
        .trailer("Agent-Intent", "add the retry budget")
        .write("src/scheduler.rs", scheduler_with_retry())
        .done();
    script
        .commit("Give the queue a bound")
        .fork("feat/b", "main")
        .by(&CODEX)
        .turn("feat-b", "1")
        .trailer("Agent-Intent", "bound the queue")
        .write("src/queue.rs", content::queue_rs(1))
        .done();
    script
        .commit("Merge the retry budget")
        .by(&HUMAN_MAINTAINER)
        .merge_of("main", "feat/a")
        .done();
    script
        .commit("Merge the queue bound")
        .by(&HUMAN_MAINTAINER)
        .merge_of("main", "feat/b")
        .done();
    script
        .commit("Widen the scheduler window")
        .fork("feat/c", "main")
        .by(&PI)
        .turn("feat-c", "1")
        .trailer("Agent-Intent", "widen the window")
        .write("src/scheduler.rs", scheduler_rs(2))
        .done();
    script
        .commit("Change the spin behaviour at the window's edge")
        .fork("feat/d", "main")
        .by(&CODEX)
        .turn("feat-d", "1")
        .trailer("Agent-Intent", "change the spin behaviour")
        .write("src/scheduler.rs", scheduler_rs(3))
        .done();
    script
        .commit("Merge the wider window")
        .by(&PI)
        .turn("feat-c", "2")
        .merge_of("main", "feat/c")
        .done();
    script
        .commit("Merge the spin change and resolve the conflict")
        .by(&HUMAN_REVIEWER)
        .merge_of("main", "feat/d")
        .write("src/scheduler.rs", scheduler_rs(4))
        .done();
    script
        .commit("Add a page about merging")
        .fork("feat/e", "main")
        .by(&HUMAN_MAINTAINER)
        .write("docs/merging.md", doc_page("Merging"))
        .done();
    script
        .commit("Merge the merging page")
        .by(&HUMAN_MAINTAINER)
        .merge_of("main", "feat/e")
        .done();
    script
        .commit("Start the export feature")
        .fork("feat/never", "main")
        .by(&PI)
        .turn("feat-never", "1")
        .trailer("Agent-Intent", "start the export feature")
        .write("src/export.rs", scratch_rs(3))
        .done();
    script
}

/// Ten thousand commits in one hundred turns, for the performance budget and the scale cases.
///
/// The files stay few and the commits stay many, which is the opposite of how a real import looks and is done
/// on purpose: the cost of a fixture is paid per file per commit in tree rewriting and digesting, and a
/// scenario with ten thousand files would spend the entire CI budget on the fixture instead of on the thing
/// being timed. What this scenario is for is segmenting ten thousand commits, holding ten thousand episodes,
/// and the turn-spans that are wider than any hand-written scenario can reach.
///
/// It writes no event log. A stream of that size would be the largest artifact in the corpus, and ADR-005's
/// truncation rule is what `huge_file` tests instead.
pub fn ten_k_commits() -> Script {
    const GROUPS: usize = 100;
    const PER_GROUP: usize = 100;
    let mut script = Script::new("ten_k_commits");
    script.events = Events::None;
    scaffold(&mut script);
    script
        .commit("Publish the import surface")
        .by(&PI)
        .turn("import", "0")
        .trailer("Agent-Intent", "publish the import surface")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/imported.rs", content::imported_rs(0))
        .done();
    for group in 0..GROUPS {
        for index in 0..PER_GROUP {
            let n = group * PER_GROUP + index;
            script
                .commit(&format!("import batch {n}"))
                .by(&DEPS_BOT)
                .turn("import", &format!("g{group}"))
                .write("src/imported.rs", content::imported_rs(n))
                .done();
        }
    }
    script
        .commit("Check the import finished")
        .by(&HUMAN_MAINTAINER)
        .write("docs/import.md", migration_md(1))
        .done();
    script
}

/// A submodule: the upstream's own history, a gitlink pointing into it, and a bump of that pointer.
///
/// The companion repository is built into a sibling directory before the main one, because a gitlink names a
/// commit and a fixture that pointed at a commit that does not exist would be a broken repository rather than
/// a fixture. `git fsck` inside the result is the check that the gitlink was written the way git wants it;
/// the ground truth claims which upstream commit each gitlink names.
pub fn submodule() -> Script {
    let mut upstream = Script::new("upstream");
    upstream.events = Events::None;
    scaffold(&mut upstream);
    upstream
        .commit("Publish the upstream core")
        .by(&HUMAN_MAINTAINER)
        .write(
            "upstream/mod.rs",
            module_with_hint("upstream", "The upstream core."),
        )
        .done();
    upstream
        .commit("Fix the boundary check")
        .by(&PI)
        .turn("upstream", "1")
        .trailer("Agent-Intent", "fix the boundary check")
        .write(
            "upstream/mod.rs",
            module_with_hint("upstream", "The upstream core, with the fix."),
        )
        .done();
    upstream
        .commit("Release 0.2.0")
        .by(&HUMAN_MAINTAINER)
        .tag("v0.2.0")
        .write(
            "upstream/mod.rs",
            module_with_hint("upstream", "The upstream core, released."),
        )
        .done();

    let mut script = Script::new("submodule");
    script.companion(Companion {
        name: "upstream".to_owned(),
        script: upstream,
        location: Location::Sibling,
    });
    scaffold(&mut script);
    script
        .commit("Publish the module tree")
        .by(&PI)
        .turn("sub", "1")
        .trailer("Agent-Intent", "publish the module tree")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/core/mod.rs", core_mod(false))
        .done();
    script
        .commit("Vendor the upstream as a submodule")
        .by(&PI)
        .turn("sub", "2")
        .trailer("Agent-Intent", "vendor the upstream")
        .submodule(
            "vendor/upstream",
            "https://example.invalid/lumen-upstream.git",
            "upstream",
            Gitlink::Index(1),
        )
        .done();
    script
        .commit("Point the submodule at the released upstream")
        .by(&PI)
        .turn("sub", "3")
        .trailer("Agent-Intent", "bump the submodule")
        .submodule(
            "vendor/upstream",
            "https://example.invalid/lumen-upstream.git",
            "upstream",
            Gitlink::Tip,
        )
        .done();
    script
        .commit("Say where the upstream came from")
        .by(&HUMAN_REVIEWER)
        .write("docs/upstream.md", doc_page("The upstream"))
        .done();
    script
}

/// A depth-three clone of a history whose ancestors are genuinely absent.
///
/// The visible commits are rewritten copies: the window's first commit has no parent at all, because there is
/// nothing to point at, and `.git/shallow` names the tip. A history walker that walks to the root instead of
/// to the shallow cut will ask for an object that is not in the repository and fail with a message nobody in
/// the product can act on, so the corpus contains the shape and the ground truth says which commits are the
/// cut.
pub fn shallow_clone() -> Script {
    const DEPTH: usize = 3;
    let mut upstream = Script::new("upstream");
    upstream.events = Events::None;
    scaffold(&mut upstream);
    for round in 0..9 {
        upstream
            .commit(&format!("Upstream work round {round}"))
            .by(if round % 3 == 0 {
                &HUMAN_MAINTAINER
            } else {
                &PI
            })
            .turn("upstream", &format!("r{round}"))
            .trailer("Agent-Intent", "do the upstream work")
            .write(
                "upstream/mod.rs",
                module_with_hint("upstream", &format!("Round {round}.")),
            )
            .write("docs/upstream.md", migration_md(round))
            .done();
    }

    let mut script = Script::new("shallow_clone");
    script.companion(Companion {
        name: "upstream".to_owned(),
        script: upstream,
        location: Location::Sibling,
    });
    script.extra(Extra::ShallowClone {
        companion: "upstream".to_owned(),
        depth: DEPTH,
        branch: "main".to_owned(),
    });
    script
}

/// Nothing wrong with it at all.
///
/// Every detector in the product is claimed to be silent on a tidy repository, and a corpus made only of
/// pathological cases would let a detector that fires on everything score perfectly. This is the control:
/// one agent turn that adds a module and its test, a human doc commit, a dependency bump that was done
/// properly with its lockfile, and an unrelated pair of files in one commit, which is the only thing here a
/// detector is allowed to call unusual.
pub fn clean_baseline() -> Script {
    let mut script = Script::new("clean_baseline");
    scaffold(&mut script);
    script
        .commit("Publish the module tree")
        .by(&PI)
        .turn("clean", "1")
        .trailer("Agent-Intent", "publish the module tree")
        .write("src/lib.rs", content::lib_rs("api"))
        .write("src/main.rs", main_rs())
        .write("src/core/mod.rs", core_mod(false))
        .write("src/store/mod.rs", store_mod(false))
        .write("src/api/mod.rs", api_mod("routes"))
        .write("src/api/routes.rs", content::routes(1))
        .done();
    script
        .commit("Add the export module and its test")
        .by(&PI)
        .turn("clean", "2")
        .trailer("Agent-Intent", "add the export module")
        .write(
            "src/export.rs",
            module_with_hint("export", "Write samples out."),
        )
        .write("tests/export.rs", tests_for("export", "writes_a_sample"))
        .done();
    script
        .commit("Bump regex to 1.11.0 and update the lockfile")
        .by(&PI)
        .turn("clean", "3")
        .trailer("Agent-Intent", "bump regex")
        .write("Cargo.toml", bump("regex", "1.11.0"))
        .write(
            "Cargo.lock",
            cargo_lock(&[
                ("serde", "1.0.219"),
                ("serde_json", "1.0.140"),
                ("regex", "1.11.0"),
                ("thiserror", "2.0.11"),
            ]),
        )
        .done();
    script
        .commit("Document the export path and note the window setting")
        .by(&HUMAN_REVIEWER)
        .write("docs/export.md", doc_page("Export"))
        .write("config/scheduler.toml", scheduler_toml(0))
        .done();
    script
}

/// The committers, named here so that adding a scenario that uses one of them does not also have to touch
/// the import list, and so that an unused one is a comment rather than a warning.
const _: &[&Identity] = &[&HUMAN_MAINTAINER, &HUMAN_REVIEWER, &PI, &CODEX, &DEPS_BOT];
