"""`fathom card …` — the verb surface of `reco-4ey`, and the only IO in the package.

```
python3 -m recommender.card card --fixture graph_full          # the blessed card
python3 -m recommender.card card --manifest episode.json       # a real episode
python3 -m recommender.card card --repo AFTER --old-repo BEFORE
python3 -m recommender.card summary --manifest episode.json    # the one-screen block only
python3 -m recommender.card figures --manifest e.json --section what
python3 -m recommender.card figure --manifest e.json --section blast --figure "basis"
python3 -m recommender.card blast --manifest e.json --all
python3 -m recommender.card attention --manifest e.json
python3 -m recommender.card verify --manifest e.json           # exit non-zero if it cannot prove itself
python3 -m recommender.card sections ; … views ; … policy ; … definition
python3 -m recommender.card score ; … bench
python3 -m recommender.card demo
```

An episode arrives three ways, in increasing order of fuss, exactly as in `fathom structure`:
`--repo AFTER --old-repo BEFORE` walks two working trees (the only filesystem access in the
package, ADR-002 §4.5, and the walk doubles as the file inventory the blast heuristics need);
`--fixture NAME` uses a labelled corpus case; and `--manifest FILE` (or `-`) carries the whole
thing in the shape `e2.11` will hand consumers:

```json
{"id": "ep-7c1", "title": "add scaled_total, drop a test",
 "changes": [{"path": "src/cart.py", "change": "modified", "before": "…", "after": "…"},
              {"path": "db/migrate/2026….rb", "change": "added", "after": "…"}],
 "before_paths": ["src/cart.py", "…"], "after_paths": ["src/cart.py", "…"],
 "deps": {"headline": "2 new direct dependencies", "direct_added": 2, "upgraded": 3,
           "ecosystems": ["cargo"], "evidence": ["Cargo.toml line 24"]},
 "graph": {"nodes": ["src/cart.py", "src/report.py"],
            "edges": [["src/report.py", "src/cart.py"]]},
 "risk": {"score": 82, "rank": 4, "of": 7, "reasons": ["migration drops a column"]},
 "absent": {"dependencies": "the index holds no dependency state for this revision"}}
```

`deps` is the shape `fathom-analyze::deps` produces, `graph` is `e3.2`'s, and `risk` is `e4.4`'s;
omitting any of them is a normal case, not an error — the card prints the gap and the verb that
fills it. `--no-deps` / `--no-graph` force the degraded path so a reviewer (and CI) can see what
the card does without an analysis that has not landed.

The config flags are `e0.8`'s verbatim, so `fathom card policy` and `fathom config explain`
cannot disagree about which layer won.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import replace
from pathlib import Path

from recommender.config.cli import common_flags
from recommender.config.errors import EXIT_CODES
from recommender.config.layer import load as load_config

from . import corpus as CORPUS
from . import eval as EVAL
from . import explain as X
from . import render
from .compose import compose
from .model import Absent, Analyses, Change, DepsDelta, Episode, ModuleGraph, RiskScore
from .policy import SECTION_IDS, Policy

#: The CLI's own bound on a file it reads. Bigger than this, the content is dropped and the path
#: is kept: an oversized blob is an unknown, and an unknown is narrower than a guess.
MAX_BLOB_BYTES = 512 * 1024

_TOPIC_DOC = """\
what the card answers

  what changed after noise suppression, where the code landed, what the tree now depends on,
  which non-code surfaces moved and who has to act, what is tested, and how far the change
  reaches — every figure a button into the view that proves it, byte-deterministic for a given
  episode, and complete (with its gaps named) when an analysis is missing.
