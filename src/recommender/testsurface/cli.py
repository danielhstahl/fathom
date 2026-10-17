"""`fathom testsurface …` — the verb surface of `e2.9`, and the only IO in the package.

The Rust product exposes this from `fathom-cli` (verbs parse, build a context, dispatch,
return an exit code — ADR-002 §4.3, §7.3); the executable forms here are:

```
python3 -m recommender.testsurface card --manifest episode.json     # the per-episode card
python3 -m recommender.testsurface card --repo after --old-repo before --path src/a.rs
python3 -m recommender.testsurface explain <path> --before a --after b
python3 -m recommender.testsurface policy                          # switches + winning layer
python3 -m recommender.testsurface definition                      # the tunable definition
python3 -m recommender.testsurface languages                       # the parser table
python3 -m recommender.testsurface score --min-precision 1.0       # the CI gate
python3 -m recommender.testsurface bench
python3 -m recommender.testsurface demo                            # the docs' card
```

A pair is *two revisions of one path*, and the verb needs both to say anything useful, so
it takes them three ways, in increasing order of fuss: `--before/--after` name two files;
`--repo` plus `--old-repo` read one path out of two trees; `--manifest` carries the whole
episode, inline or base64, which is the shape `e2.11` will hand to consumers:

```json
{"id": "ep-1", "title": "add write_snapshot, skip a flaky test",
 "changes": [{"path": "src/store.rs", "change": "modified",
              "before": "…", "after": "…",
              "before_base64": "…", "after_base64": "…",
              "old_path": "src/old.rs",
              "added_lines": 12, "deleted_lines": 8}],
 "repo_test_mentions": ["load_snapshot"]}
```

The flags for finding `fathom.toml` are `e0.8`'s verbatim, so `fathom testsurface policy`
and `fathom config explain` cannot disagree about which layer won. `added_lines` /
`deleted_lines` are accepted and preferred when the caller has real diff stats; without
them the pair is counted here, with the ranges kept so a Rust test module can be attributed
separately.
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
from .delta import Change, Episode, measure
from .eval import (MIN_EPISODES, MIN_PAIRS, MIN_PARSE, MIN_PRECISION, MIN_RECALL, score)
from .parsers import CASES_VERSION, describe_languages, parse
from .policy import Policy

#: Same cap `e2.1` puts on a blob it will read: a 400 MB committed file is not parsed.
MAX_BLOB_BYTES = 4 * 1024 * 1024


#: The global `--help` epilog: every verb, and where its configuration lives.
_TOPIC_DOC = """\
verbs
  card          the per-episode card: ratio and absolutes, file counts, named cases,
                findings with locations, and the stated confidence of the proxy
  findings      the same episode, findings only, one per line
  explain       one file pair: what the parser thinks is a test, and what changed
  languages     the parser table, and the list of what is *not* understood
  policy        every finding switch and every definition knob, with the winning layer
  definition    the definition of 'new public logic, no new test', as configured
  score         accuracy and finding-quality gates on the labelled corpus (CI)
  bench         cost per parse, per episode, per card
  demo          one worked episode across four languages (docs, and the smoke test)

e2.9 asks one question per episode: what did the testing contract do, in names and
locations? Tests deleted, tests skipped and assertions weakened are findings of their
own, and the ratio is always shown with its two absolutes, never as a bare quotient. A
repository with no test framework this tool can parse is not reported as zero tests: it
reports a line-count proxy and says so.

configuration (`[test]`)
  test.findings.<kind>            one switch per finding kind — see `policy`
  test.untested.<knob>            the definition of "new public logic, no new test"
  test.proxy.confidence.<basis>   what to claim when the basis is weaker than named cases
  test.card.max_findings          rows per card; the remainder is counted, not dropped
  `fathom testsurface policy` prints every key with the layer that won it; the flags for
  finding `fathom.toml` are `e0.8`'s, so this verb and `fathom config explain` cannot
  disagree. See [docs/concepts/test-surface.md](docs/concepts/test-surface.md).
