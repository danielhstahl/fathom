#!/usr/bin/env python3
"""Drive a real `fathom tui` over a pty and record the session as an asciinema cast.

This is the harness `e1.12` (`reco-glv`) is made of: a scripted reviewer who never touches the
mouse, never leaves the TUI, and whose keystrokes and timings are written down as data. It is a
*player*, not a test — it asserts nothing about the frames it sees — but it is the only honest way
to produce the walkthrough the acceptance criteria ask for, because it drives the shipped binary
through a real terminal (alt screen, raw mode, redraws, the lot) rather than a mock frame.

Usage:

    python3 tools/tui_session.py --root /tmp/harbour --script docs/demo/mvp-500-commit-review.json \
        --cast docs/demo/mvp-500-commit-review.cast --log /tmp/walk.log

The script file is a JSON document:

    {"title": "...", "steps": [
        {"keys": "t", "note": "open the timeline", "settle": 0.6},
        {"keys": "jjj", "note": "move down", "repeat": 3},
        {"sleep": 2.0, "note": "let the watcher notice a commit made outside"},
        {"shell": "git -C ... commit ...", "note": "the rebase, done outside the TUI on purpose"}
    ]}

`keys` is sent to the terminal verbatim, with `<Enter>`, `<C-c>`, `<Space>`, `<Esc>`, `<Tab>`
spelled out the way the keymap document spells them. `shell` runs a command *between* keystrokes —
that is how the reviewer performs the "do a rebase midway" step, which by definition happens in
another process.

Output:

  * `--cast`  an asciicast v2 file: the byte stream the terminal received, with the delay before
              each chunk. Play it back with `asciinema play` or asciinema.org.
  * `--log`   the same bytes with a caption line per step, as plain text (screen-scrapeable).
  * `--steps` per-step wall-clock timings, so "time to first orientation" is a measured number
              rather than a claim in a document. A `shell` step also reports `shell_ms` — how long the
              outside command itself took — so a step that commits and then waits for the TUI to notice
              can be read as a detection latency rather than as a commit plus one (`reco-bpc`).

Nothing here is product code: no store, no repository, no frame knowledge. The TUI is a subprocess
behaving exactly as it does for a person.
"""

from __future__ import annotations

import argparse
import json
import os
import pty
import re
import select
import shutil
import subprocess
import sys
import time
import unicodedata
import struct  # noqa: F401 - the winsize pack lives in spawn()
from pathlib import Path

# Key names, spelled the way `src/recommender/tui/keymap.py` spells them, become bytes.
KEY_BYTES = {
    "Enter": b"\r",
    "Space": b" ",
    "Esc": b"\x1b",
    "Tab": b"\t",
    "BS": b"\x7f",
    "Up": b"\x1b[A",
    "Down": b"\x1b[B",
    "Right": b"\x1b[C",
    "Left": b"\x1b[D",
    "PageUp": b"\x1b[5~",
    "PageDown": b"\x1b[6~",
    "Home": b"\x1b[H",
    "End": b"\x1b[F",
    "Backspace": b"\x7f",
    "F1": b"\x1bOP",
    "F2": b"\x1bOQ",
}

TOKEN = re.compile(r"<([A-Za-z-]+)>|(.)", re.S)


def encode(keys: str) -> bytes:
    """Turn `<C-d>jj<Enter>` into bytes, one keystroke per token."""
    out = bytearray()
    pos = 0
    while pos < len(keys):
        match = TOKEN.match(keys, pos)
        if match is None:  # pragma: no cover - the pattern matches any single character
            raise SystemExit(f"cannot encode {keys[pos]!r}")
        named, literal = match.group(1), match.group(2)
        if literal is not None:
            out.extend(literal.encode())
        elif named in KEY_BYTES:
            out.extend(KEY_BYTES[named])
        elif len(named) == 1:
            out.extend(named.encode())
        elif "-" in named:
            head, _, last = named.rpartition("-")
            if head.lower() == "s-c" or head.lower() == "c":
                # <C-w>l is a chord: two keystrokes, no delay between them.
                out.extend(ctrl_bytes(head, last))
            else:
                raise SystemExit(f"unsupported key <{named}>")
        else:
            raise SystemExit(f"unsupported key <{named}>")
        pos = match.end()
    return bytes(out)


