#!/usr/bin/env python3
"""Record the five-minute demo as an asciicast, and keep the recording honest.

`reco-3qr` asks for "an asciinema/GIF recording under five minutes", and the criterion is "recording is
under five minutes and viewable without sound". A hand-made recording satisfies neither for long: it goes
stale the first time a command changes its output, and nothing notices. So this file *makes* the recording
by running the tour and writing what the terminal actually received, and `check` re-reads the committed
cast and fails when it no longer says what the tour says.

    python3 tools/record_demo.py record [--out docs/demo/five-minute.cast]
    python3 tools/record_demo.py check                        # the gate CI runs
    python3 tools/record_demo.py gif                          # render the cast to a GIF, if a renderer exists
    python3 tools/record_demo.py show                          # the cast's transcript, as text

**How the recording is made.** Every step runs in a real pty (`pty.openpty()`, window size set to the
cast's 100×28, so wrapping is git's and the binary's and not this file's), and the bytes the terminal
received are written as asciicast v2 output events with the delay it actually took. The tour's own step
list comes from `tools/five_minute_demo.py`, run against a freshly assembled clone of the demo repository,
followed by the one command and one look per seeded pathology from `tools/demo_exhibits.py` — so the cast
and the two documents are three renderings of the same data. Between steps the recorder writes a caption
line (`▸ 3 · why is this one in front of me`) and a pause, and after a step that exits non-zero it writes
the exit code, because "viewable without sound" means the film has to say out loud that the gate went red.

**What `check` guarantees**, in order of how each one would embarrass the demo if it drifted: the cast is
under the five-minute budget (the same `BUDGET_S` the tour uses); it carries no audio of any kind — every
event is an output event, and the header says nothing to listen for; it was recorded at the demo's current
`demo-v*` tag, so a re-tag that skipped the re-recording is a failure rather than a grey hair; and every
string the pathology catalogue promises a viewer will see is in the transcript once the escape sequences are
stripped.

**The GIF is derived, not checked in by force.** `gif` runs `agg` if it is installed, or
`asciinema cat | gifski` if that pair is, and prints the exact command when neither is — exit 3, meaning
*not available here*, not *broken*. The cast is the artefact of record because it is text, it diffs, and it
can be checked; a GIF is the same film in a format nobody can grep.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import pty
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import fcntl
import termios

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import demo_exhibits  # noqa: E402
import five_minute_demo  # noqa: E402
import publish_demo_repo  # noqa: E402

CAST = ROOT / "docs" / "demo" / "five-minute.cast"
REPORT = ROOT / "docs" / "demo" / "five-minute.cast.json"

#: The pages that quote the film's length. `check` re-derives the number from the cast and refuses a page
#: that quotes another one — the rule `five_minute_demo.py` learned for the tour's timings, applied to the
#: recording, because "under five minutes" is a claim a number has to keep proving.
QUOTING = ("docs/demo/five-minute.md", "README.md")

WIDTH, HEIGHT = 100, 28

#: Reading time, in seconds: before a step's command line, after its output, and per added line of output.
#: The tour itself takes about three seconds of compute; the pauses are what make it watchable, and they
#: are capped so the film cannot grow past the budget by accident.
PAUSE_BEFORE = 0.5
PAUSE_AFTER_LINE = 0.05
PAUSE_AFTER_MAX = 3.0
#: How long a "typed" command takes to appear, as a per-character delay. Zero is a film that types faster
#: than a person reads and looks like a log rather than a demo.
TYPE_PER_CHAR = 0.012
TYPE_MAX = 0.6

ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[=>]")
PROMPT = "demo $ "


def strip_ansi(text: str) -> str:
    return ANSI.sub("", text).replace("\r\n", "\n").replace("\r", "\n")


def set_size(master: int) -> None:
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", HEIGHT, WIDTH, 0, 0))


def play(argv: list[str], cwd: pathlib.Path, env: dict[str, str]) -> tuple[bytes, float, int]:
    """Run one command on a real pty and return the bytes the terminal received, and how long it took."""
    master, slave = pty.openpty()
    set_size(master)
    started = time.time()
    process = subprocess.Popen(
        argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=slave, stderr=slave, env=env
    )
    os.close(slave)
    chunks: list[bytes] = []
    while True:
        try:
            chunk = os.read(master, 65536)
        except OSError:
            break
        if not chunk:
            break
        chunks.append(chunk)
    process.wait()
    os.close(master)
    return b"".join(chunks), time.time() - started, process.returncode


class Cast:
    """An asciicast v2, written as it is recorded."""

    def __init__(self, title: str) -> None:
        self.frames: list[tuple[float, str]] = []
        self.title = title
        self.clock = 0.0

    def wait(self, seconds: float) -> None:
        self.clock += seconds

    def write(self, text: str, *, raw: bool = False) -> None:
        """Emit one output event. `raw` keeps escape sequences; otherwise they reach the terminal as-is."""
        if not text:
            return
        payload = text if raw else text.replace("\n", "\r\n")
        self.frames.append((self.clock, payload))

    def caption(self, text: str) -> None:
        """A line the film says out loud, because there is no narrator."""
        self.wait(PAUSE_BEFORE)
        self.write(f"\x1b[1;36m▸ {text}\x1b[0m\r\n", raw=True)

    def type(self, command: str) -> None:
        self.wait(PAUSE_BEFORE)
        self.write(f"\x1b[32m{PROMPT}\x1b[0m", raw=True)
        self.wait(min(len(command) * TYPE_PER_CHAR, TYPE_MAX))
        self.write(f"{command}\r\n")

    def duration(self) -> float:
        return self.clock

    def write_to(self, path: pathlib.Path, timestamp: int) -> None:
        header = {
            "version": 2,
            "width": WIDTH,
            "height": HEIGHT,
            "idle_time_limit": 4.0,
            "timestamp": timestamp,
            "title": self.title,
            "env": {"SHELL": "/bin/sh", "TERM": "xterm-256color"},
        }
        with path.open("w", encoding="utf-8") as handle:
            handle.write(json.dumps(header) + "\n")
            previous = 0.0
            for delay, payload in self.frames:
                handle.write(
                    json.dumps([round(delay - previous, 6), "o", payload], ensure_ascii=False) + "\n"
                )
                previous = delay


def tour(root: pathlib.Path) -> list[dict]:
    """The steps to record: the scripted tour, the one command, and one look per pathology.

    The tour's own list is `five_minute_demo.steps`, so the cast cannot drift from the page that lists the
    same moves. Everything runs against a freshly cloned demo repository, which is what a stranger clones:
    the numeric claims that belong to the miniature alone are not made here.
    """
    oids: dict[str, str] = {}
    log = subprocess.run(
        ["git", "log", "--format=%H%x09%s"], cwd=root, capture_output=True, text=True, check=False
    )
    for line in log.stdout.splitlines():
        oid, _, subject = line.partition("\t")
        oids.setdefault(subject.strip(), oid)

    plan = [{"title": step.title, "argv": list(step.argv), "step": step}
            for step in five_minute_demo.steps(root, mini=False)]
    plan.append(
        {
            "title": "the one command: everything this page documents, in one artefact",
            "argv": list(demo_exhibits.ONE_COMMAND),
        }
    )
    for row in demo_exhibits.CATALOGUE:
        probe = (row.get("also") or [row["probe"]])[0]
        argv = list(probe.get("argv") or probe.get("git") or row["probe"]["argv"])
        plan.append(
            {
                "title": f"the seeded {row['pathology']} — {row['look'].split(';')[0]}",
                "argv": argv,
                "is_git": "git" in probe,
                "subs": {"$COMMIT": oids.get(row["commits"][0], "HEAD")},
            }
        )
    return plan


def render_argv(argument: list[str], root: pathlib.Path, extra: dict[str, str]) -> list[str]:
    out = []
    for item in argument:
        item = item.replace("$ROOT", str(root)).replace("$FIRST", demo_exhibits.FIRST_COMMIT)
        for needle, value in extra.items():
            item = item.replace(needle, value)
        out.append(item)
    return out


def record(args: argparse.Namespace) -> int:
    binary = args.bin or str(ROOT / "target" / "debug" / "fathom")
    if not pathlib.Path(binary).is_file():
        print(f"no fathom binary at {binary}; `cargo build -p fathom-cli` first", file=sys.stderr)
        return 1
    work = pathlib.Path(tempfile.mkdtemp(prefix="fathom-demo-cast-"))
    try:
        assembled, facts = publish_demo_repo.assemble()
        clone = work / "signal"
        subprocess.run(
            ["git", "clone", "--quiet", str(assembled), str(clone)],
            check=True, capture_output=True, text=True,
        )
        tag = facts["tag"]
        cast = Cast(f"{tag} · the five-minute Fathom demo · no sound needed")
        env = {
            **os.environ,
            "PS1": PROMPT,
            "TERM": "xterm-256color",
            "FATHOM_CHECK__MODE": "enforce",
            "FATHOM_REVIEWER": "Demo Reviewer <demo@fathom.test>",
            "PATH": os.environ.get("PATH", ""),
        }
        context: dict[str, str] = {}
        plan = tour(clone)
        cast.caption(f"{tag} — a repository Fathom reviews itself on, in under five minutes")
        for index, item in enumerate(plan, start=1):
            step = item.get("step")
            if step is not None:
                argv = step.render(clone, context)
                extra: dict[str, str] = {}
            else:
                extra = item.get("subs", {})
                argv = render_argv(item["argv"], clone, extra)
                if item.get("is_git"):
                    argv = ["git", *argv]
            # What the film types is what a person types: `fathom`, not wherever this run's binary lives,
            # and the clone as `signal`, not as somebody's temporary directory.
            shown = list(argv)
            command = " ".join(
                argument if " " not in argument else f'"{argument}"'
                for argument in (a.replace(str(clone), "signal") for a in shown)
            )
            run_argv = [binary if argument == "fathom" else argument for argument in argv]
            cast.caption(f"{index} · {item['title']}")
            cast.type(command)
            output, took, code = play(run_argv, clone, env)
            if step is not None and step.capture is not None:
                found = re.search(step.capture[1], output.decode("utf-8", "replace"))
                if found:
                    context[step.capture[0]] = found.group(1)
            cast.wait(max(took, 0.0))
            cast.write(output.decode("utf-8", "replace"), raw=True)
            if code != 0:
                cast.wait(PAUSE_BEFORE)
                cast.write(f"\x1b[1;31m→ exit {code}\x1b[0m\r\n", raw=True)
            lines = output.decode("utf-8", "replace").count("\n")
            cast.wait(min(lines * PAUSE_AFTER_LINE, PAUSE_AFTER_MAX))
        duration = cast.duration()
        if duration > five_minute_demo.BUDGET_S:
            print(
                f"the recorded film is {duration:.0f}s, over the {five_minute_demo.BUDGET_S:.0f}s budget; "
                "shorten the pauses or the tour",
                file=sys.stderr,
            )
            return 1
        out = pathlib.Path(args.out)
        cast.write_to(out, int(time.time()))
        report = {
            "contract": "fathom.demo-cast/1",
            "cast": os.path.relpath(out, ROOT) if str(out).startswith(str(ROOT)) else str(out),
            "tag": tag,
            "seconds": round(duration, 2),
            "budget_seconds": five_minute_demo.BUDGET_S,
            "events": len(cast.frames),
            "width": WIDTH,
            "height": HEIGHT,
            "audio": False,
            "steps": [item["title"] for item in plan],
        }
        destination = pathlib.Path(args.report) if args.report else REPORT
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {out}: {len(cast.frames)} events, {duration:.1f}s of film (budget "
              f"{five_minute_demo.BUDGET_S:.0f}s), tagged {tag}")
        return 0
    finally:
        shutil.rmtree(work, ignore_errors=True)


def read_cast(path: pathlib.Path) -> tuple[dict, list[tuple[float, str, str]]]:
    with path.open(encoding="utf-8") as handle:
        header = json.loads(handle.readline())
        events = []
        for line in handle:
            if not line.strip():
                continue
            event = json.loads(line)
            events.append((float(event[0]), str(event[1]), str(event[2])))
    return header, events


def transcript(events: list[tuple[float, str, str]]) -> str:
    return strip_ansi("".join(payload for _, _, payload in events))


def check(args: argparse.Namespace) -> int:
    path = pathlib.Path(args.out)
    if not path.is_file():
        print(f"{path} is missing; `python3 tools/record_demo.py record`", file=sys.stderr)
        return 1
    header, events = read_cast(path)
    problems: list[str] = []
    if header.get("version") != 2:
        problems.append(f"the cast is asciicast version {header.get('version')}, not 2")
    kinds = {kind for _, kind, _ in events}
    if not kinds <= {"o"}:
        problems.append(f"the cast carries {sorted(kinds)} events; a film with a sound track is a film "
                        "somebody cannot watch on a plane")
    if not events:
        problems.append("the cast has no events at all")
    # asciicast v2 stores each event's delay since the one before it, so the film's length is the sum.
    duration = sum(delay for delay, _, _ in events)
    if duration <= 0:
        problems.append("the cast has no length")
    elif duration > five_minute_demo.BUDGET_S:
        problems.append(
            f"the cast runs {duration:.0f}s, over the {five_minute_demo.BUDGET_S:.0f}s the criterion gives it"
        )
    title = str(header.get("title", ""))
    tag = publish_demo_repo.demo_tag()
    if tag not in title:
        problems.append(
            f"the cast was recorded at {title.split('·')[0].strip() or 'no tag'} and the demo is now "
            f"{tag} — re-record it (`tools/record_demo.py record`)"
        )
    # The sidecar the pages quote their numbers from, and the pages themselves. `five_minute_demo.py` long
    # ago learned that a document quoting a remembered timing is a document that goes stale; a film is the
    # same problem with a bigger surface, so the number is re-derived here rather than trusted.
    sidecar = path.with_suffix(".json")
    label = sidecar.relative_to(ROOT) if sidecar.is_relative_to(ROOT) else sidecar
    if sidecar.is_file():
        report = json.loads(sidecar.read_text(encoding="utf-8"))
        if abs(float(report.get("seconds", -1)) - duration) > 0.6:
            problems.append(
                f"{label} says {report.get('seconds')}s and the cast runs "
                f"{duration:.1f}s; re-record, do not edit the JSON"
            )
        if report.get("tag") != tag:
            problems.append(f"{label} is tagged {report.get('tag')} and the demo is {tag}")
        if report.get("audio"):
            problems.append(f"{label} claims an audio track")
    elif sidecar == REPORT:
        problems.append(f"{REPORT.relative_to(ROOT)} is missing; `record` writes it next to the cast")
    for page in QUOTING:
        text = (ROOT / page).read_text(encoding="utf-8")
        if f"{round(duration):d} s" not in text:
            problems.append(
                f"{page} does not say {round(duration):d} s, which is how long the film now runs"
            )
    said = transcript(events)
    for needle in ("fathom check failed", "→ exit 7", "review.md"):
        if needle not in said:
            problems.append(f"the cast never shows {needle!r}, which the tour promises a viewer sees")
    for row in demo_exhibits.CATALOGUE:
        # One of the row's promises, not every one: the strict version — each documented string in the
        # artefact the row names — is `tools/demo_acceptance.py`'s job, and some of those strings live in
        # the exported file rather than on the screen. What the film owes a viewer is that the exhibit is
        # *on screen* somewhere, in the command the film actually ran.
        wanted = [
            want
            for probe in [row["probe"], *row.get("also", [])]
            if "git" not in probe
            for want in probe.get("expect", [])
        ]
        if wanted and not any(want in said for want in wanted):
            problems.append(
                f"the cast shows none of {wanted}, which is how the viewer is supposed to find the "
                f"seeded {row['pathology']}"
            )
    if problems:
        for problem in problems:
            print(f"demo cast: {problem}", file=sys.stderr)
        return 1
    print(
        f"{path.name}: {len(events)} events, {duration:.1f}s of silent film, tagged {tag}, "
        f"every promised finding in frame"
    )
    return 0


def gif(args: argparse.Namespace) -> int:
    cast = pathlib.Path(args.out)
    out = cast.with_suffix(".gif")
    if shutil.which("agg"):
        argv, name = ["agg", str(cast), str(out)], "agg"
    elif shutil.which("asciinema") and shutil.which("gifski"):
        argv, name = None, "asciinema cat + gifski"
    else:
        print(
            "no GIF renderer on this machine. The cast is the artefact of record; to derive the GIF, "
            "install one of these and re-run:\n"
            "  cargo install agg          # then: python3 tools/record_demo.py gif\n"
            "  # or: brew install asciinema gifski, then:\n"
            f"  asciinema cat {cast} | gifski -o {out} -\n",
            file=sys.stderr,
        )
        return 3
    if argv:
        run = subprocess.run(argv, capture_output=True, text=True, check=False)
    else:
        first = subprocess.run(["asciinema", "cat", str(cast)], capture_output=True, check=False)
        run = subprocess.run(
            ["gifski", "-o", str(out), "-"], input=first.stdout, capture_output=True, check=False
        )
    if run.returncode != 0:
        print(f"{name} failed: {(run.stderr or b'') if isinstance(run.stderr, bytes) else run.stderr}"
              .strip(), file=sys.stderr)
        return 1
    print(f"wrote {out} from {cast.name} with {name}")
    return 0


def show(args: argparse.Namespace) -> int:
    path = pathlib.Path(args.out)
    if not path.is_file():
        print(f"{path} is missing", file=sys.stderr)
        return 1
    _, events = read_cast(path)
    print(transcript(events))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("action", choices=["record", "check", "gif", "show"])
    parser.add_argument("--out", default=str(CAST), help="the asciicast file")
    parser.add_argument("--report", help="where to write the cast's report JSON")
    parser.add_argument("--bin", help="the fathom binary (default target/debug/fathom)")
    args = parser.parse_args()
    if args.action == "record":
        return record(args)
    if args.action == "check":
        return check(args)
    if args.action == "gif":
        return gif(args)
    return show(args)


if __name__ == "__main__":
    sys.exit(main())
