#!/usr/bin/env python3
"""Generator for the Fathom delivery plan -> beads graph JSON.

Fathom is a TUI that helps a human keep a working mental model of a codebase
that a coding agent is writing. This file is the single source of truth for the
ticket graph; `bd create --graph` consumes its output.

Usage:
    python3 .plan/gen.py > .plan/plan.json
    bd create --graph .plan/plan.json --dry-run
    bd create --graph .plan/plan.json

Conventions:
  * keys are stable local placeholders (e0.3); bd assigns real IDs on create.
  * deps are 'blocks' edges: this node cannot start until the target is done.
  * parent is hierarchy only (does not block), so children become ready on merit.
  * priority 1 = must happen for the thesis to work, 2 = core product,
    3 = hardening/polish, 4 = deferred/nice-to-have.
"""
import json
import sys

NODES = []


def t(key, parent, title, typ="task", prio=2, est=None, deps=None, desc="",
      ac="", notes=None, labels=None, **extra):
    n = {"key": key, "title": title, "type": typ, "priority": prio}
    if parent:
        n["parent"] = parent
    if est:
        n["estimate"] = est
    if desc:
        n["description"] = desc
    if ac:
        n["acceptance_criteria"] = ac
    if notes:
        n["notes"] = notes
    if deps:
        n["deps"] = [{"target": d, "type": "blocks"} for d in deps]
    if labels:
        n["labels"] = labels
    n.update(extra)   # per-ticket extras the graph format allows (e.g. e0.5's contract)
    NODES.append(n)
    return key


def bullet(*items):
    return "\n".join("- " + i for i in items)


# ===================================================================== EPICS
EPICS = [
 ("e0", "Epic 0 - Foundations: thesis, architecture decisions, workspace, fixtures", 1,
  "Everything that must exist before product code is written: the product thesis and glossary, "
  "six architecture decision records, the cargo workspace, the config/path-taxonomy system and the "
  "deterministic fixture corpus that every later ticket tests against.\n\n"
  "Exit criteria: a clean clone builds and runs `fathom --help`; CI is green on linux/macos/windows; "
  "all six ADRs are accepted; a synthetic agent-driven repo can be indexed end-to-end and the perf "
  "budget gate is wired into CI."),
 ("e1", "Epic 1 - Git spine: ingestion, episodes, timeline and diff viewing", 1,
  "The MVP backbone. Fathom observes a repository, turns git history into 'episodes' (one agent turn "
  "= one reviewable unit) and lets a human fly through the timeline and read diffs without leaving "
  "the TUI.\n\nExit criteria: a human reviews a 500-commit agent-driven fixture repo entirely inside "
  "the TUI; warm re-index after a new commit is under 150ms; episode identity and review state survive "
  "rebase/amend/force-push."),
 ("e2", "Epic 2 - Change intelligence: taxonomy, structure, dependencies and noise suppression", 1,
  "Turn raw diffs into named, evidence-backed observations: which parts of the tree are in motion, "
  "what moved or was renamed, which dependencies appeared or changed, which non-code surfaces "
  "(config, migrations, CI) were touched, and what is pure noise (generated files, reformat, lockfile "
  "churn).\n\nExit criteria: every episode answers WHERE the change landed and WHAT it now depends on, "
  "with every number clickable to its evidence. Noise is suppressed by default and never silently hidden."),
 ("e3", "Epic 3 - Architecture awareness: module graph, cycles, layering and API surface", 2,
  "The differentiator. Build a per-revision model of modules, symbols and imports, then show the DELTA "
  "of that model per agent turn: new edges, new cycles, layering violations, public API changes, god "
  "modules and hotspots.\n\nExit criteria: a reviewer answers 'did the agent just make the architecture "
  "worse?' in under five seconds, and CI can fail on new violations only via a ratchet baseline."),
 ("e4", "Epic 4 - Agent awareness: attribution, intent, pathology detection and the attention queue", 2,
  "Make the tool agent-native instead of a generic git viewer: attribution, captured intent, detectors "
  "for agent pathologies (churn loops, scope creep, untested additions, shotgun edits, rework loops), "
  "an explainable per-episode risk score and a durable review ledger.\n\nExit criteria: the attention "
  "queue ranks the episodes worth a human's time, each with human-readable reasons, and sign-offs "
  "survive history rewrites."),
 ("e5", "Epic 5 - Ergonomics: ambient mode, search, comparison, export and day-long usability", 2,
  "Fathom lives in a second terminal next to the coding harness, all day. It must be glanceable, "
  "keyboard-first, usable at 80x24, and able to export a review artefact that a human can post on a PR "
  "as the record of what was reviewed and accepted.\n\nExit criteria: an entire review session, from "
  "notification to signed-off artefact, happens without leaving the keyboard."),
 ("e6", "Epic 6 - Harness integration: hooks, daemon, MCP server and CI gate", 2,
  "Fathom is the review half of an agentic delivery system. It ingests harness events, runs as a "
  "budgeted daemon, exposes the same model back to the coding agent over MCP, and gates CI on new "
  "architecture/dependency/risk violations.\n\nExit criteria: a working adapter for at least two real "
  "harnesses, an MCP session where the agent queries 'what did I just change and is it risky', and a CI "
  "job that fails only on new violations."),
 ("e7", "Epic 7 - Hardening, performance, packaging, docs and beta", 3,
  "Real repositories are messy and enormous. This epic covers failure tolerance, perf budgets on real "
  "OSS repos, memory ceilings, cross-platform packaging, documentation, a demo, security/privacy review "
  "and a beta feedback loop.\n\nExit criteria: published binaries for three OSes, docs a stranger can "
  "follow in five minutes, perf budgets met or consciously relaxed with a written rationale, and a "
  "closed beta feedback loop."),
]
for k, title, prio, desc in EPICS:
    head, _, exit_criteria = desc.partition("Exit criteria:")
    ac = "- Exit criteria met: " + exit_criteria.strip().rstrip(".") if exit_criteria else ""
    t(k, None, title, "epic", prio, desc=head.strip(), ac=ac,
      labels=["epic" + k[1:], "mvp"] if prio == 1 else ["epic" + k[1:]])

# ================================================================= EPIC 0
t("e0.1", "e0", "ADR-001: product thesis, glossary, north-star metric and non-goals", "decision", 1, 240,
  desc="Write the document every later ticket cites. Define the five core nouns (episode, delta, "
       "attention queue, review ledger, ambient mode), who the user is (the human accountable for "
       "software an agent wrote), and the product thesis: the bottleneck in agentic software delivery is "
       "no longer writing code, it is the human rebuilding a correct mental model of what the agent did.\n\n"
       "North-star metric: time-to-confidence (median wall-clock from episode creation to human sign-off "
       "with no subsequent rework). Guardrails: share of episodes reviewed in-session, rework rate "
       "(episodes edited or reverted within N later episodes), risk-flag false-positive rate.\n\n"
       "Explicit non-goals for v1: not a git client, not an IDE, not an agent runner, not a PR host, no "
       "cloud service, no mandatory LLM dependency.",
  ac=bullet("Doc merged as docs/concepts/thesis.md and linked from README",
            "Every term used across the plan resolves to exactly one glossary entry",
            "North-star and guardrail metrics have concrete formulas and a data source in the delta model",
            "Non-goals reviewed and agreed with the coding-harness owner"),
  labels=["adr", "docs"])

t("e0.2", "e0", "ADR-002: Rust workspace layout and crate boundaries", "decision", 1, 240,
  deps=["e0.1"],
  desc="Fix the workspace shape so eleven teams of one can work in parallel: fathom-core (domain model, "
       "no IO), fathom-store (SQLite index + cache), fathom-ingest (git + harness events), fathom-analyze "
       "(classifiers, parsers, detectors), fathom-tui (ratatui app), fathom-cli (binary + verbs), "
       "fathom-mcp (MCP server), fathom-fixtures (test repo builders).\n\n"
       "Rule: allowed dependency direction is cli -> {tui,mcp} -> analyze -> ingest -> store -> core. "
       "core has zero IO and zero unsafe. Errors with thiserror in libraries, eyre at the binary edge. "
       "Record MSRV, edition, feature-gate policy for heavy/optional deps (tree-sitter grammars, network "
       "clients) and the no-network-by-default rule.",
  ac=bullet("docs/adr/0002-workspace.md accepted with the crate dependency diagram",
            "cargo-deny + module-boundary lint in CI that fails a crate importing 'uphill'",
            "Feature-gate list published: default build pulls in zero network clients",
            "MSRV and upgrade policy stated"),
  labels=["adr"])

t("e0.3", "e0", "ADR-003: the Episode - identity, segmentation and stability across rewrites", "decision", 1, 240,
  deps=["e0.1"],
  desc="The episode is the atomic reviewable unit and everything else hangs off it, so its identity must "
       "be nailed down before anything stores a foreign key.\n\nDecide: identity scheme (content-addressed: "
       "ordered member commit SHAs plus parent boundary, plus a fuzzy content fingerprint for re-linking), "
       "boundary rules (harness turn events preferred; fallback heuristics are author identity, agent "
       "trailers, time gaps, branch/PR boundaries), behaviour under amend / rebase -i / squash / merge "
       "trains / cherry-picks, whether a partial tree (uncommitted work) is a provisional episode, and "
       "how a human commit interleaved with agent commits is treated.",
  ac=bullet("docs/adr/0003-episodes.md with worked examples for amend, rebase, squash, merge and cherry-pick",
            "Stated invariant: episode identity is stable across `git rebase -i` of the same content",
            "Tie-breaker rules for overlapping/ambiguous boundaries written down and testable",
            "Decision signed off by whoever owns the harness hook contract (e0.5)"),
  labels=["adr", "mvp"])

t("e0.4", "e0", "ADR-004: storage, indexing and cache strategy", "decision", 1, 240,
  deps=["e0.1"],
  desc="Choose and justify the local persistence model: SQLite (WAL) under .fathom/, table layout for "
       "commits, episodes, deltas, findings, sign-offs and graph snapshots; cache keys as "
       "(tree hash + analyzer version) so a code change in an analyzer invalidates exactly the right rows; "
       "incremental invalidation matrix; concurrency model (single writer, many readers, TUI attaches to a "
       "daemon); retention/pruning; corruption recovery; what must be in .gitignore; what is portable and "
       "what is disposable.\n\nAttach the perf budgets this design must meet (cold index, warm re-index, "
       "query p95, RSS ceiling) as a table, since e0.11 gates CI on it.",
  ac=bullet("docs/adr/0004-storage.md with schema draft and invalidation matrix",
            "Named perf budgets per repo size class (small/medium/large/xlarge) that e0.11 can assert on",
            "Explicit statement of what is disposable (rebuildable) vs must-survive (reviews, decisions)",
            "Recovery story tested: deleting .fathom/ rebuilds identical state (modulo reviews)"),
  labels=["adr", "mvp"])