"""


def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom testsurface", parents=[common],
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description="Test surface delta and coverage proxy (e2.9 / reco-6pl): the testing "
                    "contract per episode, not a coverage percentage.",
        epilog=_TOPIC_DOC)
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("card", parents=[common],
                       help="the per-episode card: ratio, cases, findings, confidence")
    _pair_flags(p)
    p.add_argument("--all", action="store_true",
                   help="print every finding row, ignoring test.card.max_findings")
    p.add_argument("--policy", action="store_true", help="print the policy block too")
    p.add_argument("--definition", action="store_true",
                   help="print the tunable definition under the card")
    p.add_argument("--cols", type=int, default=render.DEFAULT_COLS)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("findings", parents=[common],
                       help="just the findings for one episode, one per line")
    _pair_flags(p)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("explain", parents=[common],
                       help="one file pair: what it thinks is a test, and what changed")
    p.add_argument("path", nargs="+")
    p.add_argument("--old-repo", type=Path, default=None)
    p.add_argument("--before", type=Path, default=None)
    p.add_argument("--after", type=Path, default=None)
    p.add_argument("--json", action="store_true")

    sub.add_parser("languages", help="the parser table: languages, frameworks, refusals")

    p = sub.add_parser("policy", parents=[common],
                       help="every switch and every number, with the layer that won")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("definition", parents=[common],
                       help="the definition of 'new public logic, no new test', as configured")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("score", parents=[common],
                       help="accuracy and finding gates on the labelled corpus")
    p.add_argument("--min-parse", type=float, default=MIN_PARSE)
    p.add_argument("--min-pairs", type=float, default=MIN_PAIRS)
    p.add_argument("--min-precision", type=float, default=MIN_PRECISION)
    p.add_argument("--min-recall", type=float, default=MIN_RECALL)
    p.add_argument("--min-episodes", type=float, default=MIN_EPISODES)
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("bench", parents=[common],
                       help="cost per file and per episode, so the card budget is a number")
    p.add_argument("--repeat", type=int, default=4)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("demo", help="render the four-language demo card (docs and smoke test)")
    p.add_argument("--json", action="store_true")
    p.add_argument("--capped", action="store_true",
                   help="honour test.card.max_findings exactly as a real card does; the "
                        "default prints every row, because the demo exists to demonstrate "
                        "them and a doc card ending in \'… N more\' documents neither")
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
        files.append(Change(path=entry.get("path", ""),
                            old_path=entry.get("old_path", ""),
                            before=_field(entry, "before"), after=_field(entry, "after"),
                            change_type=entry.get("change",
                                                  entry.get("change_type", "modified")),
                            added_lines=int(entry.get("added_lines", 0) or 0),
                            deleted_lines=int(entry.get("deleted_lines", 0) or 0)))
    return Episode(id=payload.get("id", ""), files=tuple(files),
                   title=payload.get("title", ""),
                   repo_test_mentions=tuple(payload.get("repo_test_mentions", ())))


def episode_from_paths(args) -> Episode:
    files = [Change(path=path, before=_sides(args, path)[0], after=_sides(args, path)[1])
             for path in args.paths]
    return Episode(id="", files=tuple(files), title=getattr(args, "title", ""))


def episode_from_args(args) -> tuple[Episode, dict]:
    if args.manifest:
        payload = json.loads(Path(args.manifest).read_text("utf-8"))
        return episode_from_manifest(payload), payload
    if not args.paths:
        raise SystemExit("card: pass --manifest or at least one --path")
    return episode_from_paths(args), {}


# ---------------------------------------------------------------------------
# the verbs
# ---------------------------------------------------------------------------
def cmd_card(args) -> int:
    episode, _payload = episode_from_args(args)
    view = measure(episode, _policy(args))
    if args.json:
        print(json.dumps(view.as_dict(), indent=2))
        return 0
    lines = render.render(view, cols=args.cols,
                          max_findings=10**9 if args.all else None,
                          header=bool(episode.title or episode.id))
    for line in lines:
        print(line)
    problems = render.audit(lines, view=view,
                            max_findings=10**9 if args.all else None).problems
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


def cmd_findings(args) -> int:
    episode, _payload = episode_from_args(args)
    view = measure(episode, _policy(args))
    if args.json:
        print(json.dumps([finding.as_dict() for finding in view.findings], indent=2))
        return 0
    for finding in view.findings:
        print("%-20s %s" % (finding.kind, finding.render()))
    if not view.findings:
        print("no findings")
    return 0


def cmd_explain(args) -> int:
    policy = _policy(args)
    payload = []
    for path in args.path:
        before, after = _sides(args, path)
        if args.json:
            old = parse(path, before) if before is not None else None
            new = parse(path, after) if after is not None else None
            row = (new or old).as_dict()
            row["path"] = path
            payload.append(row)
            continue
        for line in X.explain_file(path, before, after, policy=policy):
            print(line)
        print()
    if args.json:
        print(json.dumps(payload if len(payload) > 1 else payload[0], indent=2))
    return 0


def cmd_languages(args) -> int:
    for line in describe_languages():
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
        print(json.dumps({"definition": [line for line in X.explain_definition(policy)],
                          "knobs": policy.as_dict()["knobs"]}, indent=2))
        return 0
    for line in X.explain_definition(policy):
        print(line)
    return 0


def cmd_score(args) -> int:
    report = score(policy=_policy(args))
    problems = report.gates(parse=args.min_parse, pairs=args.min_pairs,
                            precision=args.min_precision, recall=args.min_recall,
                            episodes=args.min_episodes)
    if args.json:
        print(json.dumps({"report": report.as_dict(), "problems": problems,
                          "bars": {"parse": args.min_parse, "pairs": args.min_pairs,
                                   "precision": args.min_precision,
                                   "recall": args.min_recall,
                                   "episodes": args.min_episodes}}, indent=2))
    else:
        print(report.text(verbose=args.verbose))
        if problems:
            print("FAIL")
            for problem in problems:
                print("  %s" % problem)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_bench(args) -> int:
    repeat = max(1, args.repeat)
    pairs = [(case.path, case.before, case.after) for case in CORPUS.PAIR_CASES]
    started = time.perf_counter()
    for _ in range(repeat):
        for path, before, after in pairs:
            parse(path, before)
            parse(path, after)
    parse_ms = (time.perf_counter() - started) * 1000 / (repeat * max(1, len(pairs) * 2))

    episodes = [case for case in CORPUS.EPISODE_CASES]
    started = time.perf_counter()
    for _ in range(repeat):
        for case in episodes:
            episode = Episode(id=case.name,
                              files=tuple(Change.from_pair(path, before, after)
                                          for path, (before, after) in case.files.items()))
            measure(episode)
    episode_ms = (time.perf_counter() - started) * 1000 / max(1, repeat * len(episodes))

    started = time.perf_counter()
    for _ in range(repeat):
        demo = X.demo()
        render.render(demo)
    card_ms = (time.perf_counter() - started) * 1000 / max(1, repeat)

    if args.json:
        print(json.dumps({"per_parse_ms": round(parse_ms, 3),
                          "per_episode_ms": round(episode_ms, 3),
                          "per_card_ms": round(card_ms, 3),
                          "corpus": {"pairs": len(pairs), "episodes": len(episodes),
                                     "parses": len(CORPUS.PARSE_CASES)},
                          "cases_version": CASES_VERSION, "repeat": repeat}, indent=2))
        return 0
    print("test surface bench (repeat %d): %.3f ms per parse, %.3f ms per episode, "
          "%.3f ms per card" % (repeat, parse_ms, episode_ms, card_ms))
    return 0


def cmd_demo(args) -> int:
    """The docs' card: one episode, four languages, one finding of every kind.

    The row cap is lifted by default, and the card says so: the demo exists to show one of
    every finding kind, and the two least severe kinds are exactly the ones a 12-row cap
    hides. `--capped` renders the same episode under the configured cap, which is what a
    real review sees.
    """
    demo = X.demo()
    if args.json:
        print(json.dumps(demo.as_dict(), indent=2))
        return 0
    cap = None if args.capped else 10**9
    lines = render.render(demo, max_findings=cap)
    if not args.capped:
        lines.extend(["    note: the demo prints every finding row; a real episode honours",
                      "          test.card.max_findings = %s, which `--capped` shows"
                      % demo.policy.number("max_findings")])
    for line in lines:
        print(line)
    problems = render.audit(lines, view=demo, max_findings=cap).problems
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        handlers = {"card": cmd_card, "findings": cmd_findings, "explain": cmd_explain,
                    "languages": cmd_languages, "policy": cmd_policy,
                    "definition": cmd_definition, "score": cmd_score, "bench": cmd_bench,
                    "demo": cmd_demo}
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
