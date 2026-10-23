#!/usr/bin/env python3
"""Generate the four generated pages of the documentation site, and fail on drift.

The docs site is `docs/` (docs/index.md is its front door). Most of it is prose a person wrote, and
four of its pages are not: their tables are copies of things the program already knows, and a table
copied by hand is a table that starts lying the day the code changes. This tool is the only thing
allowed to write between the `<!-- BEGIN GENERATED: name -->` markers in those pages.

    python3 tools/gen_docs.py check                       # what CI runs, minus `--page cli`
    python3 tools/gen_docs.py check --page cli            # needs a built binary (see below)
    python3 tools/gen_docs.py write                       # refresh every page you may write
    python3 tools/gen_docs.py write --page detectors      # one page

| page         | generated from                                                     | needs a build |
|--------------|--------------------------------------------------------------------|---------------|
| `cheatsheet` | `src/recommender/tui/{keymap,views}.py` — the same tables as ADR-006 | no            |
| `detectors`  | `crates/fathom-analyze/tests/fixtures/detectors/` + `schemas/fathom.schema.json` | no |
| `features`   | `crates/fathom-cli/src/features.rs` + `crates/fathom-core/src/privacy.rs` | no       |
| `cli`        | `fathom --help` and `fathom <verb> [<subverb>] -h` off the binary   | **yes**       |

`cli` is separate because it is the only page whose source is a compiled program: `--bin` names the
binary (default `target/debug/fathom`, the default feature set, which is the shipped product,
ADR-002 §6.1). CI checks it in the leg that has already built the workspace, and `check` refuses to
pretend otherwise — a gate that skips when its input is missing is not a gate.

Exit: 0 clean, 1 drift or a missing input. One line per problem.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
sys.path.insert(0, str(ROOT / "src"))

MARK = re.compile(
    r"<!-- BEGIN GENERATED: ([\w.\-]+) -->\n(.*?)\n?<!-- END GENERATED: \1 -->", re.S)

DETECTOR_CATALOGUE = (
    ROOT / "crates/fathom-analyze/tests/fixtures/detectors/catalogue.json")
DETECTOR_FIXTURES = (
    ROOT / "crates/fathom-analyze/tests/fixtures/detectors/fixtures")
DETECTOR_CLEAN = ROOT / "crates/fathom-analyze/tests/fixtures/detectors/clean"
SCHEMA = ROOT / "schemas/fathom.schema.json"
FEATURES_RS = ROOT / "crates/fathom-cli/src/features.rs"
EXIT_RS = ROOT / "crates/fathom-core/src/exit.rs"
PRIVACY_RS = ROOT / "crates/fathom-core/src/privacy.rs"

PAGES = {
    "cheatsheet": DOCS / "reference" / "cheatsheet.md",
    "detectors": DOCS / "detectors" / "index.md",
    "features": DOCS / "reference" / "features.md",
    "cli": DOCS / "reference" / "cli.md",
}

#: The page each block belongs to, so `write --page cheatsheet` cannot rewrite the CLI reference.
PAGE_OF_BLOCK = {
    "modes": "cheatsheet", "keys": "cheatsheet", "keymap": "cheatsheet",
    "actions": "cheatsheet", "tasks": "cheatsheet", "reserved": "cheatsheet",
    "hint": "cheatsheet", "views": "cheatsheet",
    "summary": "detectors", "knobs": "detectors", "clean": "detectors",
    "disabled": "detectors",
    "features": "features", "privacy": "features",
    "usage": "cli", "verbs": "cli", "globals": "cli", "exit": "cli",
}


# ---------------------------------------------------------------------------
# markdown table helper (same shape as recommender.tui.docs._table)
# ---------------------------------------------------------------------------
def table(header: tuple[str, ...], rows: list[tuple[str, ...]]) -> str:
    out = ["| " + " | ".join(header) + " |",
           "|" + "|".join("---" for _ in header) + "|"]
    out += ["| " + " | ".join(c.replace("|", "\\|") for c in row) + " |" for row in rows]
    return "\n".join(out)


def fence(text: str, lang: str = "text") -> str:
    body = text.rstrip("\n")
    marker = "```"
    while marker in body:  # a hunk containing a fence must not break the fence
        marker += "`"
    return "%s%s\n%s\n%s" % (marker, lang, body, marker)


def anchor(heading: str) -> str:
    """The fragment a GitHub-style renderer gives a heading, so a generated link resolves.

    Lowercase, punctuation dropped, spaces to hyphens. `check_docs_site.py` recomputes the same
    function over every heading in the site and fails a link that lands nowhere, which is what
    makes it safe for a generated table to link at a heading it did not write.
    """
    text = heading.strip().lower()
    text = re.sub(r"[^a-z0-9 \-_]", "", text)
    return text.replace(" ", "-")


# ---------------------------------------------------------------------------
# cheatsheet — every block is recommender.tui's, because that is the shipped keymap
# ---------------------------------------------------------------------------
def cheatsheet_blocks() -> dict[str, str]:
    from recommender.tui import docs as D
    from recommender.tui import keymap as K

    out = {
        "modes": D.modes_table(),
        "keymap": D.keymap_table(),
        "actions": D.action_registry(),
        "tasks": D.tasks_table(),
        "reserved": D.reserved_chords(),
        "hint": D.hint_line(),
    }

    # `views` and `keys` are the two views of the same table a skimming reader wants: one row per
    # view, then one row per keystroke grouped by what the keystroke is for.
    rows = []
    for v in D.VIEWS:
        keys = sorted({c.split(":", 1)[1]
                       for c in K.binding_index().get("view.%s" % v.id, [])})
        rows.append(("**%s**" % v.title, "`%s`" % v.id,
                     " ".join("`%s`" % k for k in keys) or "*(no global key)*",
                     v.question))
    out["views"] = table(("View", "Mode", "Go with", "The question it answers"), rows)

    index = K.binding_index()
    groups = (("frame", "Move around"), ("review", "Review the episode"),
              ("scope", "Change what you are looking at"),
              ("overlay", "Search, palette, help"),
              ("motion", "Motion, every list"), ("index", "Index and screen"),
              ("view", "View-local"))
    rows = []
    for scope, title in groups:
        for action_id, act in sorted(K.ACTIONS.items()):
            if act.scope != scope:
                continue
            keys = sorted({c.split(":", 1)[1] for c in index.get(action_id, [])})
            if not keys:
                continue
            rows.append(("**%s**" % title,
                         " ".join("`%s`" % k for k in keys),
                         act.title,
                         ", ".join("`%s`" % m for m in act.modes)
                         if act.modes != ("global",) else "global"))
    out["keys"] = table(("Group", "Key", "Does", "Live in"), rows)
    return out


# ---------------------------------------------------------------------------
# detectors — the catalogue, the knobs and one worked example per detector
# ---------------------------------------------------------------------------
#: Which `detectors.*` knob each detector reads. `policy.rs` parses the keys; this says who uses
#: them, which is the half the schema cannot state (one key per detector, three keys shared).
KNOB_OWNER = {
    "churn": "churn_loop", "rework": "rework_loop", "shotgun": "shotgun_edit",
    "copy": "copy_paste_growth", "untested": "untested_new_logic", "todo": "todo_debt",
}
SHARED_KNOBS = {"enabled": "every detector", "suppress": "every detector",
                "max_findings": "every detector"}


def catalogue() -> list[dict]:
    return json.loads(DETECTOR_CATALOGUE.read_text())["detectors"]


def _walk_schema(node: dict, prefix: str, into: dict[str, dict]) -> None:
    for key, spec in sorted((node.get("properties") or {}).items()):
        dotted = "%s.%s" % (prefix, key) if prefix else key
        if "properties" in spec:
            _walk_schema(spec, dotted, into)
        elif "default" in spec or "description" in spec:
            into[dotted] = spec


def detector_knobs() -> dict[str, dict]:
    node = json.loads(SCHEMA.read_text())["properties"]["detectors"]
    out: dict[str, dict] = {}
    _walk_schema(node, "detectors", out)
    return out


def _episode_digest(episode: dict) -> tuple[str, list[str]]:
    """A short, readable rendering of a fixture episode: header line, then per-file hunks."""
    lines = ["%s · %s author · %s commit(s)%s" % (
        episode.get("id", "?"), episode.get("author_kind", "?"),
        episode.get("n_commits", len(episode.get("files", [])) or 1),
        "" if not episode.get("declared_scope")
        else " · declared scope: %s" % ", ".join(episode["declared_scope"]),
    )]
    for change in episode.get("files", []):
        lines.append("%s %s  +%s −%s" % (
            {"added": "A", "modified": "M", "deleted": "D",
             "renamed": "R"}.get(change.get("change", ""), "?"),
            change.get("path", "?"), change.get("add", 0), change.get("del", 0)))
        for hunk in change.get("hunks", [])[:2]:
            lines.append("  %s" % hunk.get("header", ""))
            for line in hunk.get("removed", [])[:4]:
                lines.append("  - %s" % line.strip())
            for line in hunk.get("added", [])[:6]:
                lines.append("  + %s" % line.strip())
            if len(change.get("hunks", [])) > 2:
                lines.append("  … %d more hunks" % (len(change["hunks"]) - 2))
    return lines[0], lines[1:]


def example_block(detector_id: str) -> str:
    """One worked example, taken from the fixture that detector must fire on."""
    src = DETECTOR_FIXTURES / ("%s.json" % detector_id)
    expected = DETECTOR_FIXTURES / ("%s.expected.json" % detector_id)
    if not src.exists() or not expected.exists():
        raise SystemExit("no fixture for detector %r: %s" % (detector_id, src))
    episode_doc = json.loads(src.read_text())
    findings = json.loads(expected.read_text())
    episode = episode_doc.get("episode", episode_doc)
    prior = episode_doc.get("prior", [])
    head, body = _episode_digest(episode)

    out = ["*The input* (`fixtures/%s.json`):" % detector_id, "",
           fence("\n".join([head] + body))]
    if prior:
        prior_lines = ["%s · %s author · %s commit(s)" % (
            p.get("id", "?"), p.get("author_kind", "?"),
            p.get("n_commits", len(p.get("files", [])) or 1)) for p in prior[:4]]
        if len(prior) > 4:
            prior_lines.append("… %d earlier episodes in the window" % (len(prior) - 4))
        out += ["", "The episodes before it (the history this detector argues from):", "",
                fence("\n".join(prior_lines))]
    rows = [(
        "`%s`" % f["severity"], "`%s`" % f["confidence"], f["message"],
        "`%s:%s`" % (f["jump_view"], f["jump_target"]), "`%s`" % f["suppression_key"],
    ) for f in findings[:6]]
    out += ["", "*What the shipped detector reports* (`fixtures/%s.expected.json`, re-scored in CI):"
            % detector_id, "",
            table(("Severity", "Confidence", "Message", "Jump", "Suppress with"), rows)]
    if len(findings) > 6:
        out += ["", "_… and %d more findings on this fixture._" % (len(findings) - 6)]
    return "\n".join(out)


def detectors_blocks() -> dict[str, str]:
    cands = catalogue()
    rows = []
    for d in cands:
        rows.append((
            "[`%s`](#%s)" % (d["id"], anchor("`%s` \u2014 %s" % (d["id"], d["title"]))),
            d["asks"],
            "`%s`" % d["needs"],
            "**on**" if d["default_on"] else "off",
            "`v%s`" % d["version"],
        ))
    out = {
        "summary": table(("Detector", "The question it asks", "Needs", "Ships", "Version"), rows),
    }

    off = [("[`%s`](#%s)" % (d["id"], anchor("`%s` \u2014 %s" % (d["id"], d["title"]))),
            d["reenable"] or "\u2014") for d in cands if not d["default_on"]]
    out["disabled"] = table(("Detector", "What it is waiting for, in its own words"), off)

    knobs = detector_knobs()
    blessed = json.loads(DETECTOR_CATALOGUE.read_text())["knobs"]
    rows = []
    for key, spec in sorted(knobs.items()):
        leaf = key.rsplit(".", 1)[-1]
        owner = SHARED_KNOBS.get(leaf, "")
        for token, detector in KNOB_OWNER.items():
            if ".%s." % token in key:
                owner = detector
        if not owner:
            owner = "—"
        # Defaults come from the blessed catalogue when it carries the key: `catalogue.json` is
        # asserted against the Rust `CATALOGUE`, while the schema's own `detectors.enabled` default
        # still names an id this build does not have (`test_deletion`). The closer-to-truth source
        # wins here, and the page says so rather than quietly picking a side.
        default = blessed.get(key.split("detectors.", 1)[-1], spec.get("default", "—"))
        if isinstance(default, list):
            default = ", ".join(str(item) for item in default)
        rows.append(("`%s`" % key,
                     "`%s`" % default,
                     "`%s`" % (spec.get("type", "—")),
                     owner,
                     (spec.get("description") or "").replace("\n", " ")))
    out["knobs"] = table(("Key", "Default", "Type", "Read by", "What it decides"), rows)

    clean = sorted(DETECTOR_CLEAN.glob("*.expected.json"))
    total = sum(len(json.loads(p.read_text())) for p in clean)
    out["clean"] = table(
        ("Clean corpus", "Episodes", "Findings every detector must not produce"),
        [("[`clean/`](../../crates/fathom-analyze/tests/fixtures/detectors/clean)",
          str(len(clean)), str(total))])

    for d in cands:
        out["example.%s" % d["id"]] = example_block(d["id"])
    return out


# ---------------------------------------------------------------------------
# features — the compiled-in set, from the crate that prints it
# ---------------------------------------------------------------------------
DECLARED_RE = re.compile(r'\("([a-z0-9\-]+)",\s*cfg!\(feature\s*=\s*"([a-z0-9\-]+)"\)\)')
NET_ROW_RE = re.compile(
    r"feature:\s*\"([a-z0-9\-]+)\",\s*owner:\s*\"([a-z0-9\-]+)\",\s*"
    r"consent_key:\s*\"([a-z0-9.\_]+)\",\s*ticket:\s*\"([a-z0-9.]+)\"", re.S)


def declared_features() -> list[str]:
    text = FEATURES_RS.read_text()
    body = text.split("const DECLARED", 1)[1].split("];", 1)[0]
    pairs = DECLARED_RE.findall(body)
    if not pairs:
        raise SystemExit("features.rs: no DECLARED entries parsed — the generator is out of date")
    return [name for name, _feature in pairs]


def net_features() -> list[dict]:
    text = PRIVACY_RS.read_text()
    return [{"feature": f, "owner": o, "consent_key": k, "ticket": t}
            for f, o, k, t in NET_ROW_RE.findall(text)]


#: feature -> the page that explains what turning it on changes. `tools/check_docs_site.py` fails
#: when `features.rs` grows a feature this map does not have, or when a target stops existing.
FEATURE_DOCS = {
    "open": (["docs/concepts/handoff.md", "docs/concepts/diff-viewer.md"],
             "jump from an episode to the file, at its line, in your editor"),
    "difftool": (["docs/concepts/handoff.md", "docs/concepts/diff-viewer.md"],
                 "one file's two sides, in your own difftool"),
    "tui": (["docs/reference/cheatsheet.md", "docs/adr/0006-tui.md"],
            "the terminal UI: every view, the keymap, the layout ladder."),
    "mcp": (["docs/reference/cli.md", "docs/adr/0005-harness-contract.md"],
            "`fathom mcp serve`, the tools an agent may ask; the two write tools are off until `mcp.allow_writes` turns them on and starts their audit trail (ADR-005 §4.2)."),
    "watch": (["docs/recipes/run-beside-the-agent.md"],
              "`fathom watch` and `fathom index --watch`: index as commits land."),
    "sqlite-bundled": (["docs/adr/0004-storage.md"],
                       "the store: SQLite compiled into the binary rather than found on the host."),
    "langs-all": (["docs/concepts/structure-extraction.md"],
                  "all six tree-sitter grammars, so structure extraction covers the tree."),
    "deps-all": (["docs/concepts/dependency-surface.md"],
                 "every dependency ecosystem, so a lockfile is never silently unread."),
    "minimal": (["docs/adr/0002-workspace.md"],
                "the headless build for a CI gate: no terminal, no store, no grammars."),
    "net-advisories": (["docs/concepts/dependency-risk.md"],
                       "outbound advisory lookups, and only with `privacy.advisories`."),
    "net-llm": (["docs/concepts/narrative-polish.md"],
                "an outbound model call. **Not shipped** — `e4.3a` measured it and it lost."),
    "net-http-transport": (["docs/adr/0005-harness-contract.md"],
                           "MCP over HTTP instead of stdio, gated the same way."),
    "schema-gen": (["docs/concepts/delta-model.md", "docs/reference/cli.md"],
                   "`--schema --write`, which is how the checked-in schemas are made."),
}


def _rel_to_page(path: str) -> str:
    """A link from `docs/reference/features.md` to a repo-relative doc path."""
    return os.path.relpath(path, "docs/reference")


def features_blocks() -> dict[str, str]:
    consent = {row["feature"]: row for row in net_features()}
    rows = []
    for name in declared_features():
        pages, what = FEATURE_DOCS[name]
        links = " · ".join("[%s](%s)" % (pathlib.Path(p).name, _rel_to_page(p))
                           for p in pages)
        gate = consent.get(name)
        rows.append(("`%s`" % name, what,
                     "`%s`" % gate["consent_key"] if gate else "—",
                     "`%s`" % gate["ticket"] if gate else "—", links))
    privacy_rows = [(
        "`%s`" % row["feature"], "`%s`" % row["owner"], "`%s`" % row["consent_key"],
        "`%s`" % row["ticket"],
        "[`refusal()`](../../crates/fathom-core/src/privacy.rs)") for row in net_features()]
    return {"features": table(
        ("Feature", "What turning it on changes", "Consent key", "Plan", "Documented in"), rows),
        "privacy": table(("Feature", "Owner crate", "Consent key", "Plan", "The refusal"),
                         privacy_rows)}


# ---------------------------------------------------------------------------
# cli — the only page whose source is a built program
# ---------------------------------------------------------------------------
def run_help(binary: pathlib.Path, argv: list[str]) -> str:
    proc = subprocess.run([str(binary)] + argv + ["-h"], capture_output=True, text=True)
    if proc.returncode != 0:
        raise SystemExit("%s %s -h exited %d: %s"
                         % (binary, " ".join(argv), proc.returncode, proc.stderr.strip()))
    return proc.stdout


def run_long_help(binary: pathlib.Path, argv: list[str]) -> str:
    proc = subprocess.run([str(binary)] + argv + ["--help"], capture_output=True, text=True)
    if proc.returncode != 0:
        raise SystemExit("%s %s --help exited %d: %s"
                         % (binary, " ".join(argv), proc.returncode, proc.stderr.strip()))
    return proc.stdout


def help_sections(text: str) -> dict[str, list[str]]:
    """Split clap's help into its `Usage:` / `Commands:` / `Arguments:` / `Options:` sections."""
    sections: dict[str, list[str]] = {}
    current = "top"
    sections[current] = []
    for line in text.splitlines():
        if re.match(r"^(Usage|Commands|Arguments|Options|Global Options):\s*$", line):
            current = line.rstrip(":")
            sections.setdefault(current, [])
            continue
        sections[current].append(line)
    return {k: v for k, v in sections.items() if v}


