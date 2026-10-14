"""Measure what a terminal can actually deliver, instead of trusting a man page.

ADR-006 §8 says two things about chords: some are *physically ambiguous* (Ctrl-I and Tab
are one byte), and the rest of what a reviewer might expect to be able to bind are
signals or flow-control characters while the driver's default flags are on. Both claims
are checkable, so they are checked here rather than asserted.

Method: open a pty pair, put the slave into one of three modes, write the byte a keyboard
would send on the master, and read what a program reading its stdin would receive.

    raw      `tty.setraw`: crossterm's raw mode, ICANON/ECHO/ISIG/IXON off
    fathom   raw **with ISIG left on** — the mode ADR-006 §8.1 commits to, because the
             exit hook, not the keymap, owns terminal restoration (`e1.5` AC)
    cooked   the pty's inherited default flags (canonical mode)

A "no byte" result for a signal key is the *point*: it means the chord cannot be bound,
because nothing ever reaches the key reader.

Run:  python3 -m recommender.tui probe [--machine]
Exit: 0 always if a pty is available; 0 with `pty_available=false` if it is not, because
      a contract that fails on a machine without `/dev/ptmx` teaches nothing.
"""

from __future__ import annotations

import os
import select
import time

CASES: tuple[tuple[str, bytes, str], ...] = (
    ("<Tab>/<C-i>", b"\x09", "one byte for two chords: they cannot be distinguished"),
    ("<Enter>/<C-m>", b"\x0d", "CR; canonical mode maps it to LF"),
    ("<C-j>", b"\x0a", "LF; identical to <Enter> once ICRNL has had its way"),
    ("<C-h>/<BS>", b"\x08", "BS here, 0x7f on other terminals — same problem"),
    ("<Esc>/<C-[>", b"\x1b", "one byte; and the prefix of every escape sequence"),
    ("<C-@>/<C-Space>", b"\x00", "NUL"),
    ("<C-c>", b"\x03", "SIGINT while ISIG is on"),
    ("<C-z>", b"\x1a", "SIGTSTP while ISIG is on"),
    ("<C-\\>", b"\x1c", "SIGQUIT while ISIG is on"),
    ("<C-s>", b"\x13", "XOFF while IXON is on"),
    ("<C-q>", b"\x11", "XON while IXON is on"),
    ("A-x (Meta x)", b"\x1bx", "ESC then 'x': indistinguishable from Esc, x"),
    ("<S-Tab>", b"\x1b[Z", "backtab, if the terminal sends it at all"),
    ("<Down>", b"\x1b[B", "arrow keys are escape sequences, not chords"),
)


def _modes(termios, tty, base):
    import termios as _t

    def raw(fd):
        tty.setraw(fd)

    def fathom(fd):
        a = _t.tcgetattr(fd)
        a[0] = 0                                   # ICRNL, IXON, ISTRIP … all off
        a[1] = a[1] & ~(_t.OPOST)                  # no output post-processing
        a[2] = a[2] | _t.CS8                       # 8-bit clean
        a[3] = _t.ISIG | _t.CS8                    # signals STILL DELIVERED
        _t.tcsetattr(fd, _t.TCSANOW, a)

    def cooked(fd):
        """Restore the pty's inherited flags: this is what a program that never touched
        termios would see — line-buffered, IXON on, ISIG on."""
        _t.tcsetattr(fd, _t.TCSANOW, base)

    return {"raw": raw, "fathom": fathom, "cooked": cooked}


