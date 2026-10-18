"""`fathom rename …` — the verb surface of `e2.4`, and the only IO in the package besides `gitio`.

A window is *two revisions of one tree*, and every verb that says anything about a pairing needs one,
so it can be supplied three ways, in increasing order of what it can prove:

```
--old-repo DIR --new-repo DIR     two working trees: content only, so every pairing is a guess
--manifest FILE                   a serialised window (the shape e2.11's consumers hand us)
--repo REPO --base A --head B     a real git repository: git's own rename lines come along too
```

```
pairs       the pairing card: asserted, guessed, suppressed, and the cost of the window
explain     one window or one path: why it is a fact, why that is a guess, and how to check it
policy      every knob and the layer that won; what the engine will not claim
ledger      the accept/reject decisions on disk, and any that no longer match a pairing
accept      record that a reviewer agrees with a guess (it is shown as asserted afterwards)
reject      record that a reviewer refuses it (hidden from the card, still counted)
undo        forget a recorded decision
score       the measured gates on the labelled corpus (CI)
bench       the cost bench: repository size up, change set fixed, shipped mode beside the refused one
demo        one worked window for the documents
```

The config flags are `e0.8`'s verbatim, so `fathom rename policy` and `fathom config explain` cannot
disagree about which layer won. Rows are numbered by their position on the card, which is what
`accept`/`reject` take (`--row 3`) alongside `--path` for anything with a name in it.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

from recommender.config.cli import common_flags
from recommender.config.errors import ConfigError, EXIT_CODES
from recommender.config.layer import load as load_config

from . import corpus as CORPUS
from . import eval as EVAL
from . import explain as X
from . import gitio
from . import ledger as L
from . import model as M
from . import pair as PAIR
from . import render
from .policy import Policy

REPORT = Path("benchmarks") / "reports" / "e2-4-rename-pairing.json"

_TOPIC_DOC = """\
verbs
  pairs     the pairing card: what moved, at what tier, what is hidden and what was unseeable
  explain   why one pairing is a fact or a guess, with the command that checks it
  policy    every knob, the layer that won, and what this engine will not claim
  ledger    the recorded accept/reject decisions, and the ones that no longer match
  accept    agree with a guess: it is shown as asserted, with your name on it
  reject    refuse a pairing: hidden from the card and still counted
  undo      forget a recorded decision
  score     the measured gates on the labelled corpus (CI)
  bench     cost: repository size up with the change set fixed, and the refused O(repo) mode beside it
  demo      one worked window, for the documents

e2.4 pairs paths across a review window and says, for every pairing, whether git asserts it or
we only guess. A fact is a pairing git's rewriter reports at rename.fact_bar; everything else is a
labelled guess a reviewer may accept or reject. Splits, folds and copies are groupings, never
flattened into 1:1 renames. e2.3 (folder delta) and e2.10 (the episode card) consume
`recommender.rename.pair.pair()` directly.