def subcommands(text: str) -> list[tuple[str, str]]:
    """The `Commands:` block of a help screen, minus clap's own `help`."""
    sections = help_sections(text)
    rows: list[tuple[str, str]] = []
    for line in sections.get("Commands", []):
        match = re.match(r"^  ([a-z][a-z0-9\-]*)(\s\s+)(.*)$", line)
        if match and match.group(1) != "help":
            rows.append((match.group(1), match.group(3).strip()))
    return rows


def usage_line(text: str) -> str:
    for line in text.splitlines():
        if line.startswith("Usage:"):
            return line[len("Usage:"):].strip()
    return "?"


#: verb -> (pages that explain it, the sentence this page uses). A shipped verb with no entry here
#: fails `gen_docs.py`, which is the mechanical half of "every shipped feature has a doc page".
#: Paths are repo-relative; they are rewritten relative to `docs/reference/cli.md`.
VERB_DOCS = {
    "index": (["docs/concepts/onboarding.md", "docs/concepts/cli.md"],
              "build the index every other verb reads"),
    "show": (["docs/concepts/episode.md", "docs/concepts/cli.md"],
             "one episode, with its file rows"),
    "episodes": (["docs/concepts/cli.md", "docs/concepts/timeline.md"],
                 "the episode list, as rows — the rail without a screen"),
    "attribution": (["docs/concepts/episode.md", "docs/concepts/cli.md"],
                    "who wrote it, and how to overrule that"),
    "review": (["docs/concepts/review-ledger.md", "docs/concepts/cli.md"],
               "the sign-offs, and whether they still match"),
    "analyze": (["docs/concepts/delta.md", "docs/concepts/delta-model.md"],
                "the delta document, human or JSON"),
    "narrative": (["docs/concepts/episode-narrative.md", "docs/concepts/episode.md"],
                  "the prose, with a citation per claim"),
    "drift": (["docs/concepts/plan-drift.md", "docs/concepts/session-state.md"],
              "what the plan promised against what landed"),
    "why": (["docs/concepts/risk-score.md", "docs/concepts/risk-scoring.md"],
            "the risk score as an auditable sum"),
    "stats": (["docs/concepts/cli.md", "docs/adr/0004-storage.md"],
              "scale, index state, cache hit rate"),
    "check": (["docs/recipes/gate-a-pipeline.md", "docs/reference/cli.md"],
              "the CI gate; the verb is `e6.6`'s"),
    "doctor": (["docs/troubleshooting.md", "docs/concepts/cli.md"],
               "the build, the environment and the index, with remedies"),
    "export": (["docs/concepts/export-artefact.md", "docs/recipes/export-for-a-pr.md"],
               "the review artefact, three renderings"),
    "mcp": (["docs/adr/0005-harness-contract.md", "docs/recipes/ask-an-agent.md"],
            "the agent-facing surface; the server is `e6.4`'s"),
    "watch": (["docs/recipes/run-beside-the-agent.md", "docs/concepts/session-state.md"],
              "index as commits land"),
    "daemon": (["docs/concepts/watch-daemon.md", "docs/concepts/cli.md"],
               "the one writer per repository: lease, socket, budget"),
    "status": (["docs/concepts/watch-daemon.md", "docs/concepts/cli.md"],
               "who is keeping this index current, and what it costs"),
    "ambient": (["docs/concepts/ambient-mode.md", "docs/adr/0006-tui.md"],
                "the glance pane for the second terminal"),
    "hooks": (["docs/harness/README.md", "docs/adr/0005-harness-contract.md"],
              "the harness event stream, and why a line was refused"),
    "open": (["docs/concepts/handoff.md", "docs/concepts/diff-viewer.md"],
             "jump from an episode to the file, at its line, in your editor"),
    "difftool": (["docs/concepts/handoff.md", "docs/concepts/diff-viewer.md"],
                 "one file's two sides, in your own difftool"),
    "tui": (["docs/reference/cheatsheet.md", "docs/adr/0006-tui.md"],
            "the keymap, the session state and the theme in force"),
    "cmp": (["docs/adr/0006-tui.md", "docs/concepts/delta.md"],
            "the composed delta between any two episodes or revisions"),
    "search": (["docs/adr/0006-tui.md", "docs/concepts/timeline.md"],
               "one fuzzy box over episodes, paths, symbols, dependencies and findings"),
    "context": (["docs/concepts/review-memory.md", "docs/concepts/review-ledger.md"],
                "what this repository already knows about a path or an episode"),
    "note": (["docs/concepts/review-memory.md", "docs/concepts/cli.md"],
             "the review memory: decisions, questions and follow-ups, committed"),
}