t("e0.5", "e0", "ADR-005: harness integration contract (hooks v1 and MCP surface)", "decision", 1, 240,
  deps=["e0.1"],
  desc="Define the boundary between the coding harness and Fathom so neither has to change to fit the "
       "other.\n\nPart 1, events: append-only JSONL at .fathom/events.jsonl (path configurable), event "
       "types session_start, turn_start, turn_end, tool_call, file_edit, test_run, approval, error; required "
       "and optional fields; monotonic sequence numbers; concurrent-append and partial-line tolerance; "
       "trust model (harness output is untrusted input).\n\nPart 2, MCP: the read-only tool surface the "
       "agent may call (explain_episode, changes_between, architecture_delta, dependency_delta, "
       "risk_report, search_history) and the gated write tools (mark_reviewed, add_review_note), including "
       "who may call them and what is never returned (secrets, file contents outside the model).\n\nPart 3, "
       "versioning: how capability negotiation and additive evolution work.",
  ac=bullet("docs/adr/0005-harness-contract.md plus JSON Schema files under schemas/",
            "Two independent adapter sketches (pi-style hooks, generic git hook) judged implementable",
            "Explicit list of read vs gated-write MCP tools and their gating",
            "Contract states what happens on missing, stale or malformed events (degrade, never block the agent)"),
  labels=["adr", "integration"],
  # Carried as data because `e0.11`'s CI gate and `e4.x`'s adapters both read them, and the
  # plan file is generated: a hand-edit here would be lost by the next `gen.py` run.
  contract="docs/adr/0005-harness-contract.md",
  verification={"runtime_probe": "python3 -m recommender.harness.elf_probe --machine",
                "startup_gate": "python3 -m recommender.harness.harness run verify",
                "tests": "python3 -m pytest -q",
                "docs": "python3 tools/check_docs.py"},
  implementation=["src/recommender/harness/", "src/recommender/hooks/",
                  "tests/test_harness_contract.py"])

t("e0.6", "e0", "ADR-006: TUI information architecture, view inventory and keymap", "decision", 1, 240,
  deps=["e0.1"],
  desc="Decide the shape of the product before building screens. View inventory with pane layout, focus "
       "order and drill-in/out: Timeline (episode rail + episode card), Diff (file tree + hunk view), "
       "Structure (directory treemap with churn heat), Graph (module graph + delta overlay), Deps "
       "(manifest table + transitive tree), Queue (ranked attention worklist), Ambient (glanceable "
       "single pane), Search (fuzzy overlay), Artefact (export preview).\n\nFix the global keymap "
       "conventions (j/k list motion, enter drill, Esc up, d diff, s structure, g graph, p deps, q queue, "
       "/ search, , command palette, a ambient, x mark reviewed, ctrl-r reindex, ? help) and the rules: "
       "everything reachable from the keyboard, every view usable at 80x24, mouse is additive only.",
  ac=bullet("docs/adr/0006-tui.md with one ASCII layout per view plus focus order",
            "Keymap table, with the chords reserved for user override",
            "Documented degradation order when the terminal shrinks below each view's minimum size",
            "Two reviewers can name the exact key sequence for the three most common tasks"),
  labels=["adr", "ux"])

t("e0.7", "e0", "Scaffold cargo workspace, CI matrix and release skeleton", "chore", 1, 480,
  deps=["e0.2"],
  desc="Create the workspace exactly as ADR-002 defines it, with compiling stub crates, shared lint config "
       "(rustfmt, clippy -D warnings, cargo-deny for licences/advisories), and CI that builds and tests on "
       "ubuntu/macos/windows for every PR. Add a cargo-dist skeleton so each PR produces a downloadable "
       "binary, and a top-level README with a five-line quickstart.",
  ac=bullet("clean clone: cargo build --workspace && cargo test --workspace succeeds",
            "CI green on linux, macos and windows; fmt and clippy are hard failures",
            "cargo dist artifact uploaded per PR and downloadable without auth",
            "fathom --version and fathom --help work from the built binary"),
  labels=["infra", "mvp"])

t("e0.8", "e0", "Config system: fathom.toml discovery, schema and explanation", "feature", 1, 300,
  deps=["e0.2"],
  desc="Layered configuration with precedence flags > env > repo fathom.toml > user config > system "
       "defaults, deserialized into one typed root config with a JSON Schema checked in for editor tooling. "
       "Sections: paths and noise globs, ecosystem enable/disable, layer definitions, risk weights, keymap "
       "overrides, cache and memory caps, network consent.\n\nShip `fathom config explain [--path P]` that "
       "prints which layer won for each effective value, because every later feature's behaviour must be "
       "traceable to a file.",
  ac=bullet("Precedence unit tests for all five layers",
            "Unknown keys warn with a nearest-name suggestion instead of failing silently",
            "fathom config explain --path src/foo.rs prints the resolved decision chain",
            "schemas/fathom.schema.json generated and validated in CI"),
  labels=["config", "mvp"])

t("e0.9", "e0", "Path taxonomy and noise policy engine", "feature", 1, 300,
  deps=["e0.8"],
  desc="A single place that decides what a path IS (source, test, docs, config, schema, migration, "
       "generated, vendored, lockfile, asset, binary) and how noisy it is, so classifiers and detectors "
       "never encode their own glob lists. Built-in knowledge for common generated-file conventions "
       "(*.pb.go, *_generated.rs, dist/, *.snap, storybook, openapi codegen, vendor/, node_modules) plus "
       "user globs and negations from fathom.toml.\n\nEvery decision is explainable: `fathom explain <path>` "
       "prints which rule matched and which rule it beat.",
  ac=bullet("Decision table test covering at least 150 real-world paths across 20 well-known repo shapes",
            "fathom explain <path> names the matching rule, its source layer and the runners-up",
            "User globs can both promote and demote a path relative to built-ins",
            "Lookup is O(log n)-ish and cheap enough to call per changed file (no regex-per-file blowup)"),
  labels=["mvp", "analysis"])

t("e0.10", "e0", "Deterministic fixture generator and golden scenario corpus", "task", 1, 400,
  deps=["e0.7"],
  desc="Test infrastructure that unblocks almost every later ticket: fathom-fixtures builds synthetic git "
       "repositories with fixed timestamps, fixed committers (including agent trailers and harness event "
       "logs) and byte-identical commit SHAs, driven by named scenarios: rename_storm, dependency_bump_storm, "
       "cycle_introduced, test_deletion, reformat_only, scope_creep, rebase_rewrite, churn_loop, huge_file, "
       "unicode_paths, many_merges, ten_k_commits, submodule, shallow_clone.\n\nEach scenario ships a "
       "ground-truth file (expected episodes, expected findings) so analysis tickets assert on reality "
       "instead of on ad-hoc strings.",
  ac=bullet("cargo run -p fathom-fixtures -- --scenario cycle_introduced /tmp/repo reproduces identical SHAs on all three OSes",
            "Every scenario has a ground-truth JSON with expected episode boundaries and expected findings",
            "Corpus runs in CI in under 2 minutes and is used by at least e1.3, e2.2 and e4.3",
            "Adding a scenario is one function plus one ground-truth file (documented)"),
  labels=["test-infra", "mvp"])

t("e0.11", "e0", "Performance budget harness and CI regression gate", "task", 2, 240,
  deps=["e0.7", "e0.10"],
  desc="Encode the budgets from ADR-004 as criterion benches plus a scripted end-to-end index benchmark over "
       "the fixture corpus, asserted in CI. Track cold index time, warm single-commit re-index, diff-viewer "
       "open time, query p95 and peak RSS. Fail the build on more than a 15 percent regression and publish "
       "the trend as a job artifact so growth is visible before it is accidental.",
  ac=bullet("benchmarks/BUDGETS.md lists every budget with its numeric threshold",
            "A deliberate regression (sleep injected) turns CI red",
            "make bench reproduces the numbers locally in under 5 minutes",
            "RSS measured, not just wall time"),
  labels=["perf", "infra"])

t("e0.12", "e0", "Sequencing checkpoint: reconcile the plan against spike findings", "chore", 1, 180,
  deps=["s1", "s2", "s3", "s4"],
  desc="The plan is a bet on four empirical unknowns: parser accuracy, segmentation accuracy, the cost of "
       "graph-per-revision and rename detection across merges. After the spikes land, re-read the whole graph "
       "and edit it: close tickets the spikes invalidated, split anything now obviously larger than two days, "
       "re-prioritise Epic 3 if graph-per-revision is too expensive, and confirm the MVP cut line (label mvp).",
  ac=bullet("Each spike's recommendation is reflected by an actual change to a ticket (or a comment saying why not)",
            "No ticket left with an estimate above two days; nothing depends on an invalidated ticket",
            "bd dep cycles reports no cycles and bd ready returns a non-empty, honest first sprint",
            "MVP label set re-confirmed as the smallest shippable spine"),
  labels=["planning"])

# ================================================================= EPIC 1
t("e1.1", "e1", "Repository discovery and git object access via gix", "feature", 1, 480,
  deps=["e0.7"],
  desc="Find and read repositories without shelling out to git: discovery from cwd upward, worktrees, bare "
       "repos, submodules, detached HEAD, shallow and partial clones (treat promisor-missing objects as a "
       "first-class degraded state, not a panic). Commit graph walking with first-parent and include/exclude "
       "options, ref/HEAD tracking, commit-grafts-safe iteration, and a thin abstraction over gix so the "
       "CLI can fall back to the git binary for the rare exotic case.\n\nAlso read the working tree state "
       "(staged/unstaged) so later tickets can show uncommitted work.",
  ac=bullet("Fixture coverage: worktree, bare, submodule, shallow clone, detached HEAD, unicode paths",
            "Non-repo and shallow-clone cases return typed, user-facing errors with a suggested remedy",
            "Log walk over a 100k-commit repo under 2s cold and no allocation blowup (bench recorded)",
            "No `git` subprocess in the default code path (asserted by a test spying on process spawns)"),
  labels=["git", "mvp"])

t("e1.2", "e1", "Commit ingestion and normalization pipeline", "feature", 1, 480,
  deps=["e1.1", "e0.4"],
  desc="Turn git commits into a stable normalized record: SHA, parents, author/committer and offsets, "
       "message, trailers, per-file change records (old/new path, change type, mode, binary flag, hunk "
       "headers, added/removed line counts, raw rename candidate) plus size caps so a 500MB file does not "
       "enter memory. Parallelize per-commit work with rayon, keep ingestion idempotent and resumable, and "
       "define the canonical JSON export used by snapshot tests.",
  ac=bullet("A 500-commit fixture ingests into the store and round-trips to the golden JSON export byte-for-byte",
            "Commits with huge/binary/renamed files ingest without exceeding a documented memory cap",
            "Re-running ingestion is a no-op (idempotent) and interruption resumes from the last durable commit",
            "Ingestion throughput recorded in the bench suite"),
  labels=["git", "mvp"])

t("e1.3", "e1", "Episode segmentation: commits into reviewable episodes", "feature", 1, 600,
  deps=["e1.2", "e0.3", "s2"],
  desc="Implement the segmentation rules from ADR-003: boundary sources in priority order are harness turn "
       "events, explicit trailers, then heuristics (author/identity change, time gap, branch boundary, "
       "message shape). Group contiguous commits of one agent session into one episode, keep interleaved "
       "human commits as their own episodes, and expose the computed boundary reason per episode so a wrong "
       "guess is debuggable and overridable from fathom.toml.\n\nEpisode identity must follow ADR-003 so "
       "rewrites do not orphan later review state.",
  ac=bullet("Segmentation report on the fixture corpus hits at least 90 percent boundary precision and recall against ground truth",
            "Every episode records which rule produced its boundary and the confidence of that call",
            "Boundaries are stable across rebase/amend of the same logical change",
            "fathom.toml can force a split or a merge, and the override is visible in the episode card"),
  labels=["episodes", "mvp"])