verbs that read a window take --old-repo/--new-repo, --manifest, or --repo with --base/--head.
"""


# -- plumbing ------------------------------------------------------------------------------------

def _config(args):
    env = {} if getattr(args, "no_env", False) else None
    return load_config(getattr(args, "repo", None), repo_file=getattr(args, "repo_file", None),
                       user_file=getattr(args, "user_file", None), env=env,
                       require_repo=False, sets=getattr(args, "set", []),
                       extends=getattr(args, "extend", []),
                       offline=getattr(args, "offline", False))


def _policy(args) -> Policy:
    return Policy.from_config(_config(args))


def _add_window_flags(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--base", default="HEAD~1", help="base revision (default HEAD~1)")
    parser.add_argument("--head", default="HEAD", help="head revision (default HEAD)")
    parser.add_argument("--old-repo", default=None, help="compare two working trees instead of git")
    parser.add_argument("--new-repo", default=None, help="with --old-repo")
    parser.add_argument("--manifest", default=None, help="read a serialised window")
    parser.add_argument("--window-id", default="", help="label the window (cards and ledger keys)")
    parser.add_argument("--episode", action="store_true",
                        help="label the window an episode rather than a commit pair")


def _repo_dir(args) -> Path:
    """The repository to read. `--repo` is `e0.8`'s flag and defaults to discovery, so a bare
    invocation means "the current directory", which is what a reviewer standing in a checkout has."""
    return Path(getattr(args, "repo", None) or ".")


def _window(args, policy: Policy) -> M.Window:
    granularity = "episode" if getattr(args, "episode", False) else "commit_pair"
    if args.manifest:
        window = gitio.window_from_manifest(json.loads(Path(args.manifest).read_text()))
        if args.window_id:
            window.id = args.window_id
        return window
    if args.old_repo or args.new_repo:
        if not (args.old_repo and args.new_repo):
            raise SystemExit("--old-repo and --new-repo go together")
        window = gitio.window_from_dirs(Path(args.old_repo), Path(args.new_repo),
                                        window_id=args.window_id or "worktree",
                                        granularity=granularity)
        return window
    repo = _repo_dir(args)
    return gitio.window_from_repo(repo, args.base, args.head,
                                  window_id=args.window_id or "%s..%s" % (args.base, args.head),
                                  granularity=granularity, policy=policy)


def _ledger_path(args, repo: Path) -> Path:
    if getattr(args, "state", None):
        return Path(args.state)
    return Path(repo) / L.DEFAULT_PATH


def _reviewer(args, repo: Path) -> str:
    if getattr(args, "reviewer", ""):
        return args.reviewer
    try:
        return subprocess.run(["git", "config", "user.email"], cwd=repo, capture_output=True,
                              text=True, check=True).stdout.strip() or "unknown"
    except (subprocess.CalledProcessError, OSError):
        return "unknown"


def _result(args, policy: Policy) -> M.Result:
    window = _window(args, policy)
    ledger = L.Ledger.load(_ledger_path(args, _repo_dir(args)))
    return PAIR.pair(window, policy, ledger=ledger.as_mapping())


def _pick(pairings: list[M.Pairing], args) -> list[M.Pairing]:
    if getattr(args, "row", None):
        ordered = [p for p in pairings if p.disposition != M.DISPOSITION_REJECTED]
        try:
            return [ordered[int(args.row) - 1]]
        except (ValueError, IndexError):
            raise SystemExit("row %s is not on the card (1..%d); run `fathom rename pairs`"
                             % (args.row, len(ordered)))
    if getattr(args, "path", ""):
        hits = [p for p in pairings
                if args.path in list(p.sources) + list(p.destinations)]
        if not hits:
            raise SystemExit("no pairing mentions %r" % args.path)
        return hits
    raise SystemExit("name the pairing: --row N (the number on the card) or --path PATH")


# -- verbs ---------------------------------------------------------------------------------------

def cmd_pairs(args) -> int:
    policy = _policy(args)
    result = _result(args, policy)
    if args.json:
        print(json.dumps(result.as_dict(), indent=1, sort_keys=True))
        return 0
    lines = render.render(result, policy, limit=None if args.all else policy.integer("card_rows"))
    audit = render.audit(result, lines, policy=policy)
    for line in lines:
        print(line)
    if args.audit or not audit.ok:
        for line in audit.describe():
            print(line)
    return 0 if audit.ok else EXIT_CODES["ERROR"]


def _needs_window(args) -> bool:
    """`rename explain` with no window prints what the engine will not claim; with one, its pairs."""
    return bool(args.manifest or args.old_repo or args.new_repo or args.window_id
                or (args.base, args.head) != ("HEAD~1", "HEAD"))


def cmd_explain(args) -> int:
    policy = _policy(args)
    if not _needs_window(args):
        for line in X.about(policy):
            print(line)
        return 0
    result = _result(args, policy)
    lines = X.explain_window(result, policy, needle=args.path or "")
    if args.json:
        print(json.dumps({"window": result.as_dict()["window"], "lines": lines,
                          "pairings": [p.as_dict() for p in result.pairings]}, indent=1,
                         sort_keys=True))
        return 0
    for line in lines:
        print(line)
    return 0


def cmd_policy(args) -> int:
    policy = _policy(args)
    if args.json:
        print(json.dumps({"policy": policy.as_dict(), "limits": policy.limits()}, indent=1,
                         sort_keys=True))
        return 0
    for line in policy.describe():
        print(line)
    print("")
    for line in X.about(policy):
        print(line)
    return 0


def cmd_ledger(args) -> int:
    policy = _policy(args)
    ledger = L.Ledger.load(_ledger_path(args, _repo_dir(args)))
    problems: list[str] = []
    if args.audit:
        problems = ledger.audit_against(_result(args, policy))
    if args.json:
        print(json.dumps({"path": str(ledger.path or ""), "entries": ledger.as_document()["entries"],
                          "unreadable": ledger.unreadable, "stale": problems}, indent=1,
                         sort_keys=True))
        return 0
    for line in ledger.describe():
        print(line)
    for problem in problems:
        print("  ? %s" % problem)
    return 0


def _record(args, decision: str) -> int:
    policy = _policy(args)
    result = _result(args, policy)
    targets = _pick(result.pairings, args)
    ledger = L.Ledger.load(_ledger_path(args, _repo_dir(args)))
    reviewer = _reviewer(args, _repo_dir(args))
    when = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    for pairing in targets:
        entry = ledger.record(pairing, decision, reviewer=reviewer, when=when, note=args.note)
        print("%-8s %s %s -> %s [%s] by %s (bound %s)"
              % (entry.decision, entry.kind, ", ".join(entry.sources),
                 ", ".join(entry.destinations), entry.tier, reviewer, entry.binding[:8]))
        if pairing.tier == M.TIER_FACT and decision == M.DISPOSITION_REJECTED:
            print("  note: git asserts this one at the fact bar; your rejection hides it from the "
                  "card and it stays counted as suppressed, but it is not evidence that was ours "
                  "to overrule")
    if args.dry_run:
        print("(--dry-run: nothing written)")
        return 0
    path = ledger.save()
    print("wrote %s (%d decision(s))" % (path, len(ledger.entries)))
    return 0


def cmd_accept(args) -> int:
    return _record(args, M.DISPOSITION_ACCEPTED)


def cmd_reject(args) -> int:
    return _record(args, M.DISPOSITION_REJECTED)


def cmd_undo(args) -> int:
    policy = _policy(args)
    ledger = L.Ledger.load(_ledger_path(args, _repo_dir(args)))
    if args.all:
        count = len(ledger.entries)
        ledger.entries.clear()
        ledger.save()
        print("forgot all %d decision(s)" % count)
        return 0
    result = _result(args, policy)
    removed = 0
    for pairing in _pick(result.pairings, args):
        removed += 1 if ledger.forget(pairing.key()) else 0
    ledger.save()
    print("forgot %d decision(s)" % removed)
    return 0


def cmd_score(args) -> int:
    import tempfile
    work = Path(tempfile.mkdtemp(prefix="fathom-e24-"))
    started = time.perf_counter()
    report = EVAL.measure(work, policy=_policy(args))
    report["measured_in_seconds"] = round(time.perf_counter() - started, 2)
    if args.write:
        EVAL.write_report(report, Path(args.report))
        print("wrote %s" % args.report)
    problems = EVAL.check_report(report, Path(args.report)) if args.check else []
    if args.json:
        print(json.dumps(report, indent=1, sort_keys=True))
        return EXIT_CODES["ERROR"] if problems else 0
    print(EVAL.text(report))
    if args.check:
        if problems:
            print("\nCHECK FAILED — the committed report disagrees with this run:")
            for problem in problems:
                print("  ! %s" % problem)
            return EXIT_CODES["ERROR"]
        print("\ncheck: this run reproduces %s" % args.report)
    return EXIT_CODES["ERROR"] if any(g["result"] != "pass" for g in report["gates"]) else 0


def cmd_bench(args) -> int:
    import tempfile
    work = Path(tempfile.mkdtemp(prefix="fathom-e24-bench-"))
    cost = EVAL.cost_measure(work)
    if args.json:
        print(json.dumps(cost, indent=1, sort_keys=True))
        return 0
    print("cost: repository size up, change set fixed at 6 moved pairs")
    for column, rows in (("shipped (window-local)", cost["shipped"]),
                         ("copies_harder (O(repo))", cost["widened"])):
        print("  %s" % column)
        for row in rows:
            print("    %6d files · %5.1f ms read + %5.1f ms paired · %d blob(s) · %d candidate "
                  "pair(s)" % (row["repo_files"], row["read_ms"], row["pair_ms"],
                               row["blobs_read"], row["candidates"]))
    print("  ratio smallest → 20x: shipped %.2fx, widened %.2fx"
          % (cost["ratio_small_to_20x_shipped"], cost["ratio_small_to_20x_widened"]))
    print("change set 1 → 48 pairs at 400 files: %.2fx" % cost["change_1_to_48_shipped"])
    return 0


def cmd_corpus(args) -> int:
    """The corpus and its labels, printed: expectations are hand-authored, so print them."""
    truths = CORPUS.truth_windows()
    if args.json:
        print(json.dumps([{
            "scenario": t.scenario, "window": t.id, "base": t.base, "head": t.head,
            "review": t.review, "pairs": t.pairs, "arrivals": t.arrivals, "note": t.note}
            for t in truths], indent=1, sort_keys=True))
        return 0
    by: dict[str, list] = {}
    for truth in truths:
        by.setdefault(truth.scenario, []).append(truth)
    for scenario in sorted(by):
        rows = [p for truth in by[scenario] for p in truth.pairs]
        print("%-24s %2d window(s), %2d labelled pair(s)"
              % (scenario, len(by[scenario]), len(rows)))
    print("\nprint one with `python3 -m recommender.rename corpus --json`")
    return 0


def cmd_demo(args) -> int:
    """One worked window: four clean moves and a rewritten one, built on the spot."""
    import tempfile
    from . import specs
    work = Path(tempfile.mkdtemp(prefix="fathom-e24-demo-"))
    script, _note = specs.SPECS["rename_rewrite"]()
    labels = CORPUS.R.build(work, script)
    policy = Policy.built_in().with_knobs("demo")
    window = gitio.window_from_repo(work, labels["c0"], labels["c1"], window_id="demo/rewrite")
    result = PAIR.pair(window, policy)
    lines = render.render(result, policy, width=args.width, limit=None)
    for line in lines:
        print(line)
    if args.audit:
        for line in render.audit(result, lines, policy=policy, width=args.width).describe():
            print(line)
    return 0


# -- parser --------------------------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="fathom rename",
        description="rename, split, merge and copy pairing with explicit tiers (e2.4)",
        epilog=_TOPIC_DOC, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    common = common_flags()          # e0.8's discovery flags, including `--repo`

    def add(name: str, handler, *, window: bool = True, json_flag: bool = True) -> argparse.ArgumentParser:
        sub = commands.add_parser(name, parents=[common], help=(handler.__doc__ or name).strip())
        if window:
            _add_window_flags(sub)
            sub.add_argument("--state", default=None,
                             help="where the accept/reject ledger lives (default "
                                  ".fathom/rename-review.json)")
        if json_flag:
            sub.add_argument("--json", action="store_true", help="machine-readable output")
        sub.set_defaults(func=handler)
        return sub

    pairs = add("pairs", cmd_pairs)
    pairs.add_argument("--all", action="store_true", help="do not cap the rows")
    pairs.add_argument("--audit", action="store_true", help="re-check the card and say what it found")

    explain = add("explain", cmd_explain)
    explain.add_argument("--path", default="", help="explain only pairings mentioning this path")

    add("policy", cmd_policy, window=False)

    ledger = add("ledger", cmd_ledger)
    ledger.add_argument("--audit", action="store_true",
                        help="also check every recorded decision still matches a pairing")

    for name, handler in (("accept", cmd_accept), ("reject", cmd_reject)):
        sub = add(name, handler)
        sub.add_argument("--row", default=None, help="the row number from the card")
        sub.add_argument("--path", default="", help="or a path the pairing mentions")
        sub.add_argument("--note", default="", help="why, for whoever reads this next")
        sub.add_argument("--reviewer", default="", help="who is deciding (default: git config)")
        sub.add_argument("--dry-run", action="store_true", help="print what would be written")

    undo = add("undo", cmd_undo)
    undo.add_argument("--row", default=None)
    undo.add_argument("--path", default="")
    undo.add_argument("--all", action="store_true", help="forget every recorded decision")

    score = add("score", cmd_score, window=False, json_flag=False)
    score.add_argument("--report", default=str(REPORT))
    score.add_argument("--only", action="append", default=[], choices=sorted(CORPUS.fixtures()),
                       help="measure one fixture (repeatable); default is all of them")
    score.add_argument("--write", action="store_true", help="write/refresh the report file")
    score.add_argument("--check", action="store_true",
                       help="fail if this run does not reproduce the committed report")
    score.add_argument("--json", action="store_true")

    bench = add("bench", cmd_bench, window=False, json_flag=False)
    bench.add_argument("--json", action="store_true")

    corpus = add("corpus", cmd_corpus, window=False, json_flag=False)
    corpus.add_argument("--json", action="store_true")

    demo = add("demo", cmd_demo, window=False)
    demo.add_argument("--width", default=render.WIDTH, type=int)
    demo.add_argument("--audit", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except ConfigError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return exc.exit_code
    except json.JSONDecodeError as exc:
        print("error: the manifest is not JSON (%s)" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except (gitio.WindowSourceError, L.LedgerError) as exc:
        print("error: %s" % exc, file=sys.stderr)
        return EXIT_CODES["ERROR"]
    except OSError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except SystemExit as exit_:
        if not exit_.args or not isinstance(exit_.args[0], str):
            raise
        print("error: %s" % exit_.args[0], file=sys.stderr)
        return EXIT_CODES["USAGE"]


if __name__ == "__main__":                                  # pragma: no cover
    sys.exit(main())
