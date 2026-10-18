"""`fathom structure …` — the verb surface of `reco-5fn`, and the only IO in the package.

The Rust product exposes this from `fathom-cli` (verbs parse, build a context, dispatch,
return an exit code — ADR-002 §4.3, §7.3); the executable forms here are:

```
python3 -m recommender.structure card --manifest episode.json      # the whole card
python3 -m recommender.structure summary --repo after --old-repo before
python3 -m recommender.structure dirs --manifest episode.json      # the directory table
python3 -m recommender.structure stories --manifest episode.json   # the structural stories
python3 -m recommender.structure findings --manifest episode.json  # one finding per line
python3 -m recommender.structure explain --manifest episode.json   # the sentence, in prose
python3 -m recommender.structure shapes                            # the shape vocabulary
python3 -m recommender.structure policy                            # switches + winning layer
python3 -m recommender.structure definition                        # the rules, as configured
python3 -m recommender.structure score                             # the CI gate
python3 -m recommender.structure separation                        # AC 3's two numbers
python3 -m recommender.structure property --seeds 40               # AC 2, on generated episodes
python3 -m recommender.structure demo                              # the docs' card
```

An episode comes in three ways, in increasing order of fuss.

`--repo AFTER --old-repo BEFORE` reads two working trees and diffs them; this is the shape a
`structure.delta` caller has, and it is the only filesystem access anywhere in the package
(ADR-002 §4.5). `--repo` doubles as the repository whose `fathom.toml` is read, exactly as it
does in `fathom noncode`. Walking the trees also supplies the *inventories*, which is what makes
the directory census mean "this folder has 14 files" rather than "this episode touched 14 files
here" — the card prints which of the two it is.

`--manifest FILE` (or `-` for stdin) carries the episode as JSON, the shape `e2.11` hands to
consumers. Pairings and inventories are optional, and their absence is reported rather than
papered over:

```json
{"id": "ep-1", "title": "move the payment package under billing",
 "changes": [{"path": "pkg/billing/a.py", "change": "added", "after": "…"},
              {"path": "pkg/pay/a.py", "change": "deleted", "before": "…"},
              {"path": "pkg/billing/b.py", "change": "renamed", "old_path": "pkg/pay/b.py",
               "before": "…", "after": "…"}],
 "moves": [{"old_path": "pkg/pay/c.py", "new_path": "pkg/billing/c.py",
            "confidence": 0.91, "source": "git", "tier": "fact", "lines": 40}],
 "before_paths": ["pkg/pay/a.py", "…"], "after_paths": ["pkg/billing/a.py", "…"]}
```

`moves` is what `e2.4` hands over: a pairing at or above `structure.move.fact_bar` counts as a
fact, and anything this package infers from bytes is labelled a guess however confident it
looks. `--fixture NAME` runs one of the labelled corpus episodes instead, which is how the
documented card is produced and how the shapes are demonstrated without a repository.

The flags for finding `fathom.toml` are `e0.8`'s verbatim, so `fathom structure policy` and
`fathom config explain` cannot disagree about which layer won.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from recommender.config.cli import common_flags
from recommender.config.errors import ConfigError, EXIT_CODES
from recommender.config.layer import load as load_config

from . import corpus as CORPUS
from . import explain as X
from . import render
from .delta import aggregate, measure
from .eval import (FUZZ_SEEDS, MIN_EPISODES, MIN_PRECISION, MIN_RECONCILE, MIN_SEPARATION,
                   MIN_SHAPES, MIN_SHAPES_COVERED, MIN_SUMMARIES, score, separation)
from .model import Change, Episode, Move
from .policy import Policy, RULE_INDEX

#: The CLI's own bound on a file it reads. Bigger than this, the content is dropped and the
#: path is kept: an oversized blob is an unknown, and an unknown is narrower than a guess.
MAX_BLOB_BYTES = 512 * 1024

_TOPIC_DOC = """\
what the card answers

  added / removed / moved lines, files created / deleted / moved, directories added and
  removed, whole-subtree moves and package relocations, the tree's depth / fanout /
  files-per-directory / top-3 concentration before and after, a structure-churn score, and
  one deterministic sentence that says where the episode landed — every clause of that
  sentence backed by evidence this module can point at.

  `dirs` prints the directory table, `verify` proves it sums to the episode totals, and the
  card prints the reconciliation either way: a table that does not reconcile is a bug, and the
  card says so rather than hiding it.