def ctrl_bytes(modifier: str, last: str) -> bytes:
    """The bytes for a `<C-x>`-style chord."""
    if not last:
        raise SystemExit("empty chord")
    if modifier.lower() == "c":
        upper = last.upper()
        if upper.isalpha():
            # Ctrl+letter is the letter minus 0x40; shift-state does not change the byte crossterm sees.
            return bytes([ord(upper) - 0x40])
        if upper.isdigit():
            # Ctrl+2..9 map to 0x00..0x07 the way terminals do it; Ctrl+1 is ESC in most emulators.
            return bytes([(ord(upper) - 0x31) % 8])
        if upper == "[":
            return b"\x1b"
        if upper == "]":
            return b"\x1d"
        raise SystemExit(f"unsupported control key <C-{last}>")
    if modifier.lower() == "s":
        return last.encode()
    raise SystemExit(f"unsupported modifier <{modifier}-...>")


class Screen:
    """A minimal VT screen: enough of the escape language to say what the frame showed.

    The cast is the record, but a cast is not quotable in a markdown file. This is the half that
    makes the walkthrough citable: it tracks cursor position, erase and scroll, and can print the
    grid after each step, which is what `e1.12`'s findings quote verbatim.
    """

    def __init__(self, width: int, height: int) -> None:
        self.width = width
        self.height = height
        self.grid = [[" "] * width for _ in range(height)]
        self.row = 0
        self.col = 0
        self.buffer = ""
        self.in_escape = False

    def feed(self, data: bytes) -> None:
        for char in data.decode("utf-8", "replace"):
            if self.in_escape:
                self.buffer += char
                if self.buffer == "\x1b":
                    continue
                if self.buffer[1] in "()]":
                    # OSC / DCS / APC: swallow to BEL or ST (ESC \).
                    if char == "\x07" or self.buffer.endswith("\x1b\\") or len(self.buffer) > 80:
                        self.buffer = ""
                        self.in_escape = False
                    continue
                if len(self.buffer) > 2 and self.buffer[1] == "[" and 0x40 <= ord(char) <= 0x7E:
                    # A CSI's final byte is 0x40..0x7E; everything before it is a parameter byte.
                    if self.buffer.startswith("\x1b["):
                        self.csi(self.buffer[2:])
                    self.buffer = ""
                    self.in_escape = False
                elif len(self.buffer) > 24:
                    self.buffer = ""
                    self.in_escape = False
                continue
            if char == "\x1b":
                self.buffer = char
                self.in_escape = True
            elif char == "\r":
                self.col = 0
            elif char == "\n":
                self.row = min(self.height - 1, self.row + 1)
            elif char == "\x08":
                self.col = max(0, self.col - 1)
            elif char == "\x07":
                pass
            elif char >= " ":
                self.put(char)

    def put(self, char: str) -> None:
        if unicodedata.combining(char):
            return
        width = 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
        if self.row < self.height and self.col < self.width:
            self.grid[self.row][self.col] = char
            if width == 2 and self.col + 1 < self.width:
                self.grid[self.row][self.col + 1] = ""
        self.col += width
        if self.col >= self.width:
            self.col = self.width - 1

    def csi(self, body: str) -> None:
        if not body:
            return
        final = body[-1]
        raw = body[:-1]
        params = [int(part) if part.isdigit() else 0 for part in raw.replace(";", " ").split()]
        first = params[0] if params else 0
        second = params[1] if len(params) > 1 else 0
        if final in "Hf":
            self.row = max(0, (first or 1) - 1)
            self.col = max(0, (second or 1) - 1)
        elif final == "A":
            self.row = max(0, self.row - (first or 1))
        elif final == "B":
            self.row = min(self.height - 1, self.row + (first or 1))
        elif final == "C":
            self.col = min(self.width - 1, self.col + (first or 1))
        elif final == "D":
            self.col = max(0, self.col - (first or 1))
        elif final == "J":
            start = 0 if first in (2, 3) else self.row
            stop = self.height if first in (2, 3) else self.height
            for row in range(start, min(stop, self.height)):
                self.grid[row] = [" "] * self.width
        elif final == "K":
            row = self.grid[self.row] if self.row < self.height else None
            if row is not None:
                start = 0 if first == 1 else self.col
                stop = self.col + 1 if first == 2 else self.width
                for col in range(start, min(stop, self.width)):
                    row[col] = " "
        elif final == "S":
            del self.grid[: first or 1]
            self.grid.extend([[" "] * self.width for _ in range(first or 1)])
        elif final == "U":
            self.grid = [[" "] * self.width for _ in range(first or 1)] + self.grid[: self.height - (first or 1)]

    def text(self) -> str:
        lines = ["".join(row).rstrip() for row in self.grid]
        while lines and not lines[-1].strip():
            lines.pop()
        return "\n".join(lines)