t("e1.4", "e1", "Incremental indexing with a tree-hash analysis cache", "feature", 1, 480,
  deps=["e1.2", "e0.4"],
  desc="Make re-indexing feel free: compute the changed path set between commits, invalidate only analyses "
       "whose inputs changed (cache key = tree/blob hash + analyzer version per ADR-004), write through the "
       "single-writer store, and support the dirty working tree as a provisional episode so the user sees "
       "their own uncommitted edits. Cancellation must be instant (user pressed ctrl-r again).",
  ac=bullet("Warm re-index after one new commit is under 150ms on the 500-commit fixture (bench, all OSes)",
            "Editing one file invalidates only analyses that consume that file (asserted by counters)",
            "kill -9 during indexing leaves a usable store; next run recovers via WAL and completes (test)",
            "fathom index --stats prints hit/miss counts per analyzer for cache debugging"),
  labels=["perf", "mvp"])

t("e1.5", "e1", "TUI application shell: event loop, focus model, keymap, help", "feature", 1, 600,
  deps=["e0.6", "e0.7"],
  desc="The ratatui + crossterm shell every view plugs into: tickless event loop with input and worker "
       "messages on one queue, frame coalescing to stay under the 16ms budget, a modal/pane stack with an "
       "explicit focus model, a keymap engine supporting chords and user overrides from fathom.toml, a "
       "global help overlay generated from the keymap, toast/status line, resize handling, a panic hook that "
       "always restores the terminal, plus --no-alt-screen for inline use and --plain for pipes.",
  ac=bullet("Interactive smoke test passes at 80x24, 100x30 and 200x60 with no layout panic",
            "Ctrl-C or a panic in a worker never leaves the terminal in alt-screen (tested)",
            "Keymap override file changes behaviour without recompiling; help overlay reflects it",
            "Frame p99 under 33ms while holding a key down with an empty model"),
  labels=["tui", "mvp"])

t("e1.6", "e1", "Timeline view: episode rail with filters and navigation", "feature", 1, 480,
  deps=["e1.5", "e1.3"],
  desc="The home screen. A virtualized episode list (one row per episode: relative time, author, "
       "agent/human badge, one-line summary, change-size sparkline, unread marker), grouped by day or by "
       "agent session, with composable filters (author, agent vs human, date range, path/area, reviewed "
       "state), quick jumps (top, bottom, first-unread, today), and a details pane that previews the "
       "selected episode. Scrolling must never block on the store.",
  ac=bullet("10k-episode fixture scrolls with p99 frame time under 16ms and constant memory",
            "Filters compose (author AND path AND unread) and are persisted in session state",
            "Selection survives re-index and re-attaches to the same episode after rebase",
            "All navigation is possible without a mouse and matches the ADR-006 keymap"),
  labels=["tui", "mvp"])

t("e1.7", "e1", "Diff viewer: file tree plus unified/split diff with highlighting", "feature", 1, 600,
  deps=["e1.5", "e1.2"],
  desc="The screen people actually live in. Left: file tree grouped by taxonomy and directory with "
       "per-file +/- counts and read/dirty markers; right: hunk-aware diff with syntax highlighting, "
       "word-diff toggle, whitespace-ignore toggle, and jump between hunks. Handle degenerate cases "
       "deliberately: binary files, mode-only changes, deleted files, files over a size cap, CRLF churn, "
       "and files where the diff is not available offline. Support copying a hunk, copying a path and "
       "opening the file at a line in $EDITOR.",
  ac=bullet("Rendering a 50k-line diff file stays under a documented memory cap via chunked reads",
            "Golden UI tests for binary, mode-only, rename-with-edits, deleted and oversize files",
            "Hunk navigation, word-diff and whitespace-toggle all behave on the fixture corpus",
            "Copy path / copy hunk / open in editor work on all three OSes"),
  labels=["tui", "mvp"])

t("e1.8", "e1", "Lazy diff materialization and list virtualization", "task", 1, 300,
  deps=["e1.7"],
  desc="Move diff decoding off the UI thread: a background worker materializes hunks on demand with "
       "priority to the visible window, caches decoded blobs in a bounded cache, cancels requests the user "
       "scrolled past, and pre-warms the next/previous episode. Apply the same pattern to long file trees "
       "and the timeline.",
  ac=bullet("Opening an episode that touched 5000 files paints in under 300ms",
            "Bounded decode cache enforced by a unit test that would otherwise exceed the cap",
            "Rapid scrolling does not queue unbounded work (in-flight cap asserted in a test)",
            "Rapid next/next/next shows progressively correct content, never stale content"),
  labels=["perf", "tui", "mvp"])

t("e1.9", "e1", "Watch mode v1: live detection of new commits and refs", "feature", 1, 400,
  deps=["e1.4"],
  desc="Watch .git for changes with file notifications plus debounced polling fallback (NFS, containers, "
       "bind mounts), and drive a visible state machine: idle, detecting, indexing (with progress), error, "
       "stale. Guard against pathological cases - rebase in progress, ref spam from a background process, "
       "repo switched under the TUI (inode change), git gc storms. Budget the watcher's CPU so it is invisible "
       "next to a running agent.",
  ac=bullet("A commit made in another terminal appears in the timeline within 1s (fixture test with real fs events)",
            "Idle CPU under 1 percent while watching a quiet repo for 10 minutes",
            "Interactive rebase, gc and repo replacement are all handled without a crash or a stuck spinner",
            "Polling fallback kicks in automatically and is reported in fathom doctor"),
  labels=["mvp", "watch"])

t("e1.10", "e1", "CLI verbs: index, show, stats, doctor and export skeleton", "task", 1, 300,
  deps=["e1.2", "e1.4"],
  desc="The headless surface, used by scripts, CI and bug reports: fathom index [--rebuild|--watch], "
       "fathom show <episode|rev> [--json], fathom stats (repo scale, index state, cache hit rate), "
       "fathom doctor (environment and repo diagnostics with remedies) and a stub export verb that e5.7 "
       "fills in. Every verb supports --json with a checked-in schema and documented exit codes.",
  ac=bullet("Each verb has a JSON schema under schemas/ and a round-trip test",
            "Documented exit codes (0 ok, 1 user error, 2 environment, 3 degraded index) asserted in tests",
            "fathom doctor output is attachable to a bug report and contains no secrets or absolute home paths",
            "--help text matches ADR-006 naming and is checked by a snapshot test"),
  labels=["cli", "mvp"])

t("e1.11", "e1", "Session state persistence and resume", "task", 2, 240,
  deps=["e1.6"],
  desc="Persist per-repo UI state - selected episode, per-view filters, layout and pane sizes, read/unread "
       "set, pinned episodes, last search - in the store (not the working tree), with concurrent-TUI safety "
       "and a --fresh escape hatch. Corrupt or mismatched state must self-heal rather than refuse to start.",
  ac=bullet("Reopening the TUI restores selection, filters and layout exactly",
            "Two simultaneous TUI sessions do not clobber each other's state (test)",
            "Truncated/corrupt state file is quarantined and the app starts clean with a warning",
            "fathom --fresh starts from defaults and does not write on exit unless something changed"),
  labels=["tui", "state"])

t("e1.12", "e1", "MVP acceptance: review a 500-commit agent repo entirely in the TUI", "task", 1, 240,
  deps=["e1.6", "e1.7", "e1.8", "e1.9", "e1.10", "e1.11"],
  desc="A scripted end-to-end validation of the spine, not a code task: take the 500-commit agent-driven "
       "fixture, and have someone who did not write the code (a) orient in under a minute, (b) find and read "
       "the three riskiest episodes, (c) do it without touching the mouse or leaving the TUI, (d) do a rebase "
       "midway and confirm nothing is lost. File a defect ticket for every friction point found; the epic is "
       "not done while P1 defects are open.",
  ac=bullet("Recorded walkthrough (asciinema cast) committed under docs/demo/",
            "Time-to-first-orientation under 60 seconds for a newcomer",
            "All discovered defects filed as beads and triaged before Epic 1 closes",
            "Perf numbers from the session recorded against ADR-004 budgets"),
  labels=["acceptance", "mvp"])

# ================================================================= EPIC 2
t("e2.1", "e2", "Change classifier: per-episode file taxonomy", "feature", 1, 360,
  deps=["e0.9", "e1.2"],
  desc="Classify every changed path in an episode into one taxonomy (source, test, docs, config, schema, "
       "migration, generated, lockfile, vendored, asset, binary, other) by combining path rules from the "
       "e0.9 policy engine with content evidence (file header markers, extension, parsed content, whether "
       "the path appears in a build config). Emit a confidence score and the reason for every decision, "
       "because everything downstream trusts this label.",
  ac=bullet("Labelled fixture set classified with at least 95 percent accuracy (report in CI)",
            "Each classification carries confidence plus a human-readable reason reachable from the UI",
            "Ambiguous paths land in 'other' rather than being guessed into 'source'",
            "Classification is cached per blob and stable across runs"),
  labels=["analysis", "mvp"])

t("e2.2", "e2", "Noise suppression: reformat, generated and lockfile churn detection", "feature", 1, 480,
  deps=["e2.1"],
  desc="Give the reviewer their attention back. Detect whitespace/formatting-only changes with an "
       "AST-normalized or token-normalized comparison (per language) rather than a whitespace flag; detect "
       "generated-file churn and lockfile-only edits; collapse an episode that is 'nothing but noise' to a "
       "single line with evidence. Critical rule: suppression is a view, never a deletion - the raw content "
       "is always one keypress away and the policy that hid it is printed inline.",
  ac=bullet("A rustfmt/prettier-only commit renders as '0 semantic changes' with the evidence for that claim",
            "A lockfile-only episode collapses with a one-line reason and an expand affordance",
            "No content is hidden without a visible reason (asserted by a UI golden test)",
            "Suppression policy is overridable per repo and per session with one keystroke"),
  labels=["analysis", "ux", "mvp"])

t("e2.3", "e2", "Folder structure delta and directory-level summary", "feature", 1, 480,
  deps=["e2.1", "e2.4"],
  desc="Answer 'where did this land' without reading file lists. Per directory: added/removed/moved lines, "
       "files created/deleted/moved, new and removed directories, subtree moves (a whole package relocated), "
       "and structural metrics (depth, fanout, files per directory, share of code in the top-3 directories) "
       "plus a structure-churn score that makes 'the agent reorganized the tree' obvious. Produce a "
       "one-sentence deterministic summary per episode backed by an evidence list.",
  ac=bullet("Fixtures for module split, directory flattening, package rename and subtree move each produce the correct one-sentence summary",
            "Directory table sums to the episode totals exactly (property test)",
            "Structure-churn score distinguishes a 20-file local change from a 20-file cross-tree reshuffle",
            "Summary is deterministic: identical input yields identical bytes"),
  labels=["analysis", "structure", "mvp"])

