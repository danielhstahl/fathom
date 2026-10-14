import json

NODES = []
def t(key, parent, title, typ="task", prio=2, est=None, deps=None, desc="", ac="", notes=None, labels=None):
    n = {"key": key, "title": title, "type": typ, "priority": prio}
    if parent: n["parent"] = parent
    if est: n["estimate"] = est
    if desc: n["description"] = desc
    if ac: n["acceptance_criteria"] = ac
    if notes: n["notes"] = notes
    if deps: n["deps"] = [{"target": d, "type": "depends-on"} for d in deps]
    if labels: n["labels"] = labels
    NODES.append(n)

# ---------------------------------------------------------------- EPICS
EPICS = [
 ("e0","Epic 0 - Foundations: product thesis, architecture decisions, workspace",1,
  "Everything that must exist before we can write product code: the product thesis, the five architecture ADRs, the cargo workspace, the test-fixture machinery, and the perf budget. Exit criteria: `fathom --help` runs in CI on 3 OSes and a synthetic repo can be indexed end-to-end."),
 ("e1","Epic 1 - Git spine: episodes, ingestion, timeline and diff viewing",1,
  "The MVP backbone. Fathom observes a repo, turns git history into 'episodes' (one agent turn = one reviewable unit), and lets a human fly through the timeline and read diffs with zero friction. Exit criteria: a human can review a 500-commit agent-driven repo entirely in the TUI without leaving it."),
 ("e2","Epic 2 - Change intelligence: topography, churn and dependencies",1,
  "Turn raw diffs into named, evidence-backed observations: which parts of the tree are in motion, what moved/renamed, what dependencies appeared/changed, and what is noise (generated files, lockfiles, reformat). Exit criteria: for each episode the user sees WHERE the change landed and WHAT it now depends on, not just line diffs."),
 ("e3","Epic 3 - Architecture awareness: module graph, API surface and cycles",2,
  "The differentiator. Build a per-revision model of module/symbol structure and imports, then show the DELTA of that model between agent turns: new edges, new cycles, layering violations, public API changes, god modules. Exit criteria: user can answer 'did the agent just make the architecture worse?' in under 5 seconds."),
 ("e4","Epic 4 - Agent-aware signals, risk scoring and the attention queue",2,
  "Make the tool agent-native rather than a generic git viewer: attribution, intent capture, and detectors for agent pathologies (churn loops, scope creep, untested additions, shotgun edits) rolled into an explainable per-episode risk score."),
 ("e5","Epic 5 - Ergonomics: ambient mode, comparison, search, export",2,
  "Fathom lives in a second terminal next to the coding harness. It must be glanceable, keyboard-first, usable at 80x24, and able to export a review artefact a human can post in a PR."),
 ("e6","Epic 6 - Hardening, performance, packaging and release",3,
  "Real repos are messy and huge. Failure tolerance, perf budgets, cross-platform packaging, docs, demo, and a beta loop."),
]
for k, title, prio, desc in EPICS:
    t(k, None, title, "epic", prio, est=None, desc=desc, labels=[k.replace("e","phase")])