def exit_codes() -> list[tuple[str, str, str]]:
    """The published exit codes, out of `fathom_core::exit::ExitCode` and its own doc comments.

    The enum is the contract and ADR-002 §7.3 is its rendering; restating the numbers here by hand
    would be the third copy, so this parses the source instead.
    """
    text = EXIT_RS.read_text()
    body = text.split("pub enum ExitCode", 1)[1].split("\n}", 1)[0]
    names = dict(re.findall(r'ExitCode::([A-Za-z]+)\s*=>\s*"([A-Z_0-9]+)"', text))
    rows: list[tuple[str, str, str]] = []
    doc: list[str] = []
    for line in body.splitlines():
        stripped = line.strip()
        if stripped.startswith("///"):
            doc.append(stripped[3:].strip())
            continue
        match = re.match(r"([A-Za-z]+)\s*=\s*(\d+),", stripped)
        if not match:
            doc = []
            continue
        rows.append((match.group(2), names.get(match.group(1), match.group(1).upper()),
                     " ".join(part for part in doc if part) or "\u2014"))
        doc = []
    if not rows:
        raise SystemExit("exit.rs: no exit codes parsed \u2014 the generator is out of date")
    return rows


def exit_codes_table() -> str:
    return table(("Code", "Name", "Means"),
                 [("`%s`" % code, "`%s`" % name, meaning)
                  for code, name, meaning in exit_codes()])