t("e2.4", "e2", "Rename and copy detection with pairing confidence", "task", 1, 360,
  deps=["e1.2", "s4"],
  desc="Git's rename detection is a good floor and a bad ceiling: it misses renames-with-rewrites, splits, "
       "merges and anything across a merge commit. Layer a content-similarity matcher (shingle/minhash style) "
       "on top of gix rename info to produce rename, split, merge and copy candidates with explicit "
       "confidence, and treat low-confidence pairs as guesses in the UI, never as facts.",
  ac=bullet("At least 90 percent pair precision and recall on the rename_storm and rename-with-rewrite fixtures",
            "Splits and merges of one file into several are represented (not forced into 1:1)",
            "Low-confidence candidates are visibly marked as guesses and can be accepted or rejected",
            "Cost is bounded: O(changed files) not O(repo files) for typical episodes (bench)"),
  labels=["analysis", "structure", "mvp"])

t("e2.5", "e2", "Dependency manifest and lockfile parsers across ecosystems", "feature", 1, 600,
  deps=["e1.2"],
  desc="Parse the dependency surface of a repository for a given revision and for a diff. Manifests: "
       "package.json, Cargo.toml, pyproject.toml, go.mod, Gemfile, mix.exs, build files (maven/gradle, "
       "best-effort read-only), *.csproj. Lockfiles: package-lock.json, yarn.lock, pnpm-lock.yaml, "
       "Cargo.lock, poetry.lock, uv.lock, requirements/locks, go.sum, Gemfile.lock, mix.lock. Parsers are "
       "tolerant: on malformed input they degrade to 'unparsed - here is the raw diff' instead of dropping "
       "the episode. Ecosystems are individually feature-gated so a build can slim down.",
  ac=bullet("Golden fixtures per ecosystem and per lockfile format (10+ formats) with exact expected structures",
            "Malformed or truncated lockfiles produce a typed 'unparsed' result and never an error that hides the episode",
            "Parsers handle workspace/monorepo layouts with many manifests in one repo",
            "Throughput: full-repo dependency scan of a monorepo fixture under 500ms (bench)"),
  labels=["deps", "analysis", "mvp"])

t("e2.6", "e2", "Dependency delta model: direct vs transitive, added, removed, upgraded", "feature", 1, 480,
  deps=["e2.5"],
  desc="Compute dependency STATE before and after an episode and diff the resolved graph, not the lockfile "
       "text. Report: new direct dependencies, removed, version upgrades/downgrades, transitive-only changes, "
       "resolution-graph size delta, new registries or git/vendored sources, integrity/hash changes, "
       "peer-dependency conflicts, and the license set delta. Summarise in human terms, e.g. 'net +140 "
       "packages from 1 new direct dependency (foo 2.1.0)'.",
  ac=bullet("Correct direct/transitive attribution for fixtures in at least six ecosystems",
            "'dependency_bump_storm' fixture distinguishes a mass transitive refresh from one new direct dep",
            "Version downgrades, source switches (registry to git) and integrity changes are each detected",
            "Dependency table renders per episode with drill-down to the lockfile hunk that proves it"),
  labels=["deps", "analysis", "mvp"])

t("e2.7", "e2", "Dependency risk signals: provenance, advisories and licence drift", "feature", 2, 400,
  deps=["e2.6", "e0.8"],
  desc="Answer the question a reviewer actually has: 'who asked for this, and should I be worried?' Link "
       "every new direct dependency to the episode and captured intent that introduced it (e4.2 when "
       "available). Add optional advisory lookup (OSV) with an explicit network consent flag, a local cache "
       "and a stale-cache-friendly offline mode, plus licence-policy checks from fathom.toml (deny copyleft in "
       "the library layer, etc).",
  ac=bullet("Default build and default config make zero network calls (test asserts no socket use)",
            "New-dependency card answers 'introduced by episode X, intent: Y' when the data exists and says so when it does not",
            "Advisory lookup works offline from cache, reports freshness and never blocks episode rendering",
            "Licence policy violations appear as findings with the offending edge path"),
  labels=["deps", "risk"])

t("e2.8", "e2", "Non-code surface changes: config, env, flags, migrations and CI", "feature", 1, 360,
  deps=["e2.1"],
  desc="The changes that break production are rarely in source files. Detect and summarise: environment "
       "variables added/removed (cross-checked against code references so a renamed var with a stale "
       "reference is flagged), feature flags added/removed with default value, database migrations "
       "(forward-only vs reversible, missing down migration, migration inside the same episode as model "
       "changes), CI workflow and permission changes (new secrets, new runners, changed OIDC, push-to-main "
       "triggers), infrastructure-as-code deltas, and Dockerfile/base-image changes. Emit an operator-action "
       "checklist per episode.",
  ac=bullet("Fixture per surface (env, flag, migration, CI workflow, IaC, Dockerfile) produces the expected finding",
            "Env var added in .env.example but never read in code is flagged, and vice versa (removed but referenced)",
            "Migration-only episodes surface an explicit 'needs deploy coordination' warning",
            "Findings roll up into a per-episode 'requires operator action' checklist"),
  labels=["config", "risk", "mvp"])

t("e2.9", "e2", "Test surface delta and coverage proxy", "feature", 1, 400,
  deps=["e2.1"],
  desc="Track the testing contract, not coverage percentages. Per episode: test lines added vs production "
       "lines added (ratio and absolute), test files created/renamed/deleted, individual test cases added or "
       "removed (parse test function names per language), skipped or marked-expected-failure tests, weakened "
       "assertions (assert removed, or equality replaced by existence check) and production code added with "
       "no corresponding test surface in the same episode.",
  ac=bullet("Per-episode card shows test-vs-prod line ratio, tests added/removed/skipped for rust, python, ts and go fixtures",
            "Deleting or skipping an existing test is a first-class finding with the test name and location",
            "'New public logic, no new test' finding has a documented and tunable definition rather than a guess",
            "Cases with no test framework present degrade to a line-count proxy with a stated confidence"),
  labels=["testing", "risk", "mvp"])

t("e2.10", "e2", "Episode summary card: what, where, why and blast radius", "feature", 1, 480,
  deps=["e2.2", "e2.3", "e2.6", "e2.8", "e2.9"],
  desc="Compose the evidence-backed one-screen answer that the rest of the product hangs on: what changed "
       "(semantic counts after noise suppression), where it landed (directories, structure summary), what "
       "it now depends on (dependency delta headline), what surface it touches (config/migration/CI flags), "
       "what is tested, and a blast-radius estimate (direct + one-hop impact using the module graph when "
       "available, path heuristics when not). Every number is a button into its evidence.",
  ac=bullet("Card renders in under 50ms with a warm index (bench) and is byte-deterministic for a given episode",
            "Every figure on the card links to the underlying evidence view (test walks each link)",
            "Card is complete and correct with all optional analyses unavailable (graceful degradation)",
            "Golden tests over the fixture corpus catch regressions in wording and ordering"),
  labels=["ux", "analysis", "mvp"])

t("e2.11", "e2", "Insight API: one versioned delta model consumed by TUI, CLI and export", "task", 1, 360,
  deps=["e2.10"],
  desc="Make the analysis model the single source of truth so the TUI, the CLI, the export artefact and the "
       "MCP server never compute their own version of reality. Define a typed, versioned EpisodeDelta "
       "structure, publish a JSON Schema, expose it via fathom analyze <episode> --json, and refactor the "
       "TUI card to render purely from it. Include a compatibility policy: additive changes allowed, breaking "
       "changes bump the version and are caught in CI.",
  ac=bullet("schemas/episode-delta.schema.json checked in and validated against every fixture",
            "The TUI contains no analysis logic (verified by review plus a crate-boundary lint)",
            "fathom analyze --json is the input to e5.7's exporter and to e6.4's MCP tools",
            "A breaking schema change without a version bump fails CI"),
  labels=["architecture", "api", "mvp"])

# ================================================================= EPIC 3
t("e3.1", "e3", "Multi-language structural extractor via tree-sitter", "feature", 2, 900,
  deps=["e1.2", "s1"],
  desc="Extract a per-file structural model: definitions (functions, classes, types, interfaces, constants), "
       "their span and complexity proxies, exported/public visibility, imports and their resolved targets, "
       "implemented interfaces/traits and inheritance. Ship grammars for Rust, Python, TypeScript/TSX, Go, "
       "Ruby and Java, chosen from the s1 spike report, plus a regex/ctags-style fallback for everything "
       "else that is honest about being approximate. Per-file parse errors are collected and surfaced, never "
       "swallowed. Deterministic output ordering is required so snapshots are diffable.",
  ac=bullet("Extraction accuracy at or above the thresholds agreed in s1, per language, on the fixture corpus",
            "Unknown or broken language yields 'structure unavailable' plus file-level analysis, never a lost episode",
            "Throughput at least 1000 files per second per core on the corpus (bench)",
            "Identical input produces byte-identical output (snapshot test)"),
  labels=["architecture", "parsers"])

t("e3.2", "e3", "Per-revision module and symbol graph builder with snapshot store", "feature", 2, 600,
  deps=["e3.1", "e0.4", "s3"],
  desc="Build the graph that makes architectural questions answerable: nodes are modules and exported "
       "symbols, edges are imports, uses, implementations and inheritance, with layer tags from config. "
       "STORED AS `s3` MEASURED (ADR-004 §13.7/§14.1): one graph DELTA per revision in `graph_delta` plus a "
       "shared content-addressed anchor every `graph.chain_full_every = 32` revisions, capped by "
       "`graph.query.max_chain_walk = 32`, derived from the blob-keyed per-file structural substrate of "
       "e1.4/e3.1 — measured 22 335 B/revision against a 32 KiB budget. NOT a full snapshot per tree hash: "
       "measured 16.7 MB/revision unshared and 1.09 MB/revision with content-defined chunking, because the "
       "per-revision membership list is the cost. Materialise one revision on selection (budget: "
       "`graph.materialise_revision.ms`), apply a delta when the cursor moves one commit, and expose a query "
       "API: dependencies of X, dependents of X, path between A and B, subgraph of a layer, changed edges "
       "between two snapshots — with node ids interned, which `s3` left as the ~3x byte reduction it projected "
       "but did not bank. See docs/concepts/graph-storage.md.",
  ac=bullet("1000-commit fixture: cold full build under 60s, marginal cost per new commit under 50ms (bench)",
            "Per-revision graph storage under store.bytes_per_commit_graph (32 KiB) on the s3 workload, "
            "and the materialisation p95 under graph.materialise_revision.ms (both measured and reported)",
            "Subgraph and path queries answer p95 under 20ms at 5000 nodes",
            "Graph for a revision is reproducible: rebuilding from an empty cache yields identical results",
            "A revision whose anchor is missing answers as a stated partial, never as a smaller graph"),
  labels=["architecture", "graph"])

t("e3.3", "e3", "Graph delta between adjacent revisions and between arbitrary episodes", "feature", 2, 500,
  deps=["e3.2"],
  desc="Compute the architectural diff: nodes and edges added/removed/changed, symbols that moved from one "
       "module to another (distinguished from delete+add), files whose public surface changed, and an "
       "edge-churn metric per module that flags 'this module keeps getting reworked'. Must work for adjacent "
       "episodes (the common case, optimised) and for arbitrary pairs (e5.3). Deterministic ordering and a "
       "JSON form usable by the exporter and MCP. For adjacent revisions the answer is a SELECT over the delta "
       "rows `s3`'s policy stores (measured 0.26 ms p95, `query.graph_delta_adjacent.ms`); any representation "
       "without stored deltas pays two full materialisations for the same question, which is one of the three "
       "reasons that policy was chosen.",
  ac=bullet("Fixtures for moved symbol, split module, dependency inversion and dead-edge removal produce the expected delta",
            "Symbol move is reported as a move, not as a deletion plus an addition",
            "Delta between two non-adjacent episodes matches the composition of the intermediate deltas (property test)",
            "Empty delta is detected in constant time so 'no architectural change' is cheap to state"),
  labels=["architecture", "graph"])