class Recorder:
    """Collects (delay, bytes) pairs and can write them as asciicast v2."""

    def __init__(self) -> None:
        self.chunks: list[list] = []
        self.started = time.monotonic()
        self.last = self.started

    def add(self, data: bytes) -> None:
        now = time.monotonic()
        if data:
            self.chunks.append([round(now - self.last, 6), data.decode("utf-8", "replace")])
        self.last = now

    def write_cast(self, path: Path, width: int, height: int, title: str, command: str) -> None:
        header = {
            "version": 2,
            "width": width,
            "height": height,
            "idle_time_limit": 4.0,
            "timestamp": int(time.time()),
            "title": title,
            "env": {"SHELL": "/bin/sh", "TERM": os.environ.get("TERM", "xterm-256color")},
        }
        with path.open("w", encoding="utf-8") as handle:
            handle.write(json.dumps(header, separators=(",", ":")) + "\n")
            for delay, text in self.chunks:
                handle.write(json.dumps([round(delay, 6), "o", text], ensure_ascii=False) + "\n")
        _ = command

    def raw_text(self) -> str:
        return "".join(text for _, text in self.chunks)


def drain(master: int, deadline: float, recorder: Recorder, raw, screen: Screen,
          quiet: bool = True, until: str | None = None,
          capture: dict | None = None) -> tuple:
    """Read whatever the terminal produced until it goes quiet or the deadline passes.

    Returns `(first, last, changed, found)`: when the first byte of the answer arrived, when the
    last one did, when the *screen* actually changed — a keystroke can emit a cursor sequence and
    draw nothing, so the third number is the one that means "the reviewer saw something happen" —
    and, if the step named an `until` string, when the screen first contained it. That fourth
    number is what makes "time until the first hunk is on screen" a measurement (`reco-0kw`):
    `changed` says the pane moved, `found` says it moved to the thing that was asked about.

    Pass a `capture` dict and the frame that *first* said `until` is kept in `capture["screen"]`.
    A toast holds the hint row for a fixed span of seconds, not for the length of a step
    (`TOAST_LIFETIME` in `fathom-tui/src/app.rs`), so a harness that only keeps the settled frame can
    watch an act happen and still hold a screenshot in which it did not:
    `tests/test_review_state_contract.py` needs the toast's own words, and this is the one honest way
    to hand them over — the frame as it was at the moment it said them.
    """
    first: float | None = None
    last: float | None = None
    changed: float | None = None
    found: float | None = None

    def note(now: float) -> None:
        nonlocal changed, found
        text = screen.text()
        if changed is None and text != before:
            changed = now
        if found is None and until is not None and until in text:
            found = now
            if capture is not None:
                capture["screen"] = text

    before = screen.text()
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return first, last, changed, found
        ready, _, _ = select.select([master], [], [], min(0.05, remaining))
        if not ready and not quiet:
            continue
        if not ready:
            # Two quiet ticks in a row means the frame settled.
            settled_ticks = 0
            while settled_ticks < 2:
                ready, _, _ = select.select([master], [], [], 0.05)
                if ready:
                    settled_ticks = 0
                    try:
                        data = os.read(master, 65536)
                    except OSError:
                        return first, last, changed, found
                    if not data:
                        # EOF: the child is gone (its last slave fd closed). Some kernels report this
                        # as a readable-then-empty master rather than EIO, and a loop that only
                        # watches for EIO spins forever on a hangup that never raises.
                        return first, last, changed, found
                    now = time.monotonic()
                    first = first or now
                    last = now
                    recorder.add(data)
                    screen.feed(data)
                    note(now)
                    if raw:
                        raw.write(data)
                        raw.flush()
                else:
                    settled_ticks += 1
            return first, last, changed, found
        try:
            data = os.read(master, 65536)
        except OSError:
            return first, last, changed, found
        if not data:
            # EOF on the master: the child exited; a select loop that only stops on EIO would spin.
            return first, last, changed, found
        now = time.monotonic()
        first = first or now
        last = now
        recorder.add(data)
        screen.feed(data)
        note(now)
        if raw:
            raw.write(data)
            raw.flush()