def probe(verbose: bool = True) -> dict:
    out: dict[str, object] = {"pty_available": False}
    try:
        import pty
        import termios
        import tty
    except Exception as exc:                                # pragma: no cover
        out["error"] = "no pty/termios on this platform: %s" % exc
        return out
    try:
        m, s = pty.openpty()
    except OSError as exc:
        out["error"] = "pty.openpty failed: %s" % exc
        return out
    out["pty_available"] = True

    # inherited default flags, read before anything is changed
    base = termios.tcgetattr(s)
    modes = _modes(termios, tty, list(base))
    out["default_iflag_ixon"] = bool(base[0] & termios.IXON)
    out["default_iflag_icrnl"] = bool(base[0] & termios.ICRNL)
    out["default_lflag_isig"] = bool(base[3] & termios.ISIG)
    out["default_lflag_icanon"] = bool(base[3] & termios.ICANON)
    out["default_cc_erase"] = _cc(base[6][termios.VERASE])
    out["default_cc_kill"] = _cc(base[6][termios.VKILL])

    results: dict[str, dict[str, str]] = {}
    try:
        for name, payload, note in CASES:
            per = {}
            for mode, setup in modes.items():
                if setup is not None:
                    setup(s)
                try:
                    os.write(m, payload)
                except OSError:
                    per[mode] = "write-failed"
                    continue
                got = b""
                deadline = time.time() + 0.08
                while time.time() < deadline:
                    r, _, _ = select.select([s], [], [], 0.01)
                    if not r:
                        continue
                    try:
                        chunk = os.read(s, 64)
                    except OSError:
                        break
                    if not chunk:
                        break
                    got += chunk
                    if len(got) >= len(payload):
                        break
                if got:
                    if got == payload:
                        per[mode] = "byte %s (unchanged)" % _hex(payload)
                    else:
                        per[mode] = "mapped to %s" % _hex(got)
                else:
                    per[mode] = "no byte (consumed by the driver)"
            results[name] = {"raw": per["raw"], "fathom": per["fathom"],
                             "cooked": per["cooked"], "note": note}
    finally:
        os.close(m)
        os.close(s)

    out["cases"] = results
    # the two claims ADR-006 makes about physical ambiguity, as booleans
    out["ctrl_i_is_tab"] = results["<Tab>/<C-i>"]["raw"].startswith("byte")
    out["ctrl_m_is_enter_after_icrnl"] = results["<Enter>/<C-m>"]["cooked"].startswith(
        "mapped")
    out["ctrl_c_is_a_signal_not_a_byte"] = results["<C-c>"]["fathom"].startswith("no byte")
    out["flow_control_consumes_ctrl_s"] = results["<C-s>"]["cooked"].startswith("no byte")
    out["meta_is_escape_plus_key"] = results["A-x (Meta x)"]["raw"] == \
        "byte %s (unchanged)" % _hex(b"\x1bx")
    if verbose:
        for name, r in results.items():
            print("%-18s raw=%-34s fathom=%-34s cooked=%s"
                  % (name, r["raw"], r["fathom"], r["cooked"]))
    return out


def _cc(value) -> str:
    """termios control characters arrive as `bytes` in Python 3."""
    if isinstance(value, int):
        return "0x%02x" % value
    return "0x%02x" % value[0] if value else "∅"


def _hex(data: bytes) -> str:
    return " ".join("0x%02x" % b for b in data) or "∅"


MACHINE_KEYS = ("pty_available", "default_iflag_ixon", "default_iflag_icrnl",
                "default_lflag_isig", "default_lflag_icanon", "default_cc_erase",
                "default_cc_kill", "ctrl_i_is_tab",
                "ctrl_m_is_enter_after_icrnl", "ctrl_c_is_a_signal_not_a_byte",
                "flow_control_consumes_ctrl_s", "meta_is_escape_plus_key")


def machine_lines() -> list[str]:
    out = probe(verbose=False)
    lines = []
    for k in MACHINE_KEYS:
        lines.append("%s=%s" % (k, str(out.get(k, "missing")).lower()))
    for name, r in out.get("cases", {}).items():
        lines.append("delivers[%s] raw=%s | fathom=%s | cooked=%s"
                     % (name, r["raw"], r["fathom"], r["cooked"]))
    return lines


if __name__ == "__main__":  # pragma: no cover
    import sys
    if "--machine" in sys.argv:
        print("\n".join(machine_lines()))
    else:
        probe(verbose=True)
