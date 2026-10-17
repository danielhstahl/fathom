"""`fathom noncode …` — the verb surface of `e2.8`, and the only IO in the package.

The Rust product exposes this from `fathom-cli` (verbs parse, build a context, dispatch,
return an exit code — ADR-002 §4.3, §7.3); the executable forms here are:

```
python3 -m recommender.noncode card --manifest episode.json      # the per-episode card
python3 -m recommender.noncode card --repo after --old-repo before --path .env.example
python3 -m recommender.noncode checklist --manifest episode.json # AC 4's roll-up, on its own
python3 -m recommender.noncode findings --manifest episode.json  # one finding per line
python3 -m recommender.noncode explain .env.example --before a --after b
python3 -m recommender.noncode surfaces                          # the six-surface table
python3 -m recommender.noncode policy                            # switches + winning layer
python3 -m recommender.noncode definition                        # the rules, as configured
python3 -m recommender.noncode score                             # the CI gate
python3 -m recommender.noncode bench
python3 -m recommender.noncode demo                              # the docs' card
```

A pair is *two revisions of one path*, taken three ways in increasing order of fuss:
`--before/--after` name two files; `--repo` plus `--old-repo` read one path out of two trees;
`--manifest` carries the whole episode, inline or base64, which is the shape `e2.11` hands to
consumers:

```json
{"id": "ep-1", "title": "add orders and wire the release pipeline",
 "changes": [{"path": ".env.example", "change": "modified",
              "before": "…", "after": "…",
              "before_base64": "…", "after_base64": "…",
              "old_path": "db/migrate/old_name.py"}],
 "repo_env_refs": ["FEATURE_X"], "repo_flag_reads": ["instant_search"]}
```

`repo_env_refs` / `repo_flag_reads` are the repository-wide reference sets the episode cannot
see (see `non_code.env.reference_scope`): a caller with an index supplies them, and the card
says which claims they were able to strengthen.

The flags for finding `fathom.toml` are `e0.8`'s verbatim, so `fathom noncode policy` and
`fathom config explain` cannot disagree about which layer won.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import time
from pathlib import Path

from recommender.config.cli import common_flags
from recommender.config.errors import ConfigError, EXIT_CODES
from recommender.config.layer import load as load_config

from . import corpus as CORPUS
from . import explain as X
from . import render
from .checklist import audit as audit_checklist
from .episode import aggregate, measure
from .eval import (MIN_EPISODES, MIN_PRECISION, MIN_RULES, MIN_SURFACES, score)
from .policy import Policy
from .surfaces import Change, Episode

#: Same cap `e2.1` puts on a blob it will read: a 400 MB committed file is not parsed.
MAX_BLOB_BYTES = 4 * 1024 * 1024

_TOPIC_DOC = """\
verbs
  card          the per-episode card: surfaces, findings with locations, the checklist
  checklist     the operator-action roll-up on its own (AC 4)
  findings      the same episode, findings only, one per line
  explain       one file pair: what it is, which surfaces claim it, what each reader found
  surfaces      the six surfaces and the findings each one owns
  policy        every finding switch and every knob, with the winning layer
  definition    the decision procedure, spelled out with the numbers in force
  score         rule, precision, surface and episode gates on the labelled corpus (CI)
  bench         cost per episode and per card
  demo          one episode across all six surfaces (docs, and the smoke test)

e2.8 asks one question per episode: what changed that is not code, and who has to do
something about it before this ships. Environment variables and their references, feature
flags and their defaults, migrations and whether they can be undone, CI secrets,
permissions, identities, runners and triggers, infrastructure deltas, and base images. Every
finding carries an action and an owner, because the artefact that matters is the checklist.

configuration (`[non_code]`)
  non_code.findings.<kind>          one switch per finding kind — see `policy`
  non_code.env.*                    declaration globs, secret names, the reference scope
  non_code.flags.* / non_code.migration.* / non_code.ci.* / non_code.iac.*
  non_code.container.require_digest_pin
  non_code.card.max_actions         checklist rows per card; the remainder is counted
  `fathom noncode policy` prints every key with the layer that won it; the flags for
  finding `fathom.toml` are `e0.8`'s, so this verb and `fathom config explain` cannot
  disagree. See [docs/concepts/noncode-surfaces.md](docs/concepts/noncode-surfaces.md).