t("e3.4", "e3", "TUI architecture map with per-episode delta overlay", "feature", 2, 700,
  deps=["e3.3", "e0.6"],
  desc="Two coordinated views sharing one graph model. Structure map: hierarchical treemap of the tree with "
       "heat mapped to churn or complexity, keyboard drill-in/out and aggregation levels so 3k nodes remain "
       "readable. Dependency map: module graph (layered or circular layout) with filtering by layer/module "
       "and highlighting of the selected module's neighbourhood. On top of both, a per-episode delta overlay: "
       "added edges solid green, removed edges ghosted red, changed nodes outlined, with hop-limit and "
       "'show only what this episode touched' filtering, plus counts when a delta exceeds what can be drawn.",
  ac=bullet("Rendering 3000 nodes stays under 100ms by aggregating beyond a zoom threshold (bench)",
            "Delta overlay is legible at 80x24 and degrades by aggregation, not by clipping",
            "Fully keyboard operable and consistent with the ADR-006 keymap; mouse is additive",
            "Selecting any edge reveals the file-and-line evidence that produced it"),
  labels=["architecture", "tui", "ux"])

t("e3.5", "e3", "Cycle detection and cycle delta", "feature", 2, 400,
  deps=["e3.2"],
  desc="Incremental strongly-connected-component maintenance rather than a full recompute per episode. "
       "Detect cycles introduced or removed by an episode, display the shortest offender path (module "
       "level, with an opt-in symbol-level view), and keep a cycle-debt ledger: existing cycles baselined "
       "with owner and expiry so only new cycles alarm. Report the count of nodes inside cycles as a "
       "trendable health metric.",
  ac=bullet("New-cycle and resolved-cycle fixtures detected with the exact minimal cycle path",
            "Cycle detection after an incremental update runs under 100ms on a 5000-node graph (bench)",
            "Baselined cycles are shown as debt with their expiry and never appear as a new violation",
            "Cycle count and nodes-in-cycles exported for the scorecard (e3.10)"),
  labels=["architecture", "graph"])

t("e3.6", "e3", "Layering rules engine with per-episode violation delta", "feature", 2, 500,
  deps=["e3.2", "e0.8"],
  desc="Let a team declare its intended architecture declaratively in fathom.toml: named layers with glob "
       "patterns, allowed and forbidden dependencies, 'no cycles' and 'no upward references' rules, "
       "no-cycle-within-layer options, and boundary rules (only the api crate may import the db layer). "
       "Evaluate violations and, crucially, the DELTA of violations per episode so the reviewer sees only "
       "what this turn did. Each violation explains why it is wrong and suggests the usual fix.",
  ac=bullet("A configured example repo yields zero false positives and catches every seeded violation in the fixture",
            "Per-episode delta separates new violations from pre-existing ones (baseline-aware)",
            "Each violation names the rule, the offending edge with file:line and a fix hint",
            "Rules are validated at config load with actionable errors, not silent no-ops"),
  labels=["architecture", "policy"])

t("e3.7", "e3", "Public API surface extraction and breaking-change diff", "feature", 2, 700,
  deps=["e3.1"],
  desc="Track the contracts the rest of the world compiles against. Per-language surface extraction: Rust "
       "pub items (via cargo metadata/rustdoc JSON where available, tree-sitter fallback), Python __all__ "
       "and stubs, TypeScript .d.ts and export barrels, Go exported identifiers, Java public types. Diff the "
       "surface between revisions and classify each change as breaking, additive or deprecating (signature "
       "and visibility changes, removed exports, changed optionality, widened parameter types) and emit a "
       "semver recommendation for the affected packages.",
  ac=bullet("Breaking/additive fixtures for at least rust, python, typescript and go classify correctly",
            "'No public API change' is detected as a fast path so most episodes cost nothing",
            "Each API change links to the file:line and the episode that made it",
            "Semver recommendation explains which specific change drove it"),
  labels=["architecture", "api"])

t("e3.8", "e3", "Hotspots: god modules, coupling, churn x complexity, dead code candidates", "feature", 2, 500,
  deps=["e3.2"],
  desc="Rank where architectural attention pays off. Per module: fan-in, fan-out, instability, size, "
       "complexity proxy (from e3.1 spans), and churn velocity over a configurable window, combined into an "
       "explainable hotspot score (high churn x high complexity is the classic quadrant). Separately list "
       "dead-code candidates: newly added symbols with no inbound references after N episodes, code behind "
       "flags that are off, and duplicated blocks that grew in the same episode. Every metric shows its "
       "contribution to the score.",
  ac=bullet("Hotspot table on the corpus ranks a seeded god module first and explains why, per metric",
            "Dead-code candidates state how long they had no references and admit uncertainty",
            "Zero data (new repo, no history) produces an empty state rather than a crash or nonsense ranking",
            "Metric weights are configurable in fathom.toml and changes are reflected deterministically"),
  labels=["architecture", "analysis"])

t("e3.9", "e3", "Architecture scorecard and drift trend over time", "feature", 2, 360,
  deps=["e3.5", "e3.6", "e3.7", "e3.8"],
  desc="Roll the architecture signals into something a human can track across a week of agent output: a "
       "five-dimension scorecard (boundaries respected, coupling, acyclicity, API hygiene, hotspot pressure) "
       "with the documented formula, per-episode and per-window scores, a trend line across episodes, and "
       "'since my last review' deltas so catching up after two days is one number. Expose "
       "fathom architecture --json for CI and dashboards.",
  ac=bullet("Scoring formula documented to the point of being reproducible by hand from exported JSON",
            "Trend renders across at least 500 episodes without memory growth",
            "'Since last review' delta is correct when the reviewer has been away across a rebase",
            "fathom architecture --json is consumed by fathom check (e6.6) and the exporter (e5.7)"),
  labels=["architecture", "metrics"])

t("e3.10", "e3", "Ratchet: baselines and expiring technical-debt allowances", "feature", 2, 360,
  deps=["e3.9"],
  desc="Make it possible to enforce architecture rules on a messy existing codebase: a committed baseline "
       "(.fathom/ratchet.toml) that records existing violations with owner and expiry date so only NEW "
       "violations fail CI, auto-tightening when a violation disappears (the ratchet only turns one way), and "
       "explicit warnings as allowances approach expiry. Updating the baseline upward must be a deliberate "
       "act (a flag plus a required reason string that lands in the file and in the review artefact).",
  ac=bullet("CI passes with existing violations baselined and fails on one newly introduced violation",
            "Fixing a baselined violation automatically removes it from the file on the next run",
            "Raising the baseline requires --update-baseline plus a reason, and the diff shows who and why",
            "Expiring allowances surface in the TUI and in the export artefact before they lapse"),
  labels=["architecture", "policy", "ci"])

# ================================================================= EPIC 4
t("e4.1", "e4", "Attribution: agent versus human authorship with confidence", "feature", 2, 360,
  deps=["e1.2", "e0.3"],
  desc="Everything agent-specific in Fathom depends on knowing who wrote a commit. Detect authorship from "
       "trailers (Co-authored-by, Generated-by, Agent, Signed-off-by conventions), committer identity, "
       "harness events and message-shape heuristics, with a configurable registry of known agents in "
       "fathom.toml. Emit author kind plus agent identity plus confidence plus the evidence list. Ambiguous "
       "commits are 'unknown' - never guessed - and the label is user-correctable with the correction learned "
       "for future commits by the same signature.",
  ac=bullet("Fixture set covering at least six real-world trailer conventions classified at 90 percent or better",
            "Ambiguous commits are labelled unknown with the reason shown, not silently bucketed",
            "A user correction persists and applies to subsequent matching commits (test)",
            "Attribution per episode is exposed in the episode card and queryable via the CLI"),
  labels=["agents", "mvp"])

t("e4.2", "e4", "Intent capture: link prompt, plan and session to episodes", "feature", 2, 480,
  deps=["e4.1", "e0.5"],
  desc="Pull the 'why' in from the harness: the turn prompt, the plan items and the session id from hooks v1 "
       "events, with a commit-trailer fallback (Intent:/Task:) for teams without hooks. Store the intent "
       "text, the declared scope as path globs, and the plan item list, so later detectors can compare intent "
       "against execution. Missing intent is a normal state, not an error, and the UI says so explicitly.",
  ac=bullet("Episodes show prompt, session id and plan items when hook events exist (end-to-end fixture)",
            "Without hooks the trailer fallback works and, absent that, the card states 'no recorded intent'",
            "Declared scope is stored as globs and is the input to the scope-creep detector",
            "Intent text is searchable via the global search in e5.2"),
  labels=["agents", "integration"])

t("e4.3", "e4", "Agent pathology detectors", "feature", 1, 900,
  deps=["e4.2", "e2.9", "e2.1"],
  desc="Implement the catalogue of things coding agents characteristically do wrong. Each detector is a pure "
       "function over the delta model returning findings with evidence, confidence and a suppression key, "
       "registered in a table so they can be enabled per repo: churn loop (same region edited across N "
       "episodes without converging), scope creep (files touched outside declared intent), untested new "
       "logic, shotgun edit (one episode touching unrelated clusters), rework loop (revert or hunk-repeat "
       "detection), test weakening (assertions deleted or skipped), copy-paste growth, debug leftovers and "
       "TODO debt, hand-edited generated files, secret-shaped strings, and disabled type-check or lint "
       "directives.",
  ac=bullet("One fixture per detector; each detector fires on its fixture and stays silent on the clean corpus",
            "Every finding has a human-readable reason, a jump target, a confidence and a one-key suppression",
            "False-positive rate on three real OSS repos under the threshold agreed in e0.12",
            "Detector catalogue is documented with a worked example and a template for writing a new one"),
  labels=["agents", "risk", "mvp"])

t("e4.4", "e4", "Explainable per-episode risk score with tunable weights", "feature", 1, 480,
  deps=["e4.3", "e2.6", "e2.8"],
  desc="Turn findings into a ranking the human can trust: a documented feature vector per episode (findings "
       "and their severity, change size and blast radius, dependency and migration exposure, test ratio, "
       "attribution confidence, noise share) combined through transparent, clamped, per-repo-tunable weights "
       "into a 0-100 score with an ordered list of the reasons that produced it. No opaque model in v1 - "
       "if we cannot say why an episode is at 82, it should not be at 82. Include a calibration set so weight "
       "changes can be evaluated rather than felt.",
  ac=bullet("fathom why <episode> prints every feature, its weight and its contribution to the total",
            "Score is stable and reproducible; identical input yields identical output",
            "Weights configurable in fathom.toml with a deterministic, monotonic effect on ranking (property test)",
            "Calibration fixture shows precision/recall of 'top 10 percent by score' before and after retuning"),
  labels=["risk", "mvp"])