def child_rss_kib(pid: int) -> int | None:
    """Peak resident memory of the TUI child, from `/proc` (absent elsewhere, and then unknown)."""
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmHWM:"):
                return int(line.split()[1])
    except OSError:
        return None
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", required=True, help="repository to open the TUI in")
    parser.add_argument("--script", required=True, help="JSON step script")
    parser.add_argument("--binary", default=None, help="fathom binary (default: target/release/fathom)")
    parser.add_argument("--cast", default=None, help="write an asciicast v2 file here")
    parser.add_argument("--log", default=None, help="write the raw terminal bytes here")
    parser.add_argument("--steps", default=None, help="write per-step timings as JSON here")
    parser.add_argument("--screens", default=None, help="write the screen after every step as text here")
    parser.add_argument("--width", type=int, default=132)
    parser.add_argument("--height", type=int, default=40)
    parser.add_argument("--settle", type=float, default=0.45, help="seconds to let each step draw")
    parser.add_argument("--key-delay", type=float, default=0.09, help="seconds between keystrokes")
    parser.add_argument("--no-alt-screen", action="store_true", help="pass --no-alt-screen to fathom")
    args = parser.parse_args()

    binary = args.binary or str(Path(__file__).resolve().parents[1] / "target/release/fathom")
    if not Path(binary).exists():
        raise SystemExit(f"no fathom binary at {binary}; build it first")
    # A `shell` step is another terminal, and a real reviewer's shell knows where the binary it ran
    # lives. Export it so a scripted walkthrough never hard-codes a path.
    os.environ["FATHOM"] = str(Path(binary).resolve())
    script = json.loads(Path(args.script).read_text(encoding="utf-8"))
    steps = script["steps"]

    raw = open(args.log, "wb") if args.log else None  # noqa: SIM115 - closed in finally
    recorder = Recorder()
    screen = Screen(args.width, args.height)
    timings: list[dict] = []
    #: step index → (`until`, the frame that first said it), for `--screens`. See `drain`.
    needle_screens: dict[int, tuple[str, str]] = {}
    command_output: list[dict] = []
    first_paint_ms: float | None = None
    shell_ms: float | None = None
    command = [binary, "tui"]
    if args.no_alt_screen:
        command.append("--no-alt-screen")

    def spawn() -> tuple:
        """Start `fathom tui` on a fresh pty and hand back (pid, master).

        A walkthrough sometimes has to leave the TUI and come back — the rebase in step (d) of
        `e1.12` is done by another process, and the reviewer quits and reopens to see whether the
        session survived. The cast keeps running across the restart, so the record is one film.
        """
        pid, master = pty.fork()
        if pid == 0:  # child: become the TUI, in the repository under review
            try:
                os.chdir(args.root)
                env = dict(os.environ)
                env["TERM"] = os.environ.get("TERM", "xterm-256color")
                env.setdefault("NO_COLOR", "")
                exec_file = shutil.which(binary) or binary
                os.execve(exec_file, command, env)
            except Exception as exc:  # pragma: no cover - the child cannot report more
                print(f"exec failed: {exc}", file=sys.stderr)
                os._exit(127)
        import fcntl
        import termios

        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", args.height, args.width, 0, 0))
        return pid, master

    opened = time.monotonic()
    pid, master = spawn()

    try:
        # Let the shell take the screen before the first keystroke.
        opened_at, settled, _first_change, _found = drain(
            master, time.monotonic() + 1.2, recorder, raw, screen)
        first_paint_ms = round((opened_at - opened) * 1000, 1) if opened_at else None
        _ = settled
        for index, step in enumerate(steps, start=1):
            began = time.monotonic()
            shell_ms = None
            note = step.get("note", "")
            if raw:
                raw.write(b"\n\n=== step %d: %s ===\n" % (index, note.encode()))
                raw.flush()
            if "shell" in step:
                began_shell = time.monotonic()
                finished = subprocess.run(step["shell"], shell=True, check=False, cwd=args.root,
                                          capture_output=True, text=True)
                # How long the outside command took, recorded separately: a step that commits in another
                # terminal and then waits for the TUI to notice is measuring two things, and the ticket's
                # budget is about the second one alone (`reco-bpc`). Subtracting this from `visible_ms`
                # is what turns "the screen changed 400 ms after I started committing" into a detection
                # latency.
                shell_ms = round((time.monotonic() - began_shell) * 1000, 1)
                if finished.stdout.strip():
                    command_output.append({"step": index, "stdout": finished.stdout.strip()})
                if finished.stderr.strip():
                    command_output.append({"step": index, "stderr": finished.stderr.strip()})
            if step.get("keys"):
                payload = encode(step["keys"]) * int(step.get("repeat", 1))
                per = step.get("key_delay", args.key_delay)
                chunk = int(step.get("chunk", len(payload))) or len(payload) or 1
                for start in range(0, len(payload), chunk):
                    piece = payload[start:start + chunk]
                    os.write(master, piece)
                    # The delay is *between* keystrokes, never after the last one, or every
                    # measured frame latency carries the script's own pause in it.
                    if per and len(piece) == 1 and start + chunk < len(payload):
                        time.sleep(per)
            # `settle` is a deadline, not a sleep: the frame is measured until it stops arriving,
            # which is what makes `paint_ms` the terminal's latency rather than this script's.
            # A step may also name an `until` string; `until_ms` is then the moment the screen first
            # said it — "time until the first hunk is on screen" made measurable (`reco-0kw`).
            shot: dict = {}
            needle = step.get("until")
            painted, settled, changed, found = drain(
                master, time.monotonic() + float(step.get("hold" , step.get("settle", args.settle))),
                recorder, raw, screen, quiet="hold" not in step,
                until=step.get("until"), capture=shot)
            time.sleep(float(step.get("sleep", 0.0)))
            if needle and shot.get("screen"):
                # The frame as it was the first time it said the step's `until`. Kept beside the
                # timings rather than inside them: it is a screen, not a number, and it belongs in
                # `--screens`, where the frames are.
                needle_screens[index] = (needle, shot["screen"])
            timings.append({
                "step": index,
                "note": note,
                "keys": step.get("keys"),
                "at_s": round(began - recorder.started, 3),
                "elapsed_s": round(time.monotonic() - began, 3),
                "shell_ms": shell_ms,
                "paint_ms": round((painted - began) * 1000, 1) if painted else None,
                "visible_ms": round((changed - began) * 1000, 1) if changed else None,
                "until_ms": round((found - began) * 1000, 1) if found else None,
                "settled_ms": round((settled - began) * 1000, 1) if settled else None,
                "rss_kib": child_rss_kib(pid),
                "screen": screen.text(),
            })
            if step.get("restart"):
                # Quit, do something in another process, reopen: the same cast, a new TUI. The
                # keystrokes of this step go to the old one, so the quit is really a quit and the
                # session write that quits does actually happens before the new process starts.
                try:
                    os.close(master)
                    os.waitpid(pid, 0)
                except OSError:
                    pass
                pid, master = spawn()
                screen = Screen(args.width, args.height)
                time.sleep(0.35)
        drain(master, time.monotonic() + 0.6, recorder, raw, screen)
    finally:
        try:
            os.close(master)
        except OSError:
            pass
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
        if raw:
            raw.close()

    text = recorder.raw_text()
    if args.cast:
        recorder.write_cast(Path(args.cast), args.width, args.height,
                            script.get("title", "fathom walkthrough"), " ".join(command))
    if args.steps:
        paints = [timing["paint_ms"] for timing in timings if timing["paint_ms"]]
        visible = [timing["visible_ms"] for timing in timings if timing["visible_ms"]]
        Path(args.steps).write_text(
            json.dumps({"title": script.get("title"),
                        "total_s": round(time.monotonic() - recorder.started, 3),
                        "first_paint_ms": first_paint_ms,
                        "paint_p95_ms": sorted(paints)[max(0, int(len(paints) * 0.95) - 1)] if paints else None,
                        "paint_worst_ms": max(paints) if paints else None,
                        "visible_p95_ms": sorted(visible)[max(0, int(len(visible) * 0.95) - 1)] if visible else None,
                        "visible_worst_ms": max(visible) if visible else None,
                        "visible_absent": sum(1 for t in timings if t["visible_ms"] is None),
                        "shell_output": command_output,
                        "rss_peak_kib": max((t["rss_kib"] or 0) for t in timings) if timings else None,
                        "steps": timings}, indent=1) + "\n", encoding="utf-8")
    if args.screens:
        with Path(args.screens).open("w", encoding="utf-8") as handle:
            handle.write("# every frame the reviewer saw, as text (the cast is the real record)\n")
            for timing in timings:
                handle.write("\n\n=== step %d (%s) — %.2fs — %s ===\n%s\n"
                             % (timing["step"], timing["keys"] or "-", timing["at_s"], timing["note"],
                                timing["screen"]))
                needle = needle_screens.get(timing["step"])
                if needle and needle[1] != timing["screen"]:
                    # The frame that first said the step's `until`, kept beside the settled one: a
                    # toast holds the hint row for a few seconds and is gone before the pane settles,
                    # so the settled frame alone would record an act that did not happen.
                    handle.write("\n\n=== step %d (%s) — the frame that first said %s ===\n%s\n"
                                 % (timing["step"], timing["keys"] or "-",
                                    json.dumps(needle[0]), needle[1]))

    # A short human summary: the reviewer's own timeline, with the plain-text screen scrape.
    line = "=" * 78
    print(line)
    for timing in timings:
        print("  %6.2fs  step %2d  %-14s %s" % (timing["at_s"], timing["step"],
                                                (timing["keys"] or ""), timing["note"]))
    print(line)
    paints = [timing["paint_ms"] for timing in timings if timing["paint_ms"]]
    visible = [timing["visible_ms"] for timing in timings if timing["visible_ms"]]
    if paints:
        print(f"first paint {first_paint_ms} ms · key-to-frame p95 "
              f"{sorted(paints)[max(0, int(len(paints) * 0.95) - 1)]:.0f} ms · worst {max(paints):.0f} ms")
    if visible:
        print(f"screen actually changed after {len(visible)}/{len(timings)} keys · p95 "
              f"{sorted(visible)[max(0, int(len(visible) * 0.95) - 1)]:.1f} ms · worst {max(visible):.1f} ms"
              f" · {len(timings) - len(visible)} key(s) drew nothing at all")
    print(f"cast {args.cast or '(not written)'} · {len(text):,} bytes of terminal output recorded")
    return 0


if __name__ == "__main__":
    sys.exit(main())