def cli_blocks(binary: pathlib.Path) -> dict[str, str]:
    root_help = run_long_help(binary, [])
    root_short = run_help(binary, [])
    out: dict[str, str] = {}
    out["usage"] = fence(usage_line(root_short))

    # The verb table: the one-line summary clap publishes, the status the build actually has, and
    # the page that explains it. `status` is a claim about the program, so it is taken from the
    # source rather than from memory — see `tools/check_docs_site.py`.
    implemented = implemented_verbs()
    rows = []
    for name, summary in subcommands(root_short):
        if name not in VERB_DOCS:
            raise SystemExit("verb %r has no doc entry: add it to VERB_DOCS in tools/gen_docs.py "
                             "and give it a page (acceptance criterion: no shipped verb is "
                             "undocumented)" % name)
        pages, _what = VERB_DOCS[name]
        links = " \u00b7 ".join("[*%s*](%s)" % (pathlib.Path(d).stem, _rel_to_page(d))
                                for d in pages)
        rows.append(("**[`%s`](#fathom-%s)**" % (name, name), summary.replace("`", ""),
                     "shipped" if name in implemented else "*refuses: names its plan item*",
                     links))
    out["verbs"] = table(("Verb", "What it answers", "This build", "Documented in"), rows)

    globals_section = help_sections(root_help).get("Options", [])
    out["globals"] = fence("\n".join(globals_section).strip())

    out["exit"] = exit_codes_table()

    for name, _summary in subcommands(root_short):
        short = run_help(binary, [name])
        out["help.%s" % name] = fence(short)
        for sub, _sub_summary in subcommands(short):
            out["help.%s.%s" % (name, sub)] = fence(run_help(binary, [name, sub]))
    return out


