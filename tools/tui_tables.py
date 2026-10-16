#!/usr/bin/env python3
"""Render the Rust TUI's parity tables from the Python tables that own them.

`crates/fathom-tui/tests/tables/mod.rs` is a copy of `src/recommender/tui/{keymap,views}.py` in Rust
literals. ADR-006 §2.1 makes those Python tables the normative ones — the Rust port is the thing
under test — so the test file has to say what the normative side says *today*, in a form a reviewer
can diff. Writing them by hand is how a port quietly diverges: the divergence is found when somebody
trusts the test.

    python3 tools/tui_tables.py > crates/fathom-tui/tests/tables/mod.rs
    python3 tools/tui_tables.py check   # exit 1 if the checked-in file has drifted

`check` is what `tests/tables.rs` promises in its own doc comment; the Rust tests compare these
constants against `fathom_tui::keymap` / `fathom_tui::views` at runtime, so the chain is
Python tables → this file → Rust tables → Rust behaviour, and every link is checked somewhere.
"""
from __future__ import annotations

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from recommender.tui import keymap, views  # noqa: E402

OUT = ROOT / "crates" / "fathom-tui" / "tests" / "tables" / "mod.rs"

#: The Python `Pane.kind` spellings, in the Rust enum they become.
KIND = {"body": "Body", "canvas": "Canvas", "field": "Field", "list": "List",
        "table": "Table", "treemap": "Treemap"}

#: The Python `Pane.role` spellings, in the Rust enum they become.
ROLE = {"primary": "Primary", "query": "Query", "secondary": "Secondary"}

#: `Mode(name="list")` → `Mode::List`. Same words, Rust's casing convention.
MODE = {m.name: m.name[:1].upper() + m.name[1:] for m in keymap.MODES}


def _q(text: str) -> str:
    """A Rust string literal, with `\\` and `"` escaped."""
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _strs(items: list[str] | tuple[str, ...]) -> str:
    return "&[" + ", ".join(_q(item) for item in items) + "]"


def _modes(modes: tuple[str, ...]) -> str:
    return "&[" + ", ".join(f"Mode::{MODE[mode]}" for mode in modes) + "]"