t("e4.5", "e4", "Attention queue: the ranked review worklist", "feature", 1, 480,
  deps=["e4.4", "e1.6"],
  desc="The queue is the product's main workflow. Rank unread episodes by risk weighted by staleness and "
       "blast radius, and give triage actions that are single keystrokes: mark reviewed, flag with a note, "
       "dismiss with a reason (from a fixed, learnable vocabulary), snooze until a condition. Show queue "
       "counts by severity, a caught-up state, and a 'what you skipped and why' retrospective so "
       "systematic dismissing is visible rather than invisible.",
  ac=bullet("Clearing a 100-episode queue where most episodes are low risk takes under 5 minutes (timed test)",
            "Every triage action records who, when, why and is written to the review ledger (e4.6)",
            "Queue ordering is explainable and reproducible; ties are broken deterministically",
            "Dismissed-with-reason categories roll up in the retrospective view"),
  labels=["ux", "workflow", "mvp"])

t("e4.6", "e4", "Review ledger: durable sign-offs that survive history rewrites", "feature", 1, 480,
  deps=["e4.5", "e0.3"],
  desc="The review ledger is the durable record that a human accepted a change. Sign-offs are keyed to "
       "episode identity plus a content fingerprint and stored as human-readable, committable markdown under "
       ".fathom/reviews/ with a mirrored DB index, so a review can travel through a PR. Re-linking must "
       "reconnect sign-offs after rebase, amend and squash using the fingerprint and member-commit sets, and "
       "must fail loudly when content actually changed after sign-off (a stale sign-off is a bug, not a "
       "detail). Per-reviewer identity from git config or a configured identity.",
  ac=bullet("After rebase -i, amend and squash fixtures at least 95 percent of sign-offs re-link automatically",
            "Content changed after sign-off is flagged as stale and blocks a clean 'all reviewed' state",
            "Ledger files are diff-friendly and land sensibly in a PR (golden file)",
            "Tampering with a ledger entry is detectable (fingerprint mismatch reported)"),
  labels=["workflow", "audit", "mvp"])

t("e4.7", "e4", "Episode narrative: deterministic explanation of what, why and risk", "feature", 2, 480,
  deps=["e2.10", "e4.3"],
  desc="Generate prose a human can skim: a deterministic, template-composed narrative per episode built from "
       "the delta model and findings, with every claim citing an evidence id (no unattached assertions). "
       "Optionally polish with an LLM behind an explicit opt-in flag, which must preserve citations, mark "
       "itself as generated text, be reproducible-enough for tests via fixtures/mocks, and degrade to the "
       "template narrative when unavailable or offline.",
  ac=bullet("Default path uses no network and is byte-deterministic for identical input (test)",
            "Every claim in the narrative carries a resolvable evidence reference (validated in tests)",
            "Opt-in LLM path is tested with a mocked provider and is clearly watermarked in UI and export",
            "Narrative is the text used by the export artefact (e5.7) so both stay consistent"),
  labels=["ux", "narrative"])

t("e4.8", "e4", "Plan-versus-execution drift: contradictions and unimplemented intent", "feature", 2, 400,
  deps=["e4.2", "e3.3"],
  desc="Compare what the agent said it would do with what it did, across a session rather than a single "
       "turn: plan items never implemented (no corresponding change after N episodes), edits outside the "
       "declared scope, dependencies introduced outside the plan, tests promised and not written, refactors "
       "claimed but not performed (surface unchanged), and plan text that contradicts a previous episode's "
       "sign-off. Emit findings with both sides of the contradiction linked.",
  ac=bullet("Drift fixture produces the expected findings: unimplemented item, out-of-scope edit, unplanned dependency",
            "Each finding links the plan text and the diff/graph evidence that contradicts it",
            "Cross-session drift is bounded and does not re-report an already-dismissed contradiction",
            "Works without hooks using the Intent: trailer fallback, with reduced confidence stated"),
  labels=["agents", "risk"])

# ================================================================= EPIC 5
t("e5.1", "e5", "Ambient mode: glanceable status for the second terminal", "feature", 2, 480,
  deps=["e1.9", "e4.5"],
  desc="The mode Fathom lives in while an agent works: a short (fits 80x12) pane showing the latest episode "
       "and its risk, how many episodes are waiting in the queue, indexing state, and the single most "
       "important reason to look now. One keypress drills into the full view; when caught up it goes quiet "
       "and dim rather than chatty. No animation loops, no colour strobing - it sits next to a terminal where "
       "someone is also reading agent output.",
  ac=bullet("fathom ambient renders correctly at 80x12 and in a narrow tmux pane",
            "Idle CPU under 1 percent with the repo quiet; no redraws without events",
            "New high-risk episode is visible within 1s and is one keypress from the full view",
            "Caught-up state is visually distinct and unambiguous"),
  labels=["ux", "workflow"])

t("e5.2", "e5", "Global fuzzy search across episodes, paths, symbols, dependencies and findings", "feature", 2, 400,
  deps=["e3.1", "e1.6"],
  desc="One search box for the whole repository history. Index episode summaries, commit messages, intent "
       "text, file and directory paths, symbol names, dependency names and finding types; rank with a stable "
       "fuzzy scorer; results show enough context to choose (episode, date, author, why it matched) and jump "
       "straight into the right view at the right selection. Support filters as tokens (author:, since:, "
       "risk>, dep:, path:), plus recent and saved searches.",
  ac=bullet("p95 result latency under 50ms while typing on a 10k-episode corpus (bench)",
            "Scoring is deterministic and stable across runs (golden test)",
            "Filter tokens compose with free text and are documented in the help overlay",
            "Search state persists in session state (e1.11) and results deep-link correctly"),
  labels=["ux", "search"])

t("e5.3", "e5", "Compare mode: any two episodes or revisions side by side", "feature", 2, 400,
  deps=["e2.11", "e3.3"],
  desc="Answer 'what changed between the point I last looked and now' - the question a reviewer actually has "
       "after being away. Compose the full delta view (structure, dependencies, architecture, findings, test "
       "surface) for an arbitrary pair of endpoints, across branches, with a shareable textual spec such as "
       "fathom cmp a1b2..c3d4 and a nameable saved comparison ('since the refactor'). Reuse the standard "
       "delta pipeline - no second implementation.",
  ac=bullet("Comparison of two non-adjacent episodes equals the composed per-episode deltas (property test)",
            "Cross-branch comparison works and labels both endpoints unambiguously",
            "Saved comparisons survive restarts and appear in the command palette",
            "fathom cmp <a>..<b> --json works headlessly for scripting"),
  labels=["ux", "analysis"])

t("e5.4", "e5", "Command palette, configurable keymaps and vim motions", "feature", 2, 360,
  deps=["e1.5", "e5.2"],
  desc="Every action is an addressable command with a stable id, surfaced in a palette (,) with fuzzy "
       "matching and the current keybinding shown next to it. Keymaps are user-overridable per mode from "
       "fathom.toml with conflict detection at load time, and list navigation matches vim conventions "
       "(j/k, ctrl-d/u, H/M/L, gt/gT for tabs) so power users do not relearn per view. A discoverability hint "
       "line at the bottom shows the next likely actions and hides itself once learned.",
  ac=bullet("Every action in the app is reachable from the palette (test enumerates actions vs palette entries)",
            "Conflicting keybindings are reported at startup with the file and line to fix",
            "Motion behaviour is identical across timeline, file tree, queue and search results (shared implementation)",
            "Hint line reflects user keymap overrides and can be disabled"),
  labels=["ux", "tui"])

t("e5.5", "e5", "Responsive layouts, unicode-safe rendering and graceful non-TTY degradation", "feature", 2, 400,
  deps=["e1.5"],
  desc="Make it usable on real setups. A layout engine with per-view minimum sizes and a documented "
       "degradation order (hide legend, collapse detail pane, reduce columns, then switch to compact rows) "
       "from 80x24 up to ultrawide; correct cell-width handling for wide CJK, combining marks, emoji and "
       "bidi in commit messages, paths and diff content (truncation must not split grapheme clusters); mouse "
       "support (click to select, scroll) as an optional extra; and a --plain line mode for pipes, CI logs "
       "and screen readers.",
  ac=bullet("Layout matrix documented and covered by screenshot tests at 80x24, 100x30, 120x40, 160x50, ultrawide and 40x10",
            "Unicode fixture set (CJK, emoji, combining, RTL) renders with correct alignment; proptest finds no panic on random text",
            "fathom --plain produces useful ordered text output with no escape sequences",
            "Works under tmux and ssh with TERM variations; degraded modes are reported, not silent"),
  labels=["tui", "a11y"])

t("e5.6", "e5", "Theming, colour-blind-safe palettes and NO_COLOR support", "chore", 2, 240,
  deps=["e1.5"],
  desc="Colour carries meaning in Fathom (risk, added/removed, layer violation) so it has to be robust: "
       "semantic colour tokens rather than raw colours at call sites, dark and light themes, palettes "
       "audited for deuteranopia and protanopia, a 16-colour ANSI fallback for minimal terminals, truecolour "
       "detection, and NO_COLOR plus a --theme flag. Risk and status must remain legible with colour fully "
       "disabled, which means every colour-coded signal also has a glyph or text label.",
  ac=bullet("Contrast ratio of 4.5:1 or better for all text on both default themes (automated check)",
            "With NO_COLOR=1 all information remains distinguishable via glyphs and labels (screenshot test)",
            "Simulation of deuteranopia and protanopia leaves added/removed/violation states distinguishable",
            "Themes are user-loadable from a documented TOML file"),
  labels=["ux", "a11y"])

t("e5.7", "e5", "Export review artefact: markdown, HTML and JSON for PRs and audits", "feature", 1, 480,
  deps=["e2.11", "e4.6", "e4.7"],
  desc="The deliverable that closes the loop with the rest of the organization: fathom export "
       "--episodes A..B --format md|html|json produces a review record containing the summary card, the "
       "dependency and architecture deltas, findings with evidence, the operator-action checklist, the "
       "sign-off block with reviewer identity and time, and links to commits. Byte-stable output (no "
       "timestamps, absolute paths or randomness that cause spurious diffs), stable ordering, and an HTML "
       "rendering that is self-contained, printable and safe (escaped content, no script injection from "
       "commit messages).",
  ac=bullet("Markdown artefact renders correctly in GitHub PR preview including nested evidence links",
            "Two consecutive exports of the same range are byte-identical (test)",
            "HTML is a single self-contained file, printable, with hostile commit messages escaped (test)",
            "Sign-off block includes reviewer, time, scope and any dismissals with reasons"),
  labels=["export", "workflow", "mvp"])

t("e5.8", "e5", "Editor and shell integration: jump to file, external diff, terminal hyperlinks", "task", 2, 240,
  deps=["e1.7"],
  desc="Fathom is a reader, not an editor - make the exits good. Open file:line in $EDITOR (vim, neovim, "
       "helix, vscode, zed, helix), OSC-8 hyperlinks for paths and commits in supporting terminals, a "
       "difftool handoff (difftastic/meld/kdiff3) for people who want a heavyweight diff, and fathom open "
       "<episode> for use from the shell. Detect missing EDITOR gracefully and say what it decided to use.",
  ac=bullet("Works with vim, neovim, helix and code on all three OSes (matrix test where CI allows, documented otherwise)",
            "OSC-8 links are emitted only when the terminal advertises support",
            "difftool handoff respects config and falls back cleanly when the tool is absent",
            "No editor or no difftool is a normal, well-explained path"),
  labels=["ux", "integration"])