def verb_variants() -> list[str]:
    """The verb names clap publishes, from `enum Verb`'s variants (kebab-cased, as typed)."""
    text = (ROOT / "crates/fathom-cli/src/verbs.rs").read_text()
    body = text.split("pub enum Verb", 1)[1].split("\n}", 1)[0]
    variants = re.findall(r"^\s{4}([A-Z][A-Za-z]+)(?:\(|,$)", body, re.M)
    if not variants:
        raise SystemExit("verbs.rs: no `enum Verb` variants parsed — the generator is out of date")
    return [re.sub(r"(?<!^)(?=[A-Z])", "-", variant).lower() for variant in variants]


def pending_verbs() -> set[str]:
    """Verbs that answer with a refusal naming the plan item that owns them.

    Two shapes, both honest and both read here rather than remembered: an arm of the dispatch that
    calls `not_implemented("fathom check", …)`, and a handler whose own body raises a `pending`
    refusal (`mcp` is that one — the server is compiled in, `e6.4` has not written it)."""
    text = (ROOT / "crates/fathom-cli/src/verbs.rs").read_text()
    pending = set(re.findall(r'not_implemented\("fathom ([a-z\-]+)"', text))
    for name in verb_variants():
        marker = "fn %s(" % name.replace("-", "_")
        start = text.find(marker)
        if start < 0:
            continue
        body = text[start + len(marker):]
        stop = body.find("\nfn ")
        if stop > 0:
            body = body[:stop]
        if "not implemented" in body:
            pending.add(name)
    return pending


