#!/usr/bin/env python3
"""Fetch the Guardrail 3 calibration corpus: three real OSS repositories, each at its pinned commit.

This is the *only* thing that puts the corpus repositories on disk, and it is deliberately a fetcher and
not a generator: it clones what the pins name, checks the pin, and stops. It cannot write
`crates/fathom-analyze/calibration/g3/**` — the pins and the hand-authored labels are inputs, and a tool
that fetched a repository and also wrote the truth about it would be a harness authoring its own truth
(the rule spikes `s1` and `s2` left behind, and `tests/test_g3_calibration_contract.py` re-checks that no
file in this tree writes to a `truth.json`).

```text
python3 tools/fetch_g3_corpus.py                      # clone into target/g3-corpus
python3 tools/fetch_g3_corpus.py --dest /tmp/g3       # somewhere else
python3 tools/fetch_g3_corpus.py --check              # are the clones present and at the pin?
```

Each clone is shallow on purpose: the harness measures one window, and a clone that carries the whole
history of a large repository is 100x the disk for commits nobody measures. The depth below is
`window * 2 + 20`, because the segmentation groups commits into episodes and merge parents reach past a
single-parent count — a window that ran off the graft would leave the harness measuring a stated partial
where it means to measure a window.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
CORPUS = ROOT / "crates" / "fathom-analyze" / "calibration" / "g3" / "corpus"
DEFAULT_DEST = ROOT / "target" / "g3-corpus"


def git(args: list[str], cwd: pathlib.Path | None = None) -> str:
    """Run git, and say what failed rather than raising a traceback at a person."""
    proc = subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False
    )
    if proc.returncode != 0:
        sys.exit(f"git {' '.join(args)} failed: {proc.stderr.strip() or proc.stdout.strip()}")
    return proc.stdout.strip()


def pins() -> list[tuple[pathlib.Path, dict]]:
    out = []
    for directory in sorted(p for p in CORPUS.iterdir() if p.is_dir()):
        pin = json.loads((directory / "pin.json").read_text())
        missing = [k for k in ("repository", "url", "head", "window") if k not in pin]
        if missing:
            sys.exit(f"{directory / 'pin.json'}: missing {missing}")
        out.append((directory, pin))
    if not out:
        sys.exit(f"no pins under {CORPUS}")
    return out


def main() -> int:
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--dest", default=str(DEFAULT_DEST), help="where the clones live")
    parser.add_argument(
        "--check", action="store_true", help="report whether each clone is present and at its pin"
    )
    parser.add_argument("--force", action="store_true", help="re-clone even when the pin is satisfied")
    args = parser.parse_args()
    dest = pathlib.Path(args.dest)
    dest.mkdir(parents=True, exist_ok=True)

    bad = 0
    for directory, pin in pins():
        where = dest / pin["repository"]
        head = ""
        if (where / ".git").is_dir():
            head = git(["rev-parse", "HEAD"], cwd=where)
        if head == pin["head"] and not args.force:
            print(f"{pin['repository']:<12} at {pin['head'][:12]} — already pinned")
            continue
        if args.check:
            print(
                f"{pin['repository']:<12} NOT AT PIN — want {pin['head'][:12]}, "
                f"have {head[:12] if head else 'nothing'}"
            )
            bad += 1
            continue
        depth = int(pin["window"]) * 2 + 20
        if where.exists():
            subprocess.run(["rm", "-rf", str(where)], check=True)
        print(f"{pin['repository']:<12} cloning {pin['url']} at {pin['head'][:12]} (depth {depth})")
        # `--no-checkout` first, then fetch the pinned oid: a plain shallow clone takes the branch tip,
        # and the pin is an oid that is not necessarily the tip of anything.
        git(["init", "-q", str(where)])
        git(["remote", "add", "origin", pin["url"]], cwd=where)
        git(["fetch", "-q", "--depth", str(depth), "origin", pin["head"]], cwd=where)
        git(["checkout", "-q", "--detach", pin["head"]], cwd=where)
        got = git(["rev-parse", "HEAD"], cwd=where)
        if got != pin["head"]:
            sys.exit(f"{pin['repository']}: asked for {pin['head']}, the remote gave {got}")
        print(f"{pin['repository']:<12} ok ({git(['rev-list', '--count', 'HEAD'], cwd=where)} commits available)")
    if bad:
        print(f"\n{bad} clone(s) missing or off-pin; run without --check to fetch them", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