t("e5.9", "e5", "First-run onboarding, empty states and error copy", "task", 2, 240,
  deps=["e1.5", "e0.7"],
  desc="The first five minutes decide whether anyone keeps this running. Ship a guided first index with "
       "progress and a realistic time estimate, per-view empty states that name the actual next action ("
       "'no episodes yet - run fathom index' rather than a shrug), and error copy that always states a "
       "remedy and points at fathom doctor. Include a --demo flag that indexes a bundled miniature repo so "
       "someone can see the product before pointing it at a real codebase.",
  ac=bullet("From a clean machine, fathom --demo shows meaningful content in under a minute with no config",
            "Every view has a specific empty state and every user-facing error names a next action (audited list)",
            "Index progress shows percentage plus a time estimate that converges",
            "Copy reviewed once as a batch so the voice is consistent"),
  labels=["ux", "docs"])

# ================================================================= EPIC 6
t("e6.1", "e6", "Hooks v1 ingestion: JSONL event reader, validation and replay", "feature", 2, 480,
  deps=["e0.5", "e1.4"],
  desc="Implement the read side of the contract in ADR-005: a reader that tails an append-only JSONL file, "
       "tolerates partial lines and interleaved concurrent appends, tracks a durable offset so restarts do "
       "not reprocess, validates against the JSON Schema, quarantines malformed events with diagnostics "
       "instead of stalling the pipeline, and can replay a file from the beginning for rebuilds. Expose "
       "fathom hooks --check to show stream health and recent rejections.",
  ac=bullet("Truncated and mid-write lines are handled without loss on the next read (test)",
            "Malformed events are quarantined and counted; the stream keeps flowing (test)",
            "Replay from offset 0 reproduces identical episode linkage (deterministic)",
            "fathom hooks --check reports lag, offset, event counts by type and rejection reasons"),
  labels=["integration", "agents"])

t("e6.2", "e6", "Reference adapters: pi-style hooks, generic harness, git hook fallback", "task", 2, 360,
  deps=["e6.1"],
  desc="Prove the contract against reality with shipped adapters: (1) a pi-style hook adapter translating "
       "session/turn/tool events into hooks v1, (2) an adapter for a second mainstream harness, and (3) a "
       "dependency-free post-commit git hook that captures what it can when there is no harness at all. Each "
       "adapter is a small documented program plus a copy-pasteable recipe, and each must be verified "
       "end-to-end in a live agent session rather than only in unit tests.",
  ac=bullet("Two harness adapters verified end-to-end: intent and episode boundaries appear in the TUI",
            "Git hook fallback is a single file, works on POSIX shells and git bash, and never blocks a commit on failure",
            "Recipes in docs/harness/ are followed by someone other than their author without help",
            "Adapter contract test suite runs against a recorded event trace"),
  labels=["integration", "agents"])

t("e6.3", "e6", "fathom watch daemon: single instance, IPC and budgeted background indexing", "feature", 2, 480,
  deps=["e1.9", "e6.1"],
  desc="One process per repository does the watching, hook tailing and indexing; TUIs and CLI commands attach "
       "over a local socket instead of each starting their own work. Requirements: single-instance lock, "
       "socket path resolution under both XDG and Windows paths, priority background indexing that yields to "
       "foreground interaction, back-pressure and a configurable CPU budget, clean shutdown with no "
       "half-written index, and fathom status --json exposing daemon health. --foreground keeps it runnable "
       "under a supervisor or in a container.",
  ac=bullet("Starting three TUIs plus a CLI causes exactly one indexing pass (asserted via counters)",
            "Under an actively committing agent the daemon stays under its configured CPU budget and the TUI stays responsive",
            "Ctrl-C, SIGTERM and a crash all leave the store consistent; restart resumes",
            "fathom status --json reports pid, uptime, offsets, queue depth and cache stats"),
  labels=["infra", "integration"])

t("e6.4", "e6", "MCP server: expose the review model back to the coding agent", "feature", 2, 600,
  deps=["e2.11", "e3.3", "e4.4", "e6.3"],
  desc="The reciprocal half of the relationship: the agent can ask the same questions the human can. Read "
       "tools - explain_episode, changes_between, architecture_delta, dependency_delta, risk_report, "
       "search_history - returning the versioned delta model with evidence ids, and gated write tools - "
       "mark_reviewed, add_review_note - behind an explicit config flag with an audit log line per call. "
       "Never return secrets or arbitrary file bodies; return references and summaries instead. Document "
       "token-size discipline: responses are bounded and paginated.",
  ac=bullet("Verified against a real MCP client session with the shipped config, not just unit tests",
            "Write tools are off by default; when enabled each call is logged with tool, args digest and result",
            "Responses are bounded and paginated; oversized results are truncated with an explicit cursor",
            "No file contents outside the model and no secrets appear in any response (test)"),
  labels=["integration", "mcp"])

t("e6.5", "e6", "Shared review memory: annotations, decisions and Q&A stored in the repo", "feature", 2, 480,
  deps=["e4.6", "e6.4"],
  desc="Reviews produce knowledge that should outlive the session. Let a human attach notes, decisions "
       "('we deliberately allow a cycle here because...'), questions and follow-ups to episodes or lines, "
       "stored as committable markdown under .fathom/reviews/ with a mirrored index, so they are reviewable "
       "in a PR, survive rebase, are searchable from the TUI and are answerable by the agent over MCP ("
       "'why did we do X' returns the recorded decision with its evidence). Decisions interlock with the "
       "ratchet in e3.10: granting an allowance writes both.",
  ac=bullet("A note attached to an episode is still linked after a rebase fixture",
            "fathom context <path-or-episode> prints the recorded decisions and open questions with dates and authors",
            "The agent can retrieve a recorded decision through an MCP tool and cite its evidence",
            "Notes appear in the export artefact (e5.7) and are diff-friendly"),
  labels=["workflow", "knowledge"])

t("e6.6", "e6", "fathom check: CI gate for architecture, dependencies and unreviewed risk", "feature", 1, 480,
  deps=["e2.6", "e3.6", "e3.7", "e3.10", "e4.4"],
  desc="The enforcement surface, and the reason this is infrastructure rather than a toy: fathom check "
       "evaluates policy (layering violations, new cycles, breaking API changes, dependency policy "
       "violations, untested new logic above a size threshold, high-risk episodes without sign-off) against "
       "a base..head range with severities, using ratchet baselines so existing debt does not block. Output "
       "formats: human, JSON, SARIF for code-scanning upload and GitHub step annotations plus a markdown job "
       "summary. Must run without network and without a full history fetch.",
  ac=bullet("Runs in under 60s on a medium repo with a shallow clone (measured in CI)",
            "SARIF validates against the code-scanning schema and uploads cleanly",
            "Policy config is per-repo, versioned and its failure modes are explicit (no silent passes)",
            "Fails only on new violations by default; the 'why did CI fail' message is one sentence long and points at the evidence"),
  labels=["ci", "policy", "mvp"])

t("e6.7", "e6", "Extensibility: detector API, example out-of-tree plugin and event bus", "feature", 3, 360,
  deps=["e2.11", "e4.3"],
  desc="Open the analysis without turning it into a support burden: a public trait for detectors (input: "
       "immutable delta model; output: versioned findings), a finding schema with namespacing and severity "
       "rules, an event bus emitting analysis lifecycle events for external tooling, and a "
       "--detectors/enable-disable config. Ship a worked third-party example in its own crate that compiles "
       "against the released library, plus a stability policy saying which APIs are covered.",
  ac=bullet("The example plugin builds and runs out-of-tree against the published crate in CI",
            "Finding schema is versioned and unknown finding types are ignored rather than fatal",
            "Disabling a built-in detector is supported and documented",
            "Stability policy for the plugin API is written down in the docs"),
  labels=["extensibility", "api"])

t("e6.8", "e6", "Privacy posture: no network by default and local-only opt-in metrics", "chore", 2, 180,
  deps=["e0.7"],
  desc="This tool reads your whole repository history, so privacy is a feature. Default build makes no "
       "outbound calls (advisory lookups and optional LLM narrative are separate opt-in feature/config "
       "gates). Any usage metrics are local-only counters in .fathom/ that the user can read and export, "
       "never transmitted. Document the posture in one page and verify it mechanically: a CI job that runs "
       "the binary with no network access and asserts identical behaviour, plus fathom doctor --privacy.",
  ac=bullet("CI job proves the default binary functions with no network egress available",
            "fathom doctor --privacy prints what is on, what is off and which config key turns each on",
            "Privacy page reviewed and linked from the README",
            "Generated SBOM and licence report published per release"),
  labels=["privacy", "docs"])

# ================================================================= EPIC 7
t("e7.1", "e7", "Messy-repository resilience sweep", "task", 2, 600,
  deps=["e1.4", "e2.4"],
  desc="Systematically break it, then make it degrade gracefully. Scenarios: shallow and partial clones, "
       "promisor-missing objects, orphan and hundreds of stale branches, merge storms (100 consecutive "
       "merges), force-push rewrites mid-session, submodules and nested worktrees, unicode and CRLF paths, "
       "500MB single files, binary asset directories, corrupt or pruned objects, an enormous author count, "
       "a repository with no merge commits ever, and a repo whose .git is on a network mount. Every scenario "
       "either works or degrades with an explicit, actionable message.",
  ac=bullet("Each scenario is an automated fixture; none panic and none lose the review ledger",
            "Degraded states are visible in the UI status line and in fathom doctor with a remedy",
            "Corrupt-object handling reports the affected commit and continues rather than aborting the index",
            "Findings logged as bugs with reproduction fixtures, not folklore"),
  labels=["hardening"])

t("e7.2", "e7", "Performance pass against real open-source repositories", "task", 2, 480,
  deps=["e0.11", "e3.2"],
  desc="Measure the three budgets that matter - cold index, warm re-index, interactive latency - on three "
       "real repositories chosen to stress different axes: one with 50k+ commits, one monorepo with many "
       "languages, and one dominated by generated code. Fix the top three regressions found, and where a "
       "budget is genuinely unreachable, consciously rewrite the budget in ADR-004 with a written rationale "
       "instead of letting the number rot.",
  ac=bullet("Benchmark report for the three repos committed under benchmarks/reports/",
            "Cold index, warm re-index and TUI latency meet ADR-004 or a revised budget is agreed in writing",
            "Peak RSS measured and within budget on all three",
            "Top three fixes are separate commits with before/after numbers in the message"),
  labels=["perf", "hardening"])

t("e7.3", "e7", "Memory ceilings, bounded caches and streaming for monorepos", "task", 2, 360,
  deps=["e3.2"],
  desc="Give memory a hard ceiling rather than a hope: configurable caps for the analysis cache, graph "
       "snapshots and decode buffers; LRU eviction with observability into what got evicted; disk spill for "
       "cold snapshots; and back-pressure so analyzers slow down instead of ballooning. The claim to prove is "
       "that peak RSS is a function of configuration, not of repository size.",
  ac=bullet("Indexing a 100k-commit fixture completes under a 512MB RSS limit (enforced in a test)",
            "Cache caps are observable via fathom stats including eviction counts",
            "Analyzers apply back-pressure instead of growing unbounded when the queue backs up",
            "Knobs are documented with their memory/time trade-offs"),
  labels=["perf", "hardening"])

