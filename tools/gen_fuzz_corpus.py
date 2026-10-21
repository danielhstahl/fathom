#!/usr/bin/env python3
"""Write the seed corpus the fuzz targets start from (`e7.8`, `reco-41a`).

Committed, generated, and re-runnable: `python3 tools/gen_fuzz_corpus.py write`, and
`python3 tools/gen_fuzz_corpus.py check` fails if the directory and this file disagree — the same rule the
documentation pages follow, because a corpus that has drifted from its generator is a corpus nobody can
explain.

Three target directories, matching `fuzz/fuzz_targets/`:

| directory | seed format | why that format |
|---|---|---|
| `fuzz/corpus/lockfile` | `<file-name>\0<content>` | a seed has to name the file it is pretending to be, or the recognizer waves it away and the parsers are never reached |
| `fuzz/corpus/paths` | `path\0path\0…` | the path readers take a list of names, so one seed tries several |
| `fuzz/corpus/hooks` | raw `.fathom/events.jsonl` bytes | the reader's unit is a line of JSON, so a crash artefact is a valid event file |

The seeds are *shapes*, not realistic files: nesting, repetition, one enormous token, separators where a
name should be, and the path spellings a clone would use. A corpus of plausible lockfiles is already what
`crates/fathom-analyze/tests/golden/deps/` is for.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CORPUS = ROOT / "fuzz" / "corpus"


def lockfile_seeds() -> dict[str, bytes]:
    """Every lockfile/manifest shape worth starting a fuzz run with."""
    out: dict[str, bytes] = {}

    def put(name: str, tag: str, content: bytes) -> None:
        out[f"lockfile/{name}__{tag}"] = name.encode() + b"\x00" + content

    # --- nesting, which is what a recursive reader loses a stack over -------------------------------
    put("pnpm-lock.yaml", "deep-mapping",
        b"".join(f"{' ' * (depth % 200)}k{depth}:\n".encode() for depth in range(2_000)))
    put("pnpm-lock.yaml", "deep-sequence",
        b"".join(f"{' ' * (depth % 200)}- k{depth}:\n".encode() for depth in range(2_000)))
    put("pnpm-lock.yaml", "shallow-real",
        b"lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: true\nimporters:\n  .:\n"
        b"    dependencies:\n      left-pad:\n        specifier: ^1.3.0\n        version: 1.3.0\n")
    put("pnpm-workspace.yaml", "wide",
        b"packages:\n" + b"".join(f"  - packages/{i}\n".encode() for i in range(500)))
    put("pom.xml", "deep-elements",
        b"<project>" + b"<a>" * 2_000 + b"</a>" * 2_000 + b"</project>")
    put("pom.xml", "unclosed", b"<project>" * 2_000)
    put("pom.xml", "doctype-ignored",
        b'<!DOCTYPE project [<!ENTITY x SYSTEM "file:///etc/passwd">]>\n'
        b"<project><modelVersion>4.0.0</modelVersion></project>")
    put("Fathom.csproj", "deep-items",
        b'<Project>' + b'<ItemGroup>' * 900 + b'</ItemGroup>' * 900 + b'</Project>')
    put("package-lock.json", "deep-array",
        b"[" * 8_000 + b"x" + b"]" * 8_000)
    put("package-lock.json", "deep-object",
        b'{"a":' * 8_000 + b"x" + b"}" * 8_000)
    put("package.json", "real",
        b'{"name": "x", "version": "1.0.0", "dependencies": {"left-pad": "^1.3.0"}}')
    put("Cargo.lock", "deep-array", b"[" * 8_000 + b"x" + b"]" * 8_000)
    put("Cargo.toml", "deep-array", b"[" * 8_000 + b"x" + b"]" * 8_000)
    put("Cargo.lock", "real",
        b'# Cargo Lock File Format v4\nversion = 4\n\n[[package]]\nname = "left-pad"\n'
        b'version = "1.3.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n')
    put("pyproject.toml", "real",
        b'[project]\nname = "x"\nversion = "1"\ndependencies = ["left-pad==1.3.0"]\n')
    put("uv.lock", "real", b'version = 1\n\n[[package]]\nname = "left-pad"\nversion = "1.3.0"\n')
    put("poetry.lock", "real", b'[[package]]\nname = "left-pad"\nversion = "1.3.0"\n')

    # --- one enormous token or line, which is what a line-oriented reader chokes on -----------------
    put("Cargo.lock", "one-long-line", b'key = "' + b"z" * 200_000 + b'"')
    put("yarn.lock", "separator-storm", b"=" * 200_000 + b"\n")
    put("gradle.lockfile", "separator-storm", b"=" * 200_000 + b"\n")
    put("go.sum", "dots", b"a" * 400 + b"." + b"b" * 400)
    put("requirements.txt", "dots", b"a" * 400 + b"." + b"b" * 400)
    put("Gemfile.lock", "real",
        b"GEM\n  remote: https://rubygems.org/\n  specs:\n    left-pad (1.3.0)\n\nPLATFORMS\n  ruby\n\n"
        b"DEPENDENCIES\n  left-pad\n\nBUNDLED WITH\n   2.4.10\n")
    put("mix.lock", "braces", b'{"left_pad", "hex", "1.3.0", ' * 200 + b"}")
    put("packages.lock.json", "empty-object", b"{}")
    put("go.mod", "real", b"module example.test/x\n\ngo 1.24\n\nrequire example.test/y v1.2.3\n")
    put("go.sum", "real", b"example.test/y v1.2.3 h1:abc=  h1:def=\n")
    put("build.gradle", "real",
        b'requirements("example.test:y:1.2.3")\n' * 100)
    put("settings.gradle", "dots", b"." * 1_000)

    # --- names that are not names, paired with content ----------------------------------------------
    for weird in ["../Cargo.lock", "/etc/passwd", "~/.ssh/id_rsa", "a/../b/Cargo.lock",
                  "Cargo.lock\u0000x", "\u00e9\u00e8.lock", "x" * 300 + ".toml"]:
        put_weird = weird.encode("utf-8", "surrogateescape")
        out[f"lockfile/name-{hashlib.sha256(put_weird).hexdigest()[:12]}"] = (
            put_weird + b"\x00" + b"version = 4\n")

    return out


def path_seeds() -> dict[str, bytes]:
    """Path-shaped seeds for the name readers."""
    groups = [
        ["Cargo.lock", "package-lock.json", "pnpm-lock.yaml", "pom.xml"],
        ["../../../../etc/passwd", "/etc/passwd", "~/.ssh/authorized_keys"],
        ["src/../Cargo.lock", "./Cargo.lock", "Cargo.lock/../../etc/shadow"],
        ["a\0b", "\u0000", "\u0001x", ""],
        ["", "/", "//", "///"],
        ["d/" * 200 + "Cargo.lock", "x" * 500, "." * 400],
        ["**/*", "[a-", "{,}", "?"],
        ["C:\\Users\\taylor\\Cargo.toml", "C:/Windows/win.ini"],
        ["\u00e9\u00e8/\u4e2d\u6587.lock", "\U0001F600/package-lock.json"],
        ["\u0001\u0002\u0003"],
    ]
    out = {}
    for index, group in enumerate(groups):
        joined = b"\0".join(item.encode("utf-8", "surrogateescape") for item in group)
        out[f"paths/group-{index:02d}"] = joined
    return out


def hook_seeds() -> dict[str, bytes]:
    """Event-stream seeds for the hook reader."""
    real = {
        "fathom_event_schema": 1, "seq": 1, "event_id": "e1", "event": "turn_start",
        "session_id": "s1", "turn_index": 1, "ts": "2026-10-21T04:00:00Z",
        "cwd": "/srv/work/repo", "detected_by": "harness",
    }
    hostile_paths = {
        "fathom_event_schema": 1, "seq": 2, "event_id": "e2", "event": "file_edit",
        "session_id": "s1", "turn_index": 1, "ts": "2026-10-21T04:00:01Z",
        "cwd": "/srv/work/repo",
        "files": ["../../../../etc/passwd", "/etc/shadow", "~/.ssh/authorized_keys",
                  "src/../../root/.bashrc", "a/../b/c.txt", "\u0000nul"],
    }
    hostile_command = {
        "fathom_event_schema": 1, "seq": 3, "event_id": "e3", "event": "test_run",
        "session_id": "s1", "turn_index": 1, "ts": "2026-10-21T04:00:02Z",
        "test_run": {"command": "cargo test; touch /tmp/pwned", "status": "passed",
                     "exit_code": 0},
    }
    deep = {"fathom_event_schema": 1, "seq": 4, "event": "turn_end",
            "payload": json.loads("[" * 200 + "]" * 200)}
    wrong_types = {"fathom_event_schema": "one", "seq": "2", "event": 7, "turn_index": -3}
    seeds = {
        "hooks/real-turn": b"\n".join([json.dumps(real).encode(), b"",
                                       json.dumps(hostile_paths).encode(), b"",
                                       json.dumps(hostile_command).encode(), b""]),
        "hooks/hostile-paths": (json.dumps(hostile_paths).encode() + b"\n"),
        "hooks/hostile-command": (json.dumps(hostile_command).encode() + b"\n"),
        "hooks/deep-payload": (json.dumps(deep).encode() + b"\n"),
        "hooks/wrong-types": (json.dumps(wrong_types).encode() + b"\n"),
        "hooks/torn-line": b'{"fathom_event_schema": 1, "seq": 9, "eve',
        "hooks/not-json": b"this is not json at all\n" + bytes([0xC3, 0xFF, 0xFE]) + b"\n",
        "hooks/huge-field": (b'{"fathom_event_schema": 1, "event": "turn_start", "blob": "'
                             + b"y" * 200_000 + b'"}\n'),
        "hooks/empty": b"",
        "hooks/binary": bytes(range(256)),
    }
    return seeds


def build() -> dict[str, bytes]:
    out = {}
    out.update(lockfile_seeds())
    out.update(path_seeds())
    out.update(hook_seeds())
    return out


def main(argv: list[str]) -> int:
    mode = argv[1] if len(argv) > 1 else "check"
    seeds = build()
    if mode == "write":
        for name, payload in sorted(seeds.items()):
            path = CORPUS / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
        print(f"wrote {len(seeds)} seeds under {CORPUS.relative_to(ROOT)}")
        return 0
    stale: list[str] = []
    have: set[str] = set()
    for path in sorted(CORPUS.rglob("*")) if CORPUS.exists() else []:
        if path.is_file():
            have.add(str(path.relative_to(CORPUS)))
    for name, payload in seeds.items():
        path = CORPUS / name
        if not path.exists():
            stale.append(f"missing: {name}")
        elif path.read_bytes() != payload:
            stale.append(f"differs from the generator: {name}")
    for extra in sorted(have - set(seeds)):
        stale.append(f"not written by the generator: {extra} (a crash artefact belongs in tests, "
                     "not here — add it as a regression test and keep the corpus reproducible)")
    if stale:
        for line in stale:
            print(line)
        print(f"\n{len(stale)} problem(s); run `python3 tools/gen_fuzz_corpus.py write`")
        return 1
    print(f"fuzz corpus OK ({len(seeds)} seeds)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