"""


def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom structure", parents=[common],
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description="Folder-structure delta and directory-level summary (reco-5fn): where "
                    "this episode's code landed.",
        epilog=_TOPIC_DOC)
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("card", parents=[common], help="the whole card, with the audit")
    _episode_flags(p)
    p.add_argument("--all", action="store_true",
                   help="print every directory row, ignoring structure.card.max_dirs")
    p.add_argument("--policy", action="store_true", help="print the policy block too")
    p.add_argument("--definition", action="store_true",
                   help="print the decision procedure under the card")
    p.add_argument("--no-audit", action="store_true",
                   help="skip the render audit (debugging: an unaudited card can lie)")
    p.add_argument("--cols", type=int, default=render.DEFAULT_COLS)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("summary", parents=[common],
                       help="the one-sentence answer (what a roll-up shows)")
    _episode_flags(p)
    p.add_argument("-v", "--verbose", action="store_true",
                   help="print every clause of the sentence with its evidence")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("dirs", parents=[common], help="the directory table on its own")
    _episode_flags(p)
    p.add_argument("--all", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("stories", parents=[common],
                       help="the structural stories: renames, subtree moves, splits, flattens")
    _episode_flags(p)
    p.add_argument("--all", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("findings", parents=[common],
                       help="just the findings for one episode, one per line")
    _episode_flags(p)
    p.add_argument("--kind", default="", help="filter to one finding kind")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("explain", parents=[common],
                       help="walk one episode in prose: the sentence, then the receipts")
    _episode_flags(p)
    p.add_argument("--directory", default=None, help="explain one directory instead")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("aggregate", parents=[common],
                       help="several episodes rolled up into one directory table")
    p.add_argument("--manifest", type=Path, required=True,
                   help="JSON list of episode manifests (see the module help)")
    p.add_argument("--json", action="store_true")

    sub.add_parser("shapes", parents=[common],
                   help="what each structural shape claims, with the numbers in force")

    p = sub.add_parser("policy", parents=[common],
                       help="every switch and every number, with the layer that won")
    p.add_argument("--json", action="store_true")
    p.add_argument("--digest-only", action="store_true",
                   help="print the config digest the payload was measured under")

    p = sub.add_parser("definition", parents=[common],
                       help="the decision procedure, with the numbers actually in force")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("score", parents=[common],
                       help="shape, sentence, trap, episode, reconciliation and separation gates")
    p.add_argument("--min-shapes", type=float, default=MIN_SHAPES)
    p.add_argument("--min-summaries", type=float, default=MIN_SUMMARIES)
    p.add_argument("--min-precision", type=float, default=MIN_PRECISION)
    p.add_argument("--min-episodes", type=float, default=MIN_EPISODES)
    p.add_argument("--min-reconcile", type=float, default=MIN_RECONCILE)
    p.add_argument("--min-separation", type=float, default=MIN_SEPARATION)
    p.add_argument("--min-shapes-covered", type=int, default=MIN_SHAPES_COVERED)
    p.add_argument("--fuzz-seeds", type=int, default=None,
                   help="generated episodes to reconcile (default: the CI run)")
    p.add_argument("--fast", action="store_true",
                   help="skip the generated episodes (the four shapes and the traps only)")
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("separation", parents=[common],
                       help="AC 3: the same twenty files, changed in place versus reshuffled")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("property", parents=[common],
                       help="AC 2 on generated episodes: does the table reconcile?")
    p.add_argument("--seeds", type=int, default=40)
    p.add_argument("--start", type=int, default=0)

    p = sub.add_parser("demo", parents=[common], help="the labelled card the docs print")
    p.add_argument("--fixture", choices=list(CORPUS.NAMES), default="subtree_move")
    p.add_argument("--json", action="store_true")
    return ap


def _episode_flags(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--manifest", type=Path, default=None,
                        help="JSON episode (see the module help); '-' means stdin")
    parser.add_argument("--old-repo", type=Path, default=None,
                        help="the before tree, read the way --repo is (the common --repo flag "
                             "names the after tree, and the repository whose fathom.toml is read)")
    parser.add_argument("--revision", default="", help="episode id to label the card with")
    parser.add_argument("--title", default="", help="episode title for the card header")
    parser.add_argument("--fixture", choices=list(CORPUS.NAMES), default=None,
                        help="use a labelled corpus episode instead of a real one")


# ---------------------------------------------------------------------------
# config and policy
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


# ---------------------------------------------------------------------------
# episode input
# ---------------------------------------------------------------------------
def _read_tree(root: Path | None) -> dict[str, str | None]:
    """One working tree as `{path: content}`, with unreadable blobs kept as `None`.

    A `None` value is the honest index-only entry: the path is known, the bytes are not, so the
    episode can still count the file and report the move it was told about, while anything that
    would need the bytes says it could not see them.
    """
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


def _episode(args, config=None) -> tuple[Episode, Policy]:
    """Resolve `--fixture` / `--manifest` / `--repo`+`--old-repo` into one episode and one policy."""
    policy = _policy(args, config)
    if getattr(args, "fixture", None):
        return CORPUS.fixture(args.fixture), policy
    if getattr(args, "manifest", None) is not None:
        return _episode_from_manifest(args.manifest), policy
    root = getattr(args, "repo", None)
    if root is not None or args.old_repo is not None:
        after = _read_tree(Path(root) if root is not None else None)
        before = _read_tree(args.old_repo)
        if not before and not after:
            raise SystemExit("neither --repo nor --old-repo contained any files")
        return Episode.from_tree_pair(args.revision or "working-tree", before, after,
                                      title=args.title), policy
    raise SystemExit("`fathom structure %s` needs --manifest, --repo/--old-repo, or --fixture"
                     % getattr(args, "command", "card"))


def _episode_from_manifest(path: Path) -> Episode:
    """The JSON shape in the module docstring, made into an `Episode`. Strict, and loud."""
    text = sys.stdin.read() if str(path) == "-" else Path(path).read_text("utf-8", "replace")
    payload = json.loads(text)
    if not isinstance(payload, dict):
        raise ValueError("the manifest must be a JSON object")
    files: list[Change] = []
    for item in payload.get("changes", []):
        change = str(item.get("change") or item.get("change_type") or "modified")
        files.append(Change(path=str(item.get("path", "")),
                            before=item.get("before"), after=item.get("after"),
                            change_type=change, old_path=str(item.get("old_path", "")),
                            lines_before=int(item.get("lines_before") or 0),
                            lines_after=int(item.get("lines_after") or 0)))
    moves = [Move(old_path=str(item.get("old_path", "")), new_path=str(item.get("new_path", "")),
                  confidence=float(item.get("confidence", 1.0)),
                  source=str(item.get("source", "caller")), tier=str(item.get("tier", "guess")),
                  lines=int(item.get("lines") or 0))
             for item in payload.get("moves", [])]
    return Episode(id=str(payload.get("id") or "episode"), files=tuple(files),
                   title=str(payload.get("title") or ""), moves=tuple(moves),
                   before_paths=tuple(str(item) for item in payload.get("before_paths", [])),
                   after_paths=tuple(str(item) for item in payload.get("after_paths", [])))


# ---------------------------------------------------------------------------
# verbs
# ---------------------------------------------------------------------------
def cmd_card(args) -> int:
    episode, policy = _episode(args)
    view = measure(episode, policy)
    if args.json:
        print(json.dumps(view.as_dict(), indent=2, sort_keys=True))
        return 0
    lines = render.render(view, cols=args.cols, show_all=args.all)
    problems = [] if args.no_audit else render.audit(lines, view=view,
                                                     show_all=args.all).problems
    for line in lines:
        print(line)
    if args.definition:
        for line in X.explain_definition(policy):
            print(line)
    if args.policy:
        for line in X.explain_policy(policy):
            print(line)
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_summary(args) -> int:
    episode, policy = _episode(args)
    view = measure(episode, policy)
    summary = view.summary()
    if args.json:
        print(json.dumps(summary, indent=2, sort_keys=True))
        return 0
    print(summary["sentence"])
    if args.verbose:
        for clause in summary["clauses"]:
            print("  · %s" % clause["text"])
            for evidence in clause["evidence"]:
                print("      — %s" % evidence)
        for note in view.notes:
            print("  note: %s" % note)
    return 0


def cmd_dirs(args) -> int:
    episode, policy = _episode(args)
    view = measure(episode, policy)
    if args.json:
        print(json.dumps({"totals": view.totals(), "row_sums": view.row_sums(),
                          "reconciliation": view.verify(),
                          "directories": [row.as_dict() for row in view.rows]},
                         indent=2, sort_keys=True))
        return 0
    lines = render.render(view, show_all=args.all, header=False)
    for line in lines:
        print(line)
    return 0


def cmd_stories(args) -> int:
    episode, policy = _episode(args)
    view = measure(episode, policy)
    if args.json:
        print(json.dumps({"stories": [story.as_dict() for story in view.stories]},
                         indent=2, sort_keys=True))
        return 0
    if not view.stories:
        print("no structural story: every file stayed in the directory it was in, or the "
              "changes were too few and too local to group")
        return 0
    for story in view.stories:
        print("%-17s %s" % (story.shape, story.summary_clause()))
        if args.all:
            for line in story.evidence():
                print("    — %s" % line)
    return 0


def cmd_findings(args) -> int:
    episode, policy = _episode(args)
    view = measure(episode, policy)
    findings = [finding for finding in view.findings
                if not args.kind or finding.kind == args.kind]
    if args.json:
        print(json.dumps({"findings": [finding.as_dict() for finding in findings],
                          "suppressed": view.counts()["findings_suppressed"]},
                         indent=2, sort_keys=True))
        return 0
    for finding in findings:
        print(finding.render())
    if not findings:
        print("no findings")
    for kind, count in sorted(view.suppressed.items()):
        print("%d %s finding(s) not reported: %s is off (%s)"
              % (count, kind, RULE_INDEX[kind].config_key,
                 view.suppressed_source.get(kind, "policy")))
    return 0


def cmd_explain(args) -> int:
    episode, policy = _episode(args)
    if args.directory:
        lines = X.explain_directory(episode, args.directory, policy)
    else:
        lines = X.explain_episode(episode, policy)
    if args.json:
        print(json.dumps({"lines": lines}, indent=2))
        return 0
    for line in lines:
        print(line)
    return 0


def cmd_aggregate(args) -> int:
    payload = json.loads(Path(args.manifest).read_text("utf-8", "replace"))
    if not isinstance(payload, list):
        raise ValueError("`aggregate` takes a JSON *list* of episode manifests")
    episodes = [_manifest_from_dict(item) for item in payload]
    rolled = aggregate(episodes, policy=_policy(args))
    if args.json:
        print(json.dumps(rolled, indent=2, sort_keys=True))
        return 0
    print("aggregate over %d episode(s) — %s" % (rolled["episodes"], rolled["basis"]))
    print("  %s file(s) moved, %s line(s) travelled, %s director%s added, %s removed"
          % (rolled["files_moved"], rolled["lines_moved"], rolled["directories_added"],
             "y" if rolled["directories_added"] == 1 else "", rolled["directories_removed"]))
    print("  mean structure churn %.3f (%s of %d episode(s) structural)"
          % (rolled["churn_mean"], rolled["churn_structural"], rolled["episodes"]))
    for row in rolled["directories"][:20]:
        print("  %-38s +%-5d −%-5d moved in %-4d moved out %-4d"
              % (row["directory"], row["lines_added"], row["lines_removed"],
                 row["lines_moved_in"], row["lines_moved_out"]))
    return 0


def cmd_shapes(args) -> int:
    for line in X.explain_shapes():
        print(line)
    return 0


def cmd_policy(args) -> int:
    policy = _policy(args)
    if getattr(args, "digest_only", False):
        print(policy.config_digest or "(no config loaded: the built-in defaults have no digest)")
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
        print(json.dumps({"definition": X.explain_definition(policy),
                          "knobs": policy.as_dict()["knobs"]}, indent=2))
        return 0
    for line in X.explain_definition(policy):
        print(line)
    return 0


def cmd_score(args) -> int:
    seeds = 0 if args.fast else (FUZZ_SEEDS if args.fuzz_seeds is None else args.fuzz_seeds)
    report = score(policy=_policy(args), fuzz_seeds=seeds, verbose=args.verbose)
    if args.json:
        print(json.dumps(report.as_dict(), indent=2, sort_keys=True))
    else:
        print(report.text(verbose=args.verbose))
    problems = report.gates(shapes=args.min_shapes, summaries=args.min_summaries,
                            precision=args.min_precision, episodes=args.min_episodes,
                            reconcile=args.min_reconcile, separation=args.min_separation,
                            shapes_covered=args.min_shapes_covered)
    if problems:
        print("structure score below bar: %s" % "; ".join(problems), file=sys.stderr)
        return EXIT_CODES["ERROR"]
    return 0


def cmd_separation(args) -> int:
    numbers = separation(_policy(args))
    if args.json:
        print(json.dumps(numbers, indent=2, sort_keys=True))
        return 0
    print("20 files changed where they were:      churn %.3f (%s)"
          % (numbers["local"], numbers["local_band"]))
    print("20 files moved across the tree:       churn %.3f (%s)"
          % (numbers["reshuffle"], numbers["reshuffle_band"]))
    print("gap %.3f — the same twenty files, told apart by where they landed, not by how many"
          % numbers["gap"])
    return 0


def cmd_property(args) -> int:
    """AC 2 on generated episodes: one line each, and a non-zero exit if any row fails."""
    policy = _policy(args)
    failures = 0
    for seed in range(args.start, args.start + args.seeds):
        view = measure(CORPUS.fuzz_episode(seed), policy)
        problems = view.verify()
        failures += bool(problems)
        totals = view.totals()
        print("seed %5d %-52s rows=%-3d files=%-3d +%-5d −%-5d moved=%-3d churn=%.3f %s"
              % (seed, CORPUS.fuzz_plan_text(seed)[:52], len(view.rows), totals["files"],
                 totals["lines_added"], totals["lines_removed"], totals["files_moved"],
                 view.churn.score,
                 "reconciles" if not problems else "FAILS: " + "; ".join(problems)))
    print("%d/%d generated episode(s) reconcile their directory table to their own totals"
          % (args.seeds - failures, args.seeds))
    return EXIT_CODES["ERROR"] if failures else 0


def cmd_demo(args) -> int:
    view = measure(CORPUS.fixture(args.fixture), Policy.built_in())
    if args.json:
        print(json.dumps(view.as_dict(), indent=2, sort_keys=True))
        return 0
    lines = render.render(view, show_all=True)
    for line in lines:
        print(line)
    problems = render.audit(lines, view=view, show_all=True).problems
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


def _manifest_from_dict(payload: dict) -> Episode:
    """`_episode_from_manifest` for a dict already in hand (the aggregate verb)."""
    files = tuple(Change(path=str(item.get("path", "")), before=item.get("before"),
                         after=item.get("after"),
                         change_type=str(item.get("change") or "modified"),
                         old_path=str(item.get("old_path", "")))
                  for item in payload.get("changes", []))
    moves = tuple(Move(old_path=str(item.get("old_path", "")),
                       new_path=str(item.get("new_path", "")),
                       confidence=float(item.get("confidence", 1.0)),
                       source=str(item.get("source", "caller")),
                       tier=str(item.get("tier", "guess")))
                  for item in payload.get("moves", []))
    return Episode(id=str(payload.get("id") or "episode"), files=files,
                   title=str(payload.get("title") or ""), moves=moves)


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        handlers = {"card": cmd_card, "summary": cmd_summary, "dirs": cmd_dirs,
                    "stories": cmd_stories, "findings": cmd_findings, "explain": cmd_explain,
                    "aggregate": cmd_aggregate, "shapes": cmd_shapes, "policy": cmd_policy,
                    "definition": cmd_definition, "score": cmd_score,
                    "separation": cmd_separation, "property": cmd_property, "demo": cmd_demo}
        return handlers[args.command](args)
    except ConfigError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return exc.exit_code
    except json.JSONDecodeError as exc:
        print("error: the manifest is not JSON (%s)" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except ValueError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except OSError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except SystemExit as exit_:                  # the input-source guard above
        if not exit_.args or not isinstance(exit_.args[0], str):
            raise
        print("error: %s" % exit_.args[0], file=sys.stderr)
        return EXIT_CODES["USAGE"]


from .eval import separation as SEPARATION      # noqa: E402  (after the handlers, for the verb)

if __name__ == "__main__":                                  # pragma: no cover
    sys.exit(main())