"""


def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom noncode", parents=[common],
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description="Non-code surface changes: config, env, flags, migrations and CI "
                    "(e2.8 / reco-bjd), rolled up into an operator-action checklist.",
        epilog=_TOPIC_DOC)
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("card", parents=[common],
                       help="the per-episode card: surfaces, findings, checklist")
    _pair_flags(p)
    p.add_argument("--all", action="store_true",
                   help="print every action row, ignoring non_code.card.max_actions")
    p.add_argument("--policy", action="store_true", help="print the policy block too")
    p.add_argument("--definition", action="store_true",
                   help="print the decision procedure under the card")
    p.add_argument("--no-checklist", action="store_true", help="findings only")
    p.add_argument("--cols", type=int, default=render.DEFAULT_COLS)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("checklist", parents=[common],
                       help="the per-episode 'requires operator action' checklist (AC 4)")
    _pair_flags(p)
    p.add_argument("--all", action="store_true")
    p.add_argument("--by-owner", action="store_true", help="group the rows by owner")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("findings", parents=[common],
                       help="just the findings for one episode, one per line")
    _pair_flags(p)
    p.add_argument("--surface", default="", help="filter to one surface")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("explain", parents=[common],
                       help="one file pair: what it is, and what each surface reader found")
    p.add_argument("path", nargs="+")
    p.add_argument("--old-repo", type=Path, default=None)
    p.add_argument("--before", type=Path, default=None)
    p.add_argument("--after", type=Path, default=None)
    p.add_argument("--json", action="store_true")

    sub.add_parser("surfaces", help="the six surfaces and the findings each owns")

    p = sub.add_parser("policy", parents=[common],
                       help="every switch and every number, with the layer that won")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("definition", parents=[common],
                       help="the decision procedure, with the numbers actually in force")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("score", parents=[common],
                       help="rule, precision, surface and episode gates on the corpus")
    p.add_argument("--min-rules", type=float, default=MIN_RULES)
    p.add_argument("--min-precision", type=float, default=MIN_PRECISION)
    p.add_argument("--min-episodes", type=float, default=MIN_EPISODES)
    p.add_argument("--min-surfaces", type=int, default=MIN_SURFACES)
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("bench", parents=[common], help="cost per episode and per card")
    p.add_argument("--repeat", type=int, default=4)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("demo", help="the six-surface demo card (docs and smoke test)")
    p.add_argument("--json", action="store_true")
    p.add_argument("--capped", action="store_true",
                   help="honour non_code.card.max_actions exactly as a real episode does; "
                        "the default prints every row because the demo exists to demonstrate "
                        "them")
    return ap


def _pair_flags(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--manifest", type=Path, default=None,
                        help="JSON change set (see the module help)")
    parser.add_argument("--path", dest="paths", nargs="*", default=[],
                        help="paths to read from --repo / --old-repo")
    parser.add_argument("--old-repo", type=Path, default=None)
    parser.add_argument("--title", default="", help="episode title for the card header")


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _config(args):
    env = {} if getattr(args, "no_env", False) else None
    return load_config(getattr(args, "repo", None), repo_file=getattr(args, "repo_file", None),
                       user_file=getattr(args, "user_file", None), env=env,
                       require_repo=False, sets=getattr(args, "set", []),
                       extends=getattr(args, "extend", []),
                       offline=getattr(args, "offline", False))


def _policy(args, config=None) -> Policy:
    config = config if config is not None else _config(args)
    return Policy.from_config(config)


def _read(root: Path | None, path: str) -> str | None:
    if root is None:
        return None
    candidate = Path(root) / path
    try:
        if not candidate.is_file():
            return None
        return candidate.read_bytes()[:MAX_BLOB_BYTES].decode("utf-8", "replace")
    except OSError:
        return None


def _read_file(path: Path | None) -> str | None:
    if path is None:
        return None
    try:
        return Path(path).read_text("utf-8", "replace")[:MAX_BLOB_BYTES]
    except OSError:
        return None


def _sides(args, path: str) -> tuple[str | None, str | None]:
    if getattr(args, "before", None) or getattr(args, "after", None):
        return _read_file(args.before), _read_file(args.after)
    if args.repo is None and args.old_repo is None:
        return None, None
    old_root = args.old_repo or args.repo
    return _read(old_root, path), _read(args.repo, path)


def _field(entry: dict, key: str) -> str | None:
    if entry.get(key) is not None:
        return str(entry[key])
    encoded = entry.get(key + "_base64")
    if encoded:
        return base64.b64decode(encoded).decode("utf-8", "replace")
    return None


def episode_from_manifest(payload: dict) -> Episode:
    files = []
    for entry in payload.get("changes", []):
        files.append(Change(path=entry.get("path", ""), old_path=entry.get("old_path", ""),
                            before=_field(entry, "before"), after=_field(entry, "after"),
                            change_type=entry.get("change",
                                                  entry.get("change_type", "modified"))))
    return Episode(id=payload.get("id", ""), files=tuple(files), title=payload.get("title", ""),
                   repo_env_refs=tuple(payload.get("repo_env_refs", ())),
                   repo_flag_reads=tuple(payload.get("repo_flag_reads", ())))


def episode_from_args(args) -> tuple[Episode, dict]:
    if getattr(args, "manifest", None):
        payload = json.loads(Path(args.manifest).read_text("utf-8"))
        return episode_from_manifest(payload), payload
    if getattr(args, "paths", None):
        files = [Change.from_pair(path, *_sides(args, path)) for path in args.paths]
        return Episode(id="", files=tuple(files), title=getattr(args, "title", "")), {}
    raise SystemExit("%s: pass --manifest or at least one --path" % args.command)


# ---------------------------------------------------------------------------
# the verbs
# ---------------------------------------------------------------------------
def cmd_card(args) -> int:
    episode, _payload = episode_from_args(args)
    view = measure(episode, _policy(args))
    if args.json:
        print(json.dumps(view.as_dict(), indent=2))
        return 0
    lines = render.render(view, cols=args.cols, show_all=args.all, header=True)
    for line in lines:
        print(line)
    problems = render.audit(lines, view=view, show_all=args.all).problems
    if args.no_checklist:
        problems = [item for item in problems if "operator" not in item]
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    if args.definition:
        print()
        for line in X.explain_definition(view.policy):
            print(line)
    if args.policy:
        print()
        for line in X.explain_policy(view.policy):
            print(line)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_checklist(args) -> int:
    episode, _payload = episode_from_args(args)
    view = measure(episode, _policy(args))
    checklist = view.checklist
    if args.json:
        print(json.dumps(checklist.as_dict(), indent=2))
        return 0
    cap = None if args.all else int(view.policy.number("max_actions"))
    if args.by_owner:
        lines = checklist.header()
        for owner, actions in checklist.by_owner().items():
            lines.append("  %s" % owner)
            for action in actions[:(cap or len(actions))]:
                lines.extend(action.block(indent=4))
    else:
        lines = checklist.lines(max_actions=cap)
    for line in lines:
        print(line)
    problems = audit_checklist(lines, checklist=checklist, max_actions=cap).problems
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_findings(args) -> int:
    episode, _payload = episode_from_args(args)
    view = measure(episode, _policy(args))
    findings = view.findings_for(args.surface) if args.surface else view.findings
    if args.json:
        print(json.dumps([finding.as_dict() for finding in findings], indent=2))
        return 0
    for finding in findings:
        print("%-28s %s" % (finding.kind, finding.render()))
    if not findings:
        print("no findings")
    return 0


def cmd_explain(args) -> int:
    policy = _policy(args)
    payload = []
    for path in args.path:
        before, after = _sides(args, path)
        if args.json:
            from .surfaces import Change, Episode, Run
            run = Run(Episode(id="", files=(Change.from_pair(path, before, after),)), policy)
            payload.append(run.files[0].as_dict())
            continue
        for line in X.explain_file(path, before, after, policy=policy):
            print(line)
        print()
    if args.json:
        print(json.dumps(payload if len(payload) > 1 else payload[0], indent=2))
    return 0


def cmd_surfaces(args) -> int:
    for line in X.explain_surfaces():
        print(line)
    return 0


def cmd_policy(args) -> int:
    policy = _policy(args)
    if args.json:
        print(json.dumps(policy.as_dict(), indent=2))
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
    report = score(policy=_policy(args))
    problems = report.gates(rules=args.min_rules, precision=args.min_precision,
                            episodes=args.min_episodes, surfaces=args.min_surfaces)
    if args.json:
        print(json.dumps({"report": report.as_dict(), "problems": problems,
                          "bars": {"rules": args.min_rules, "precision": args.min_precision,
                                   "episodes": args.min_episodes,
                                   "surfaces": args.min_surfaces}}, indent=2))
    else:
        print(report.text(verbose=args.verbose))
        if problems:
            print("FAIL")
            for problem in problems:
                print("  %s" % problem)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_bench(args) -> int:
    repeat = max(1, args.repeat)
    from .corpus import EPISODE_CASES
    started = time.perf_counter()
    for _ in range(repeat):
        for case in EPISODE_CASES:
            measure(Episode.from_pairs(case.name, case.files))
    episode_ms = (time.perf_counter() - started) * 1000 / max(1, repeat * len(EPISODE_CASES))

    started = time.perf_counter()
    for _ in range(repeat):
        demo = X.demo()
        render.render(demo)
    card_ms = (time.perf_counter() - started) * 1000 / max(1, repeat)

    totals = aggregate([Episode.from_pairs(case.name, case.files) for case in EPISODE_CASES])
    if args.json:
        print(json.dumps({"per_episode_ms": round(episode_ms, 3),
                          "per_card_ms": round(card_ms, 3),
                          "episodes": len(EPISODE_CASES), "repeat": repeat,
                          "aggregate": totals}, indent=2))
        return 0
    print("non-code bench (repeat %d): %.3f ms per episode, %.3f ms per card; %d findings "
          "over %d episodes" % (repeat, episode_ms, card_ms, totals["findings"],
                                totals["episodes"]))
    return 0


def cmd_demo(args) -> int:
    """The docs' card: one episode, six surfaces, one finding of every severity."""
    view = X.demo()
    if args.json:
        print(json.dumps(view.as_dict(), indent=2))
        return 0
    show_all = not args.capped
    lines = render.render(view, show_all=show_all)
    if show_all:
        lines.append("    note: the demo prints every action row; a real episode honours"
                     " non_code.card.max_actions = %s, which `--capped` shows"
                     % view.policy.number("max_actions"))
    for line in lines:
        print(line)
    problems = render.audit(lines, view=view, show_all=show_all).problems
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        handlers = {"card": cmd_card, "checklist": cmd_checklist, "findings": cmd_findings,
                    "explain": cmd_explain, "surfaces": cmd_surfaces, "policy": cmd_policy,
                    "definition": cmd_definition, "score": cmd_score, "bench": cmd_bench,
                    "demo": cmd_demo}
        args.command = getattr(args, "command", args.command)
        return handlers[args.command](args)
    except ConfigError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return exc.exit_code
    except json.JSONDecodeError as exc:
        print("error: the manifest is not JSON (%s)" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except OSError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except SystemExit as exit_:                  # the `--manifest`/`--path` guard above
        if not exit_.args or not isinstance(exit_.args[0], str):
            raise
        print("error: %s" % exit_.args[0], file=sys.stderr)
        return EXIT_CODES["USAGE"]


if __name__ == "__main__":                                  # pragma: no cover
    sys.exit(main())