"""


def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom card", parents=[common],
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description="Episode summary card (reco-4ey): what changed, where it landed, what it "
                    "depends on, what surface it touches, what is tested, and the blast radius.",
        epilog=_TOPIC_DOC)
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("card", parents=[common], help="the whole card, audited")
    _episode_flags(p)
    p.add_argument("--brief", action="store_true",
                   help="the one-screen summary block only, without the figure detail")
    p.add_argument("--all", action="store_true",
                   help="ignore card.max_figures and print every figure")
    p.add_argument("--no-evidence", action="store_true",
                   help="print each figure's links but not the evidence rows")
    p.add_argument("--no-links", action="store_true",
                   help="print figures only (the shape a rail row would take)")
    p.add_argument("--no-audit", action="store_true",
                   help="skip the render audit (debugging: an unaudited card can lie)")
    p.add_argument("--policy", action="store_true", help="print the policy block under the card")
    p.add_argument("--definition", action="store_true",
                   help="print the decision procedure under the card")
    p.add_argument("--cols", type=int, default=render.DEFAULT_COLS)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("summary", parents=[common], help="the one-screen block, nothing else")
    _episode_flags(p)
    p.add_argument("--cols", type=int, default=render.DEFAULT_COLS)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("figures", parents=[common], help="the figures behind one or all sections")
    _episode_flags(p)
    p.add_argument("--section", default="", help="one section id (default: all)")
    p.add_argument("--all", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("figure", parents=[common],
                       help="one figure with every evidence row behind it")
    _episode_flags(p)
    p.add_argument("--section", required=True)
    p.add_argument("--figure", required=True)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("blast", parents=[common], help="the blast-radius drill-down")
    _episode_flags(p)
    p.add_argument("--all", action="store_true", help="ignore blast.max_rows")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("attention", parents=[common],
                       help="every finding raised, with the analysis that raised it")
    _episode_flags(p)
    p.add_argument("--all", action="store_true", help="include rows below the attention floor")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("verify", parents=[common],
                       help="re-check the card against its analyses; exit non-zero on a problem")
    _episode_flags(p)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("digest", parents=[common],
                       help="just the card digest (the byte-determinism handle)")
    _episode_flags(p)
    p.add_argument("--json", action="store_true")

    sub.add_parser("sections", parents=[common], help="the seven questions and who answers each")
    sub.add_parser("views", parents=[common],
                   help="the evidence views a figure can link to, and whether each exists")

    p = sub.add_parser("policy", parents=[common],
                       help="every switch and every number, with the layer that won")
    p.add_argument("--json", action="store_true")
    p.add_argument("--digest-only", action="store_true",
                   help="print the config digest the card was built under")

    p = sub.add_parser("definition", parents=[common],
                       help="the decision procedure, with the numbers actually in force")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("score", parents=[common],
                       help="the CI gates: states, claims, links, audits, degradation, cost")
    p.add_argument("--min-states", type=float, default=1.0)
    p.add_argument("--min-claims", type=float, default=1.0)
    p.add_argument("--min-links", type=float, default=1.0)
    p.add_argument("--min-verify", type=float, default=1.0)
    p.add_argument("--min-audits", type=float, default=1.0)
    p.add_argument("--min-tampered", type=float, default=1.0)
    p.add_argument("--min-degradation", type=float, default=1.0)
    p.add_argument("--min-blast", type=float, default=1.0)
    p.add_argument("--budget-ms", type=float, default=None)
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("bench", parents=[common],
                       help="compose + render cost against the published card budget")
    p.add_argument("--repeats", type=int, default=20)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("demo", parents=[common], help="the card the docs print")
    p.add_argument("--fixture", choices=list(CORPUS.NAMES), default=X.DEMO_CASE)
    p.add_argument("--capped", action="store_true",
                   help="honour card.max_figures so the remainder line is visible")
    p.add_argument("--json", action="store_true")
    return ap


def _episode_flags(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--manifest", type=Path, default=None,
                        help="JSON episode (see the module help); '-' means stdin")
    parser.add_argument("--old-repo", type=Path, default=None,
                        help="the before tree; --repo names the after tree and the repository "
                             "whose fathom.toml is read")
    parser.add_argument("--revision", default="", help="episode id to label the card with")
    parser.add_argument("--title", default="", help="episode title for the card header")
    parser.add_argument("--fixture", choices=list(CORPUS.NAMES), default=None,
                        help="use a labelled corpus episode instead of a real one")
    parser.add_argument("--no-deps", action="store_true",
                        help="pretend no dependency delta was supplied (the degraded path)")
    parser.add_argument("--no-graph", action="store_true",
                        help="pretend no module graph was supplied (the heuristic path)")
    parser.add_argument("--no-risk", action="store_true",
                        help="pretend no risk score was supplied (`e4.4` has not landed)")


# ---------------------------------------------------------------------------
# config, policy, episode input
# ---------------------------------------------------------------------------
def _config(args):
    env = {} if getattr(args, "no_env", False) else None
    return load_config(getattr(args, "repo", None), repo_file=getattr(args, "repo_file", None),
                       user_file=getattr(args, "user_file", None), env=env,
                       require_repo=False, sets=getattr(args, "set", []),
                       extends=getattr(args, "extend", []),
                       offline=getattr(args, "offline", False))


def _policy(args, config=None) -> Policy:
    return Policy.from_config(config if config is not None else _config(args))


def _read_tree(root: Path | None) -> dict[str, str | None]:
    """One working tree as `{path: content}`, with unreadable blobs kept as `None`."""
    if root is None:
        return {}
    out: dict[str, str | None] = {}
    for path in sorted(Path(root).rglob("*")):
        if not path.is_file() or ".git" in path.parts:
            continue
        relative = path.relative_to(root).as_posix()
        try:
            raw = path.read_bytes()[:MAX_BLOB_BYTES]
        except OSError:
            out[relative] = None
            continue
        out[relative] = None if b"\0" in raw else raw.decode("utf-8", "replace")
    return out


def _episode_and_analyses(args) -> tuple[Episode, Analyses, Policy]:
    policy = _policy(args)
    if getattr(args, "fixture", None):
        case = CORPUS.CASE_INDEX[args.fixture]
        return case.episode, _strip(case.analyses, args), policy
    if getattr(args, "manifest", None) is not None:
        episode, analyses = _from_manifest(args.manifest)
        return episode, _strip(analyses, args), policy
    root = getattr(args, "repo", None)
    if root is not None or args.old_repo is not None:
        after = _read_tree(Path(root) if root is not None else None)
        before = _read_tree(args.old_repo)
        if not before and not after:
            raise SystemExit("neither --repo nor --old-repo contained any files")
        episode = Episode.from_tree_pair(args.revision or "working-tree", before, after,
                                         title=args.title)
        return episode, _strip(Analyses(), args), policy
    raise SystemExit("`fathom card %s` needs --manifest, --repo/--old-repo, or --fixture"
                     % getattr(args, "command", "card"))


def _strip(analyses: Analyses, args) -> Analyses:
    """The `--no-*` flags: force the degraded paths the ticket cares about.

    The corpus fixtures are shared objects, so this copies before it subtracts — a verb that
    emptied the fixture's own `Analyses` would make the next command in the same process lie.
    """
    out = replace(analyses, absent=dict(analyses.absent))
    if getattr(args, "no_deps", False):
        out.deps = None
        out.absent.setdefault(
            "dependencies", Absent("dependencies", "no dependency delta was supplied "
                                                   "(`--no-deps`)", run="fathom analyze"))
    if getattr(args, "no_graph", False):
        out.graph = None
    if getattr(args, "no_risk", False):
        out.risk = None
    return out


def _from_manifest(path: Path) -> tuple[Episode, Analyses]:
    """The JSON shape in the module docstring, made into an episode and its analyses. Strict."""
    text = sys.stdin.read() if str(path) == "-" else Path(path).read_text("utf-8", "replace")
    payload = json.loads(text)
    if not isinstance(payload, dict):
        raise ValueError("the manifest must be a JSON object")
    files = []
    for item in payload.get("changes", []):
        change = str(item.get("change") or item.get("change_type") or "")
        files.append(Change(path=str(item.get("path", "")), before=item.get("before"),
                            after=item.get("after"),
                            change_type=change or _guess_type(item),
                            old_path=str(item.get("old_path", ""))))
    episode = Episode(id=str(payload.get("id") or "episode"), files=tuple(files),
                      title=str(payload.get("title") or ""),
                      before_paths=tuple(str(item) for item in payload.get("before_paths", [])),
                      after_paths=tuple(str(item) for item in payload.get("after_paths", [])),
                      author=str(payload.get("author") or ""),
                      commit_count=int(payload.get("commits") or 0))
    analyses = Analyses(source="manifest")
    deps = payload.get("deps")
    if isinstance(deps, dict):
        analyses.deps = DepsDelta(
            headline=str(deps.get("headline") or ""),
            ecosystems=tuple(str(item) for item in deps.get("ecosystems", ())),
            basis=str(deps.get("basis") or "resolved graph diff"),
            source=str(deps.get("source") or "manifest"),
            evidence=tuple(str(item) for item in deps.get("evidence", ())),
            **{key: _maybe_int(deps.get(key)) for key in
               ("direct_added", "direct_removed", "upgraded", "downgraded", "transitive_only",
                "source_switched", "integrity_changed", "licence_changed", "conflicts",
                "unparseable")})
    graph = payload.get("graph")
    if isinstance(graph, dict):
        analyses.graph = ModuleGraph(
            nodes=tuple(str(item) for item in graph.get("nodes", ())),
            edges=tuple((str(pair[0]), str(pair[1])) for pair in graph.get("edges", ())
                        if isinstance(pair, (list, tuple)) and len(pair) == 2),
            basis=str(graph.get("basis") or "resolved imports"),
            revision=str(graph.get("revision") or "after"), source="manifest",
            partial=tuple(str(item) for item in graph.get("partial", ())))
    risk = payload.get("risk")
    if isinstance(risk, dict):
        analyses.risk = RiskScore(score=float(risk.get("score") or 0.0),
                                  rank=int(risk.get("rank") or 0), of=int(risk.get("of") or 0),
                                  reasons=tuple(str(item) for item in risk.get("reasons", ())),
                                  blind_spots=tuple(str(item)
                                                    for item in risk.get("blind_spots", ())),
                                  source=str(risk.get("source") or "manifest"))
    for key, value in (payload.get("absent") or {}).items():
        analyses.absent[str(key)] = Absent(str(key), str(value))
    return episode, analyses


def _guess_type(item: dict) -> str:
    if item.get("before") is None:
        return "added"
    if item.get("after") is None:
        return "deleted"
    return "modified"


def _maybe_int(value):
    return None if value is None else int(value)


def _card(args) -> tuple:
    episode, analyses, policy = _episode_and_analyses(args)
    return compose(episode, analyses, policy), policy


# ---------------------------------------------------------------------------
# verbs
# ---------------------------------------------------------------------------
def cmd_card(args) -> int:
    card, _ = _card(args)
    if args.json:
        print(json.dumps(card.as_dict(), indent=2, sort_keys=True, default=str))
        return 0
    lines = render.render(card, cols=args.cols, brief=args.brief,
                          max_figures=None if not args.all else 10 ** 6,
                          show_evidence=not args.no_evidence,
                          show_links=not args.no_links)
    problems = [] if args.no_audit else render.audit(lines, card=card, brief=args.brief,
                                                     max_figures=None if not args.all
                                                     else 10 ** 6).problems
    for line in lines:
        print(line)
    if args.definition:
        for line in X.explain_definition(card.policy):
            print(line)
    if args.policy:
        for line in X.explain_policy(card.policy):
            print(line)
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_summary(args) -> int:
    card, _ = _card(args)
    if args.json:
        print(json.dumps({"episode": card.episode_id, "digest": card.digest(),
                          "states": card.states(), "headline": card.headline(),
                          "sections": [section.as_dict() for section in card.sections]},
                         indent=2, sort_keys=True, default=str))
        return 0
    for line in render.render(card, cols=args.cols, brief=True):
        print(line)
    return 0


def cmd_figures(args) -> int:
    card, _ = _card(args)
    wanted = [args.section] if args.section else [section.id for section in card.sections]
    if args.json:
        print(json.dumps({section_id: [figure.as_dict()
                                       for section in card.sections if section.id == section_id
                                       for figure in section.figures]
                          for section_id in wanted}, indent=2, sort_keys=True, default=str))
        return 0
    cap = None if args.all else card.policy.integer("max_figures")
    for section_id in wanted:
        section = card.section(section_id)
        if section is None:
            print("%s: not on this card" % section_id)
            continue
        print("%s (%s)" % (section_id, section.state))
        rows = section.figures if cap is None else section.figures[:cap]
        for figure in rows:
            print("  %s%s" % (_pad(figure.label), figure.value))
            for item in figure.links:
                print("      → %s" % item.render())
        if cap is not None and len(section.figures) > cap:
            print("  … %d more: fathom card figures <episode> --section %s --all"
                  % (len(section.figures) - cap, section_id))
    return 0


def cmd_figure(args) -> int:
    card, _ = _card(args)
    lines = X.explain_figure(card, args.section, args.figure)
    for line in lines:
        print(line)
    # A typo is an error. Printing "no figure …" and exiting 0 would let a script that asks for
    # the wrong thing report success, which is how a check stops being a check.
    joined = " ".join(lines)
    if joined.startswith("no section") or joined.startswith("no figure") \
            or "RESOLVES TO NOTHING" in joined:
        return EXIT_CODES["ERROR"]
    return 0


def cmd_blast(args) -> int:
    card, _ = _card(args)
    if card.blast is None:
        print("this card has no blast section (card.sections.blast is off)")
        return 0
    if args.json:
        print(json.dumps(card.blast.as_dict(), indent=2, sort_keys=True))
        return 0
    print("blast radius · %s" % card.blast.headline)
    rows = card.blast.lines()
    if args.all:
        rows = ["    %s" % item.render() for item in card.blast.direct] + \
               ["    %s" % item.render() for item in card.blast.one_hop] + \
               [line for line in card.blast.lines() if line.strip().startswith(("note",
                                                                                 "partial"))]
    for line in rows:
        print(line)
    return 0


def cmd_attention(args) -> int:
    card, _ = _card(args)
    for line in X.explain_attention(card, show_all=args.all):
        print(line)
    return 0


def cmd_verify(args) -> int:
    card, _ = _card(args)
    problems = card.verify()
    if args.json:
        print(json.dumps({"episode": card.episode_id, "digest": card.digest(),
                          "problems": problems, "links": len(card.links()),
                          "figures": len(card.figures())}, indent=2, sort_keys=True))
        return EXIT_CODES["ERROR"] if problems else 0
    print("card %s digest %s: %d figure(s), %d link(s)"
          % (card.episode_id, card.digest(), len(card.figures()), len(card.links())))
    if problems:
        for problem in problems:
            print("  problem: %s" % problem)
        return EXIT_CODES["ERROR"]
    print("  every link resolves, every figure's evidence says its number, and the second-path "
          "re-derivation agrees")
    return 0


def cmd_digest(args) -> int:
    card, _ = _card(args)
    if args.json:
        print(json.dumps({"episode": card.episode_id, "digest": card.digest(),
                          "policy_digest": card.policy.config_digest}, indent=2, sort_keys=True))
        return 0
    print(card.digest())
    return 0


def cmd_sections(args) -> int:
    for line in X.explain_sections():
        print(line)
    return 0


def cmd_views(args) -> int:
    for line in X.explain_views():
        print(line)
    return 0


def cmd_policy(args) -> int:
    policy = _policy(args)
    if args.digest_only:
        print(policy.config_digest or "(no config loaded: no digest)")
        return 0
    if args.json:
        print(json.dumps(policy.as_dict(), indent=2, sort_keys=True))
        return 0
    for line in X.explain_policy(policy):
        print(line)
    return 0


def cmd_definition(args) -> int:
    policy = _policy(args)
    if args.json:
        print(json.dumps({"knobs": policy.as_dict()["knobs"],
                          "sections": policy.as_dict()["sections"]}, indent=2, sort_keys=True))
        return 0
    for line in X.explain_definition(policy):
        print(line)
    return 0


def cmd_score(args) -> int:
    report = EVAL.score(policy=Policy.built_in(), budget_ms=args.budget_ms,
                        repeats=args.repeats)
    problems = report.gates(states=args.min_states, claims=args.min_claims, links=args.min_links,
                            verify=args.min_verify, audits=args.min_audits,
                            tampered=args.min_tampered, degradation=args.min_degradation,
                            blast=args.min_blast, budget_ms=args.budget_ms)
    if args.json:
        payload = report.as_dict()
        payload["gates"] = problems
        print(json.dumps(payload, indent=2, sort_keys=True))
        return EXIT_CODES["CHECK_VIOLATIONS"] if problems else 0
    print(report.text(verbose=args.verbose))
    if problems:
        print("FAIL")
        for problem in problems:
            print("  %s" % problem)
        return EXIT_CODES["CHECK_VIOLATIONS"]
    return 0


def cmd_bench(args) -> int:
    result = EVAL.bench(repeats=args.repeats)
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
        return EXIT_CODES["BUDGET_EXCEEDED"] \
            if result["p95_ms"] > result["budget_ms"] else 0
    print("card bench (%d repeats × %d episodes): p95 %.3f ms against a %.0f ms budget (%s), "
          "median %.3f ms" % (result["repeats"], result["episodes"], result["p95_ms"],
                              result["budget_ms"], result["budget_id"], result["median_ms"]))
    if result["p95_ms"] > result["budget_ms"]:
        print("over budget", file=sys.stderr)
        return EXIT_CODES["BUDGET_EXCEEDED"]
    return 0


def cmd_demo(args) -> int:
    case = CORPUS.CASE_INDEX[args.fixture]
    card = case.card()
    if args.json:
        print(json.dumps(card.as_dict(), indent=2, sort_keys=True, default=str))
        return 0
    cap = card.policy.integer("max_figures") if args.capped else 10 ** 6
    lines = render.render(card, max_figures=cap)
    problems = render.audit(lines, card=card, max_figures=cap).problems
    for line in lines:
        print(line)
    if args.capped:
        print("    note: the demo honours card.max_figures = %s here; the uncapped card is "
              "`fathom card demo`" % card.policy.integer("max_figures"))
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


HANDLERS = {
    "card": cmd_card, "summary": cmd_summary, "figures": cmd_figures, "figure": cmd_figure,
    "blast": cmd_blast, "attention": cmd_attention, "verify": cmd_verify, "digest": cmd_digest,
    "sections": cmd_sections, "views": cmd_views, "policy": cmd_policy,
    "definition": cmd_definition, "score": cmd_score, "bench": cmd_bench, "demo": cmd_demo,
}


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    handler = HANDLERS[args.command]
    try:
        return handler(args)
    except (ValueError, FileNotFoundError, json.JSONDecodeError) as error:
        print("fathom card: %s" % error, file=sys.stderr)
        return EXIT_CODES["ERROR"]
    except KeyError as error:
        print("fathom card: unknown %s" % error, file=sys.stderr)
        return EXIT_CODES["USAGE"]


def _pad(label: str, width: int = 24) -> str:
    return "%-*s " % (width, label) if len(label) <= width else "%s " % label


if __name__ == "__main__":
    sys.exit(main())