def implemented_verbs() -> set[str]:
    """The published verbs that are not refusals — the ones a recipe may tell someone to type."""
    return set(verb_variants()) - pending_verbs()


# ---------------------------------------------------------------------------
# splice / check / write
# ---------------------------------------------------------------------------
def blocks_for(page: str, binary: pathlib.Path) -> dict[str, str]:
    if page == "cheatsheet":
        return cheatsheet_blocks()
    if page == "detectors":
        return detectors_blocks()
    if page == "features":
        return features_blocks()
    if page == "cli":
        return cli_blocks(binary)
    raise SystemExit("unknown page %r (one of %s)" % (page, ", ".join(sorted(PAGES))))


def page_blocks(page: str, blocks: dict[str, str]) -> dict[str, str]:
    return {name: body for name, body in blocks.items()
            if PAGE_OF_BLOCK.get(name, page) == page}


def splice(text: str, generated: dict[str, str]) -> tuple[str, list[str]]:
    problems: list[str] = []
    seen: set[str] = set()

    def sub(match: re.Match) -> str:
        name = match.group(1)
        if name not in generated:
            problems.append("generated block %r is no longer produced" % name)
            return match.group(0)
        seen.add(name)
        return "<!-- BEGIN GENERATED: %s -->\n%s\n<!-- END GENERATED: %s -->" % (
            name, generated[name], name)

    new = MARK.sub(sub, text)
    missing = sorted(set(generated) - seen)
    if missing:
        problems.append("page does not use these generated blocks: %s" % ", ".join(missing))
    return new, problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="gen_docs.py", description=__doc__.splitlines()[0])
    parser.add_argument("action", choices=["check", "write"])
    parser.add_argument("--page", action="append", choices=sorted(PAGES),
                        help="one page (repeatable); default: every page that needs no build")
    parser.add_argument("--bin", default=str(ROOT / "target/debug/fathom"),
                        help="the fathom binary `cli` is generated from")
    args = parser.parse_args(argv)

    pages = args.page or [p for p in PAGES if p != "cli"]
    binary = pathlib.Path(args.bin)
    if "cli" in pages and not binary.exists():
        print("page `cli` needs a built binary at %s; run "
              "`cargo build -p fathom-cli` or pass --bin" % binary)
        return 1

    problems: list[str] = []
    for page in pages:
        path = PAGES[page]
        if not path.exists():
            problems.append("%s: page is missing" % path.relative_to(ROOT))
            continue
        generated = page_blocks(page, blocks_for(page, binary))
        text = path.read_text()
        new, page_problems = splice(text, generated)
        for problem in page_problems:
            problems.append("%s: %s" % (path.relative_to(ROOT), problem))
        if args.action == "write" and new != text:
            path.write_text(new)
            print("%s: wrote %d generated block(s)" % (path.relative_to(ROOT), len(generated)))
        elif args.action == "check" and new != text:
            problems.append("%s is stale: run `python3 tools/gen_docs.py write --page %s`"
                            % (path.relative_to(ROOT), page))
    for problem in problems:
        print(problem)
    if problems:
        print("\n%d problem(s)" % len(problems))
        return 1
    print("docs pages OK: %s" % ", ".join(sorted(pages)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