def render() -> str:
    """The whole `tests/tables.rs`, as text."""
    out: list[str] = [
        "//! The normative tables, copied out of the Python source that owns them.",
        "//!",
        "//! These constants are not the Rust implementation restated as a test. They are what",
        "//! `src/recommender/tui/{keymap,views}.py` publishes, in Rust literals, so that changing a",
        "//! table has to be a deliberate act in two places (ADR-006 §2.1). Regenerate with:",
        "//!",
        "//! ```text",
        "//! python3 tools/tui_tables.py > crates/fathom-tui/tests/tables/mod.rs",
        "//! ```",
        "//!",
        "//! `python3 tools/tui_tables.py check` fails when this file no longer matches the tables,",
        "//! the same way `tools/check_docs.py` fails when the ADR's generated blocks drift.",
        "",
        # An inner attribute, so it has to come before the first item but may follow the `//!` block.
        "#![allow(dead_code)] // not every test file reads every table",
        "#![allow(clippy::type_complexity)] // these are table literals, not an API",
        "use fathom_tui::keymap::Mode;",
        "",
        "/// Every action: id, palette label, scope, owning ticket, bindable modes, palette flag.",
        "#[rustfmt::skip]\n#[allow(clippy::type_complexity)]\n"
        "pub const EXPECTED_ACTIONS: &[(&str, &str, &str, &str, &[Mode], bool)] = &[",
    ]
    for action in keymap.ACTIONS.values():
        out.append(
            f"    ({_q(action.id)}, {_q(action.title)}, {_q(action.scope)}, {_q(action.ticket)}, "
            f"{_modes(action.modes)}, {str(action.palette).lower()}),"
        )
    out.append("];")

    out += [
        "",
        "/// The default binding table, chord by chord, exactly as `keymap.DEFAULTS` reads.",
        "#[rustfmt::skip]\npub const EXPECTED_DEFAULT_BINDINGS: &[(&str, &str, &str)] = &[",
    ]
    for mode, binds in keymap.DEFAULTS.items():
        for chord, action in binds.items():
            out.append(f"    ({_q(mode)}, {_q(chord)}, {_q(action)}),")
    out.append("];")

    out += [
        "",
        "/// Every pane with the numbers the layout ladder is built from.",
        "#[rustfmt::skip]\n#[allow(clippy::type_complexity)]\npub const EXPECTED_PANES: "
        "&[(&str, &str, &str, u8, &str, u16, u16, u16, u16, u16, bool, &str, &[&str])] = &[",
    ]
    for view in views.VIEWS:
        for pane in view.panes:
            out.append(
                f"    ({_q(view.id)}, {_q(pane.name)}, {_q(ROLE[pane.role])}, {pane.focus_order}, "
                f"{_q(KIND[pane.kind])}, {pane.ideal_cols}, {pane.min_cols}, {pane.collapse_to}, "
                f"{pane.hard_min}, {pane.trim_rows}, {str(pane.strip).lower()}, "
                f"{_q(pane.ticket)}, {_strs(pane.reads)}),"
            )
    out.append("];")

    out += [
        "",
        "/// The view registry: id, global key, title, kind, owner, ideal size, floors, chrome.",
        "#[rustfmt::skip]\npub const EXPECTED_VIEWS: "
        "&[(&str, &str, &str, &str, &str, u16, u16, u16, u16, &[&str])] = &[",
    ]
    for view in views.VIEWS:
        out.append(
            f"    ({_q(view.id)}, {_q(view.key)}, {_q(view.title)}, {_q(view.kind)}, "
            f"{_q(view.owner)}, {view.ideal_cols}, {view.ideal_rows}, {view.min_rows}, "
            f"{view.min_content_rows}, {_strs(sorted(view.chrome))}),"
        )
    out.append("];")

    out += [
        "",
        "/// The named drill edges, from ADR-006 §5's per-view tables.",
        "#[rustfmt::skip]\npub const EXPECTED_DRILL: &[(&str, &[(&str, &str, &str)])] = &[",
    ]
    for view in views.VIEWS:
        triples = ", ".join(
            f"({_q(a)}, {_q(b)}, {_q(c)})" for a, b, c in view.drills
        )
        out.append(f"    ({_q(view.id)}, &[{triples}]),")
    out.append("];")

    out += [
        "",
        "/// The three canonical tasks the help overlay is organised around (ADR-006 §9).",
        "#[rustfmt::skip]\n#[allow(clippy::type_complexity)]\n"
        "pub const EXPECTED_TASKS: &[(&str, &str, &str, &[(&str, &str, &str)])] = &[",
    ]
    for task in keymap.TASKS:
        steps = ", ".join(
            f"({_q(s.key)}, {_q(s.action)}, {_q(s.effect)})" for s in task.steps
        )
        out.append(f"    ({_q(task.id)}, {_q(task.name)}, {_q(task.asks)}, &[{steps}]),")
    out.append("];")

    out += [
        "",
        "/// Chords the terminal cannot deliver distinctly, and what each one collides with",
        "/// (ADR-006 §8.1; the §14.1 keyprobe is where these were measured).",
        "#[rustfmt::skip]\npub const EXPECTED_UNUSABLE: &[(&str, &str, &str)] = &[",
    ]
    for chord, delivers, why in keymap.TERMINAL_UNUSABLE:
        out.append(f"    ({_q(chord)}, {_q(delivers)}, {_q(why)}),")
    out.append("];")

    out += [
        "",
        "/// The global view keys, which are also the tab bar's order.",
        "#[rustfmt::skip]\npub const EXPECTED_VIEW_KEYS: &[(&str, &str)] = &[",
    ]
    for key, view in views.VIEW_KEYS.items():
        out.append(f"    ({_q(key)}, {_q(view)}),")
    out.append("];")

    return "\n".join(out) + "\n"


def main() -> int:
    """Write the tables, or check the checked-in copy against them."""
    if len(sys.argv) > 1 and sys.argv[1] == "check":
        if not OUT.exists():
            print(f"{OUT} does not exist; run `tools/tui_tables.py`", file=sys.stderr)
            return 1
        if OUT.read_text() != render():
            print(
                "crates/fathom-tui/tests/tables/mod.rs has drifted from the Python tables;\n"
                "run `python3 tools/tui_tables.py > crates/fathom-tui/tests/tables/mod.rs`",
                file=sys.stderr,
            )
            return 1
        print("tests/tables/mod.rs matches the normative tables")
        return 0
    sys.stdout.write(render())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