t("e7.4", "e7", "Cross-platform packaging and distribution", "chore", 3, 480,
  deps=["e0.7"],
  desc="Ship installable artifacts for all three OSes with cargo-dist: macOS arm64 and x86_64 (signed and "
       "notarized once credentials exist), Linux tarballs plus deb and rpm, an musl static build, Windows "
       "zip and msi, a curl installer script, a Homebrew tap, shell completions for bash/zsh/fish and a man "
       "page. Build completions and the man page from the CLI definition so they cannot drift, and record "
       "which signing steps require secrets that a fork will not have.",
  ac=bullet("curl installer works on clean ubuntu, macOS and windows VMs and installs the right artifact",
            "Completions and man page are generated from clap definitions in CI (drift is impossible)",
            "Release artifacts include checksums and an SBOM",
            "Contributor docs state which release steps need maintainer-only secrets"),
  labels=["release", "infra"])

t("e7.5", "e7", "Documentation site: concepts, recipes, detector catalogue and cheatsheet", "task", 2, 480,
  deps=["e0.6", "e2.11", "e4.7"],
  desc="Docs that make the tool comprehensible without its author: a five-minute quickstart; concept pages "
       "for episode, delta, risk score, review ledger, ratchet and ambient mode; harness recipes; a detector "
       "catalogue where every entry has what it detects, why it matters for agent-written code, a worked "
       "example, and how to tune or silence it; the keyboard cheatsheet generated from the keymap so it never "
       "lies; a CLI reference generated from clap; and a troubleshooting page seeded from beta issues.",
  ac=bullet("Docs build in CI with link checking and generated CLI/keymap sections",
            "Every shipped feature has a linked doc page; a CI check fails if a feature has no doc entry",
            "Three outside users complete the quickstart unaided and can name what an episode is",
            "Detector catalogue covers every built-in detector with an example"),
  labels=["docs"])

t("e7.6", "e7", "Demo repository and scripted five-minute demo", "task", 3, 300,
  deps=["e0.10", "e5.1", "e5.7"],
  desc="A public demo repository containing a believable agent-built history with each pathology "
       "deliberately seeded (churn loop, unplanned dependency, new cycle, deleted test, reformat-only commit) "
       "plus a DEMO.md walkthrough and an asciinema/GIF recording under five minutes. The demo is the "
       "fastest way to explain the product, and it doubles as an end-to-end acceptance fixture.",
  ac=bullet("git clone of the demo plus one fathom command reproduces every documented finding",
            "A newcomer follows the script and finds all seeded pathologies unaided",
            "Recording is under five minutes and viewable without sound",
            "Demo repo version tagged and re-recorded at each minor release"),
  labels=["docs", "demo"])

t("e7.7", "e7", "Beta loop: install path, changelog, feedback intake and self-dogfooding", "chore", 3, 300,
  deps=["e7.4", "e7.5"],
  desc="Run a real beta: a CHANGELOG generated from conventional commits, issue and bug-report templates "
       "that require fathom doctor --json so reports arrive diagnosable, a single documented feedback "
       "channel, and a short intake cadence (triage weekly, publish what was fixed and why). Also dogfood: "
       "run Fathom on its own repository, put Fathom's own issues through the attention queue, and record "
       "what the tool says about itself - the most credible marketing and the best regression test there is.",
  ac=bullet("At least five external users onboarded with their feedback captured as beads",
            "At least twenty beta issues triaged with explicit dispositions",
            "Fathom runs against its own repository in CI and the artefact is published per release",
            "Release cadence and changelog process documented and followed for two consecutive releases"),
  labels=["release", "community"])

t("e7.8", "e7", "Security and privacy hardening review", "task", 2, 300,
  deps=["e2.7", "e5.7", "e6.8"],
  desc="A written pass rather than a hope: threat model covering path traversal and symlink escape in "
       "exports and notes, repository-provided config being untrusted input (a clone must not be able to "
       "run code), catastrophic regex backtracking, decompression and huge-input denial of service, SSRF "
       "via advisory endpoints, secrets leaking into logs or artefacts, and unsafe defaults in the gated "
       "write tools. Add fuzz targets for lockfile parsers, the hook event reader and path handling with a "
       "short CI smoke and longer nightly run.",
  ac=bullet("docs/security/threat-model.md reviewed with an owner and status per item",
            "cargo-fuzz targets exist for lockfile parsing, hook events and path handling; 30-minute smoke runs in CI",
            "Cloning a hostile repository cannot execute code or write outside .fathom/ (test)",
            "Every finding is fixed or explicitly accepted with a written rationale"),
  labels=["security", "hardening"])

# ================================================================= SPIKES
SPKES = [
 ("s1", "Spike: tree-sitter extraction accuracy across six languages", 1, 240,
  "Before committing to Epic 3, measure the thing it depends on. For Rust, Python, TypeScript, Go, Ruby and "
  "Java, build throwaway extractors and measure precision and recall for definitions, exports and import "
  "resolution against hand-annotated fixtures; record throughput and grammar version pinning; note per-language "
  "dealbreakers (conditional compilation in Rust, decorators in Python, barrel re-exports in TypeScript).",
  bullet("Report with per-language precision, recall and throughput against annotated fixtures",
         "Explicit go/no-go recommendation per language with grammar versions pinned",
         "Fallback strategy defined for languages that fail the threshold",
         "Thresholds become the acceptance bar referenced by e3.1")),
 ("s2", "Spike: episode segmentation accuracy on real agent-driven histories", 1, 240,
  "Segmentation is the load-bearing assumption of the whole product. Collect 200 agent turns from real sessions, "
  "hand-annotate the true episode boundaries, then compare boundary precision and recall for three strategies: "
  "time/author heuristics only, hook-event-driven boundaries, and trailers plus message-shape signals. Also "
  "measure how often segmentation mistakes would hide a real problem (i.e. a pathology spans a wrong boundary).",
  bullet("Annotated ground truth for 200 turns committed to the fixture corpus",
         "Precision and recall table for the three strategies",
         "Recommended default rule set with the failure modes enumerated",
         "Recommendation folded into ADR-003 before e1.3 is built")),
 ("s3", "Spike: cost of full graph-per-revision at 10k commits", 1, 240,
  "Epic 3 assumes a graph snapshot per revision is affordable with structural sharing; measure it before "
  "e3.2 is designed. Prototype snapshot storage with and without sharing, measure time and bytes per commit on "
  "a real 10k-commit repository, measure incremental query latency, and test alternative invalidation policies "
  "(full snapshot every N commits plus a delta window, on-demand recomputation, content-defined chunking). "
  "DONE: five strategies written into ADR-004 §4.6's real DDL and run over a 10 004-commit repository "
  "(2 298 modules, 54 025 graph rows); decision in ADR-004 §14.1, tables generated from "
  "benchmarks/reports/s3-graph-snapshots.json, method and limits in docs/concepts/graph-storage.md.",
  bullet("Measured time and bytes per revision for each strategy",
         "Recommended invalidation and storage policy with the numbers behind it",
         "Latency figures for the queries the TUI actually issues",
         "Decision recorded in ADR-004 and reflected in e3.2's approach")),
 ("s4", "Spike: rename and copy detection reliability across merges", 2, 180,
  "Folder-structure stories fall over if rename detection is wrong. Compare git rename detection, a "
  "content-similarity matcher and a hybrid on fixtures including rename-with-rewrite, file split and merge, "
  "renames that cross a merge commit and renames introduced by a rebase. Report how often a wrong pairing "
  "would mislead a reviewer, which is more damaging than a missing pairing.",
  bullet("Precision and recall per algorithm across the rename fixture set",
         "Thresholds and confidence calibration documented",
         "Recommendation for what to show as fact vs as a guess",
         "Findings reflected in e2.4's approach")),
 ("s5", "Spike: does LLM narrative polish beat the deterministic template", 2, 180,
  "Decide whether e4.7's optional LLM path earns its complexity. Generate episode narratives both ways for 20 "
  "real episodes, have three reviewers rate accuracy, usefulness and trustworthiness blind, and specifically "
  "count hallucinated claims not supported by the evidence set. Also record cost, latency and determinism.",
  bullet("Blind comparison ratings from three reviewers with hallucination counts per approach",
         "Cost and latency table per episode",
         "Written recommendation: ship the opt-in path, ship nothing, or gate it behind further work",
         "Outcome recorded as a decision issue linked from e4.7")),
]
SPIKE_DEPS = {"s5": ["e2.10"]}
for k, title, prio, est, desc, ac in SPKES:
    t(k, None, title, "spike", prio, est=est, desc=desc, ac=ac,
      deps=SPIKE_DEPS.get(k), labels=["spike"])

# ================================================================= MILESTONES
t("m0", None, "Milestone: Alpha - spine usable for daily review", "milestone", 1, deps=["e1.12", "e0.12"],
  desc="Alpha gate: epics 0 and 1 are complete and the tool is genuinely useful for reviewing an "
       "agent-generated repository day to day, even with no analysis at all. Publish alpha binaries, announce "
       "to a small group and start collecting real usage.",
  ac=bullet("All Epic 0 and Epic 1 tickets closed with no open P1 defects",
            "Spike recommendations reflected in the plan (e0.12 done)",
            "Alpha artifacts installable on three OSes",
            "At least three people outside the core team using it weekly"),
  labels=["milestone"])

t("m1", None, "Milestone: Beta - change intelligence, attention queue and export", "milestone", 2,
  deps=["e2.11", "e4.5", "e4.6", "e5.7", "e6.2"],
  desc="Beta gate: a reviewer can work the attention queue, understand dependencies, structure and non-code "
       "surfaces, and export a sign-off artefact that lands on a PR, with harness intent flowing in. Open the "
       "beta loop and start dogfooding Fathom on Fathom.",
  ac=bullet("Epic 2 complete; the core tickets of epics 4, 5 and 6 listed as dependencies are closed",
            "Risk-score false-positive rate under the threshold agreed in e0.12 on three real repos",
            "Export artefact used in at least one real PR review outside this project",
            "No open P1 defects against the beta scope"),
  labels=["milestone"])

t("m2", None, "Milestone: 1.0 - architecture awareness, CI gate and packaging", "milestone", 2,
  deps=["e3.9", "e3.10", "e6.4", "e6.6", "e7.4", "e7.5"],
  desc="1.0 gate: architecture deltas, cycles, layering and API surface are visible and enforceable, the "
       "agent can query the same model over MCP, CI can block on new violations, packaging and docs are "
       "credible, and the hardening sweep has been run. This is the release where Fathom plus a coding harness "
       "is a defensible answer to 'how do I deliver agentic software with confidence'.",
  ac=bullet("All epic 3 and epic 6 tickets closed; epic 7 hardening items closed or consciously deferred with a written reason",
            "fathom check running in at least two external CI pipelines",
            "Docs, demo repository and five-minute demo published",
            "Perf budgets met or consciously restated in ADR-004"),
  labels=["milestone"])

# ================================================================= SERIALIZE
def main():
    keys = set()
    for n in NODES:
        assert n["key"] not in keys, "duplicate key: " + n["key"]
        keys.add(n["key"])
    for n in NODES:
        for d in n.get("deps", []):
            assert d["target"] in keys, "unknown dep target: %s (in %s)" % (d["target"], n["key"])
        if "parent" in n:
            assert n["parent"] in keys, "unknown parent: %s (in %s)" % (n["parent"], n["key"])
    json.dump({"nodes": NODES}, sys.stdout, indent=1, sort_keys=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
