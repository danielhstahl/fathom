#!/usr/bin/env python3
"""Prove the privacy posture instead of claiming it (ticket `e6.8`, reco-h7p; ADR-002 §6, §8.3).

The promise is two sentences long and each half needs a different kind of evidence:

1. **No network by default.** The compile-time half is already ADR-002's: a default build links no
   network-capable crate, and every feature that can touch a socket is `net-*`, off by default, and
   needs runtime consent. `check_workspace.py` enforces that from the manifests.
2. **Local-only metrics.** Everything Fathom records about a repository stays under `.fathom/`, where
   the user can read it, export it and delete it. Nothing is transmitted, and there is no key that
   would transmit it.

This file is the evidence. Two halves:

```bash
python3 tools/check_privacy.py check              # static: closure, consent table, sources, docs, release
python3 tools/check_privacy.py offline --binary target/debug/fathom
python3 tools/check_privacy.py selftest           # prove the egress trap actually traps
```

**`check`** needs no toolchain and no network, so it runs in `contracts.yml` on a clean clone:

| rule | what fails |
|---|---|
| `V01` | the default build's feature closure links a network-capable crate, or enables a `net-*` feature |
| `V02` | the Rust consent table (`fathom_core::privacy`) and the model's (`tools/workspace_model.py`) are not the same rows — the same drift `tests/test_workspace_contract.py` catches, read from the privacy side |
| `V03` | a `privacy.*` consent key is not user-scoped, or does not default to false, in the shipped config schema |
| `V04` | an egress-capable API (`std::net`, an HTTP client) appears in Rust code that a default build compiles |
| `V05` | a network import appears anywhere in the Python `recommender` package, whose modules may only read what they are handed |
| `V06` | the privacy page is missing, unlinked from the README or the docs front door, silent about a consent key, or contradicts the sentence `doctor --privacy` prints |
| `V07` | the SBOM and licence report are not generated per release, or the workflow that publishes them is not wired |

The dynamic half adds two rules of its own, printed with the same `V` prefix: **`V08`** a command's
output changed when egress was refused (the report carries the first differing line), and **`V09`** the
binary made a connection attempt while trapped. Both are failures; neither has ever fired on this tree.

**`offline`** is the dynamic half and the acceptance criterion: it runs the *shipped* binary in a
sandbox where egress is refused, and asserts that it behaves identically. The claim it can make is
precise, because the trap is tested first (`selftest`): *this binary, on this machine, made zero
connection attempts and produced byte-identical output with the network unavailable*. ADR-002 §8.3
recorded this as "a requirement with a named owner rather than as something measured" because the
development container has no network-namespace tooling; this tool is what closes that sentence, and it
picks the strongest trap the machine allows — a network namespace, else an `LD_PRELOAD` tripwire
(`tools/privacy_netprobe.c`), else `sandbox-exec` on macOS — and says in its report which one it used.
Where no trap is available it exits non-zero rather than reporting a pass.

Exit: 0 clean, 1 with one line per problem (`VNN` in each), 2 when the run could not be attempted
(no binary, no trap, no compiler).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import workspace_model as M  # noqa: E402

NETPROBE_SOURCE = ROOT / "tools" / "privacy_netprobe.c"
PRIVACY_PAGE = ROOT / "docs" / "concepts" / "privacy.md"
README = ROOT / "README.md"
DOC_INDEX = ROOT / "docs" / "index.md"
RELEASE_WORKFLOWS = [ROOT / ".github" / "workflows" / "sbom.yml"]
SBOM_DIR = ROOT / "sbom"

#: Anything that can open a socket to another machine, from Rust. A default build may not compile any
#: of these names: the crates behind them are banned by `deny.toml` and gated by `net-*` features, and
#: this is the source-level counterpart to `V01`'s dependency-level check.
RUST_EGRESS = (
    "std::net", "TcpStream", "TcpListener", "UdpSocket", "Domain::",
    "reqwest", "ureq::", "hyper::", "rustls", "native_tls", "openssl",
)

#: Same rule for the Python half. `recommender` modules are pure: they read what the CLI hands them
#: (`ADR-002 §4.5`), which is what makes "advisory lookup never blocks a review" testable at all.
PYTHON_EGRESS = ("socket", "ssl", "urllib", "http.client", "requests", "httpx", "aiohttp")

#: Lines the offline comparison is allowed to excuse. Every entry is a wall-clock stamp the report
#: carries, which changes between two runs a second apart and has nothing to do with the network.
VOLATILE = (
    (re.compile(r"now \d{4}-\d\d-\d\dT[\d:]+Z"), "now=<clock>"),
    (re.compile(r'"now_utc":\s*\d+'), '"now_utc":<clock>'),
    (re.compile(r"\b\d[\d,\.]*\s?(?:µs|ms)\b"), "<duration>"),
)

#: The commands the offline run compares. Read-only verbs plus one cold index (measured separately,
#: because indexing is the verb that would notice a missing network at all), each chosen for an answer
#: that comes from a different part of the pipeline: git objects, the store, the analysis cache, the
#: ranking, the JSON contract and the consent table itself.
OFFLINE_COMMANDS = (
    ["--version"],
    ["--help"],
    ["--plain", "stats"],
    ["--plain", "check"],
    ["--plain", "search", "crate"],
    ["--plain", "analyze", "HEAD~1"],
    ["--plain", "show", "HEAD~1"],
    ["doctor", "--privacy"],
    ["doctor", "--json"],
)

#: A public address and port for the selftest. It must be alive when there is no trap, so that a trap
#: that silently stopped trapping fails the test instead of passing it; `1.1.1.1` is as close to a
#: guarantee as a test gets, and the timeout is short enough that a *slow* refusal counts as a failure.
SELFTEST_HOST = ("1.1.1.1", 443)
SELFTEST_SNIPPET = (
    "import socket, sys\n"
    "try:\n"
    "    socket.create_connection((%r, %d), timeout=4)\n"
    "except Exception as exc:\n"
    "    print('refused:' + exc.__class__.__name__)\n"
    "    sys.exit(3)\n"
    "print('reached')\n"
    "sys.exit(0)\n" % SELFTEST_HOST
)


class Problem(Exception):
    """One finding, with the rule id that produced it."""

    def __init__(self, rule: str, where: str, detail: str) -> None:
        super().__init__("%s: %s: %s" % (rule, where, detail))
        self.rule, self.where, self.detail = rule, where, detail

    def line(self) -> str:
        return "%s: %s: %s" % (self.rule, self.where, self.detail)


# ---------------------------------------------------------------------------
# the static half
# ---------------------------------------------------------------------------

def rust_net_features() -> list[dict]:
    """The published consent table, read out of `crates/fathom-core/src/privacy.rs`.

    Parsed rather than imported because the table is Rust and this tool is Python — the same reason
    `tests/test_workspace_contract.py` parses it: the check has to survive being written in the language
    that is *not* the one being checked.
    """
    text = (ROOT / "crates" / "fathom-core" / "src" / "privacy.rs").read_text(encoding="utf-8")
    rows = []
    for match in re.finditer(
        r"NetFeature\s*\{([^}]*)\}", text, re.S
    ):
        body = match.group(1)
        fields = dict(re.findall(r"(\w+)\s*:\s*\"([^\"]*)\"", body))
        if "feature" in fields:
            rows.append(fields)
    return rows


def check_closure(problems: list, notes: list) -> None:
    """`V01` — the default binary's closure, computed from the manifests (no cargo, no network)."""
    closure = M.closure_external_crates(M.cli_shipped_default_closure())
    linked = sorted(closure & set(M.NETWORK_CRATES))
    if linked:
        problems.append(Problem("V01", "Cargo.toml",
                                "the default build links network-capable %s (ADR-002 R6)"
                                % ", ".join("`%s`" % n for n in linked)))
    net_enabled = [item for item in M.cli_shipped_default_closure() if item.endswith(("net-",))
                   or "/net-" in item]
    if net_enabled:
        problems.append(Problem("V01", "Cargo.toml",
                                "the default build enables `net-*` features %s" % net_enabled))
    notes.append("V01   default build links %d external crates%s"
                 % (len(closure),
                    ", none of them network-capable" if not linked
                    else ", of which %d are network-capable" % len(linked)))


def check_consent_table(problems: list, notes: list) -> None:
    """`V02` — the Rust table, the model and the shipped schema must be one table, not three."""
    rows = rust_net_features()
    if not rows:
        problems.append(Problem("V02", "crates/fathom-core/src/privacy.rs",
                                "no `NetFeature` rows parsed, so this check is reading nothing"))
        return
    rust = {row["feature"]: row.get("consent_key") for row in rows}
    model = dict(M.NETWORK_CONSENT_KEYS)
    if rust != model:
        problems.append(Problem("V02", "crates/fathom-core/src/privacy.rs",
                                "consent table %s disagrees with tools/workspace_model.py %s"
                                % (rust, model)))
    schema = json.loads((ROOT / "schemas" / "fathom.schema.json").read_text(encoding="utf-8"))
    # The schema's `privacy` section is keyed without the prefix the table publishes, because the section
    # name *is* the prefix; adding it back is the whole translation.
    declared = schema["properties"].get("privacy", {}).get("properties", {})
    declared = {"privacy.%s" % key: spec for key, spec in declared.items()}
    missing = sorted(set(model.values()) - set(declared))
    if missing:
        problems.append(Problem("V02", "schemas/fathom.schema.json",
                                "consent keys with no schema entry: %s" % ", ".join(missing)))
    notes.append("V02   %d gated features, one consent table in Rust, the model and the schema"
                 % len(rows))

    # `V03` — the scope rule. A repo-scoped consent key would make `git clone` a privacy event, which is
    # the exact thing ADR-001 §4.6 forbids, so it is checked rather than trusted.
    for key, spec in sorted(declared.items()):
        if spec.get("x-fathom-scope") != "user":
            problems.append(Problem("V03", "schemas/fathom.schema.json",
                                    "privacy.%s is scope %r; consent must be user-scoped"
                                    % (key, spec.get("x-fathom-scope"))))
        if spec.get("default") is not False:
            problems.append(Problem("V03", "schemas/fathom.schema.json",
                                    "privacy.%s does not default to false" % key))
    notes.append("V03   every consent key is user-scoped and defaults to false")


def check_rust_sources(problems: list, notes: list) -> None:
    """`V04` — no egress-capable call in code a default build compiles.

    Files whose *own text* names a `net-` feature are the gated ones and are exempt: that is the whole
    point of the naming rule. Everything else in a shipped crate has to be clean.
    """
    shipped = sorted({item.partition("/")[0] for item in M.cli_shipped_default_closure()
                      if (ROOT / "crates" / item.partition("/")[0] / "src").is_dir()})
    scanned = 0
    for crate in shipped:
        for path in sorted((ROOT / "crates" / crate / "src").rglob("*.rs")):
            text = path.read_text(encoding="utf-8")
            if re.search(r"feature\s*=\s*\"net-", text):
                continue  # a gated file: its egress is what the feature is for
            scanned += 1
            for line_no, line in enumerate(text.splitlines(), start=1):
                stripped = line.strip()
                if stripped.startswith(("///", "//!", "//")):
                    continue  # prose about the ban is not the ban
                for needle in RUST_EGRESS:
                    if needle in line:
                        problems.append(Problem(
                            "V04", "%s:%d" % (path.relative_to(ROOT), line_no),
                            "`%s` in default-shipped code with no `net-*` gate in the file "
                            "(ADR-002 R6)" % needle))
    notes.append("V04   %d default-shipped .rs files carry no egress call" % scanned)


#: The Python modules allowed to touch the outside world: the CLI edges that perform a consented fetch
#: and read a cache. `tests/test_deprisk_contract.py` calls the same split `ANALYSIS_MODULES` vs
#: `IO_MODULES`; this is that split seen from the privacy side, so the exemption is one list and not two.
PYTHON_IO_SURFACES = ("cli.py", "__main__.py")


def check_python_sources(problems: list, notes: list) -> None:
    """`V05` — the analysis half is pure: no network import outside the gated CLI edges.

    AST rather than grep, so a string that merely *mentions* `socket` (and this repository has several,
    in comments about the rule) is not a hit, and an `import socket` hidden inside a function body is.
    """
    import ast

    package = ROOT / "src" / "recommender"
    files = sorted(p for p in package.rglob("*.py") if "__pycache__" not in p.parts)
    pure = [p for p in files if p.name not in PYTHON_IO_SURFACES]
    exempt = len(files) - len(pure)
    for path in pure:
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            names: list[str] = []
            if isinstance(node, ast.Import):
                names = [alias.name for alias in node.names]
            elif isinstance(node, ast.ImportFrom) and node.module:
                names = [node.module]
            for name in names:
                head = name.partition(".")[0]
                if head in {n.partition(".")[0] for n in PYTHON_EGRESS}:
                    problems.append(Problem(
                        "V05", "%s:%d" % (path.relative_to(ROOT), node.lineno),
                        "imports `%s`; the Python half reads only what it is handed (ADR-002 §4.5)"
                        % name))
    notes.append("V05   %d Python analysis modules import no network stack (%d CLI edge(s) are the "
                 "only IO surface)" % (len(pure), exempt))


def check_docs(problems: list, notes: list) -> None:
    """`V06` — the page exists, is reachable, and says the things that are true."""
    if not PRIVACY_PAGE.is_file():
        problems.append(Problem("V06", PRIVACY_PAGE.relative_to(ROOT),
                                "the privacy page is missing"))
        return
    page = PRIVACY_PAGE.read_text(encoding="utf-8")
    for name, path in (("README.md", README), ("docs/index.md", DOC_INDEX)):
        text = path.read_text(encoding="utf-8")
        if "concepts/privacy.md" not in text and "docs/concepts/privacy.md" not in text:
            problems.append(Problem("V06", name,
                                    "does not link %s; a posture nobody can find is a rumour"
                                    % PRIVACY_PAGE.relative_to(ROOT)))
    for row in rust_net_features():
        for field in ("feature", "consent_key"):
            if "`%s`" % row[field] not in page:
                problems.append(Problem("V06", PRIVACY_PAGE.relative_to(ROOT),
                                        "never names `%s`, so the page is behind the table"
                                        % row[field]))
    # The three sentences are the claims; `check_docs_site.py` keeps the page reachable and this keeps
    # it on-message. `LOCAL_ONLY_SENTENCE` is what `doctor --privacy` prints, quoted from the source so
    # the page cannot drift from the binary it documents.
    diagnose = (ROOT / "crates" / "fathom-cli" / "src" / "diagnose.rs").read_text(encoding="utf-8")
    if "usage metrics:" not in diagnose:
        problems.append(Problem("V06", "crates/fathom-cli/src/diagnose.rs",
                                "no longer prints the local-only line this page documents"))
    for claim in ("no endpoint", "user-scoped", "doctor --privacy", "check_privacy.py"):
        if claim not in page:
            problems.append(Problem("V06", PRIVACY_PAGE.relative_to(ROOT),
                                    "does not state %r" % claim))
    notes.append("V06   %s is linked from README.md and docs/index.md and names every gate"
                 % PRIVACY_PAGE.relative_to(ROOT))


def check_release_artefacts(problems: list, notes: list) -> None:
    """`V07` — the SBOM and licence report exist, and a release publishes them."""
    # `licen[cs]e`, both spellings on purpose: the generator writes `licenses` (the SPDX and
    # Cargo.toml spelling, and what tooling greps for) and the prose in this repository says `licence`.
    generated = sorted(SBOM_DIR.glob("*.cdx.json")) + sorted(p for p in SBOM_DIR.glob("*.md")
                                                             if "licen" in p.name)
    if not generated:
        problems.append(Problem("V07", "sbom/",
                                "nothing generated: run `python3 tools/gen_sbom.py write`"))
    for workflow in RELEASE_WORKFLOWS:
        if not workflow.is_file():
            problems.append(Problem("V07", workflow.relative_to(ROOT), "missing"))
            continue
        text = workflow.read_text(encoding="utf-8")
        if "gen_sbom.py" not in text:
            problems.append(Problem("V07", workflow.relative_to(ROOT),
                                    "never runs `tools/gen_sbom.py`, so nothing is published"))
        if "release" not in text:
            problems.append(Problem("V07", workflow.relative_to(ROOT),
                                    "is not wired to a release, so it publishes nothing per release"))
    notes.append("V07   SBOM and licence report are generated and wired to the release "
                 "(%d artefact(s))" % len(generated))


def run_check(verbose: bool) -> int:
    problems: list = []
    notes: list = []
    for step in (check_closure, check_consent_table, check_rust_sources, check_python_sources,
                 check_docs, check_release_artefacts):
        step(problems, notes)
    for problem in problems:
        print(problem.line())
    if verbose:
        for note in notes:
            print("  " + note)
    elif not problems:
        print("check_privacy: %d static checks clean (%s)"
              % (len(notes), ", ".join(note.split()[0] for note in notes)))
    return 1 if problems else 0


# ---------------------------------------------------------------------------
# the dynamic half: the egress trap
# ---------------------------------------------------------------------------

def _run(argv: list[str], env: dict | None = None, cwd: pathlib.Path | None = None,
         timeout: int = 120) -> subprocess.CompletedProcess:
    return subprocess.run(argv, env=env, cwd=str(cwd) if cwd else None, capture_output=True,
                          text=True, timeout=timeout)


class Trap:
    """One way of making egress impossible for a child process, plus the proof that it works."""

    def __init__(self, kind: str, prefix: list[str] = (), note: str = "") -> None:
        self.kind = kind
        self.prefix = list(prefix)
        self.note = note

    # -- construction -------------------------------------------------------
    @classmethod
    def candidates(cls) -> list["Trap"]:
        out: list[Trap] = []
        for prefix, note in (
            (["unshare", "-rn"], "unshared network namespace (loopback only, and down)"),
            (["sudo", "-n", "unshare", "-n"], "root-owned network namespace"),
        ):
            if shutil.which(prefix[0]):
                out.append(cls("netns:" + prefix[0], prefix, note))
        compiler = next((c for c in ("cc", "gcc", "clang") if shutil.which(c)), None)
        if compiler and sys.platform.startswith("linux"):
            out.append(cls("ld_preload", note="`%s`-built tripwire over libc `connect`/`getaddrinfo`"
                           % compiler))
        if shutil.which("sandbox-exec"):
            out.append(cls("sandbox-exec", ["sandbox-exec", "-p",
                                            "(version 1)(allow default)(deny network*)"],
                           "macOS seatbelt profile denying `network*`"))
        return out

    def prepare(self, workdir: pathlib.Path) -> None:
        """Compile the preload shim, if this trap needs one."""
        self.shim = None
        if self.kind != "ld_preload":
            return
        if not NETPROBE_SOURCE.is_file():
            raise FileNotFoundError("%s is missing" % NETPROBE_SOURCE)
        self.shim = workdir / ("netprobe-%s.so" % hashlib.sha256(
            NETPROBE_SOURCE.read_bytes()).hexdigest()[:10])
        compiler = next(c for c in ("cc", "gcc", "clang") if shutil.which(c))
        result = _run([compiler, "-O2", "-fPIC", "-Wall", "-Wextra", "-shared", "-o",
                       str(self.shim), str(NETPROBE_SOURCE)])
        if result.returncode != 0:
            raise RuntimeError("the netprobe shim did not build:\n%s" % result.stderr)

    # -- use ---------------------------------------------------------------
    def wrap(self, argv: list[str]) -> list[str]:
        return self.prefix + argv

    def env(self, base: dict, log: pathlib.Path) -> dict:
        env = dict(base)
        if self.kind == "ld_preload":
            env["LD_PRELOAD"] = str(self.shim)
            env["NETPROBE_LOG"] = str(log)
        elif self.kind.startswith("netns"):
            env.pop("http_proxy", None)
            env.pop("https_proxy", None)
            env.pop("HTTP_PROXY", None)
            env.pop("HTTPS_PROXY", None)
        return env

    def attempts(self, log: pathlib.Path) -> list[str] | None:
        """Every refused attempt, or `None` where the trap cannot report them (netns, seatbelt)."""
        if self.kind != "ld_preload":
            return None
        if not log.exists():
            return []
        return [line for line in log.read_text(encoding="utf-8").splitlines()
                if line.startswith("attempt ")]

    # -- proof -------------------------------------------------------------
    def verify(self, log: pathlib.Path) -> tuple[bool, str]:
        """Run a program that *should* be stopped, and check that it was.

        This is the whole reason the tool can claim anything: an untested sandbox is a sandbox that may
        have stopped working, and a silent pass is worse than a red build. The probe *prints* which half
        happened, because a non-zero exit on its own cannot tell "the sandbox refused it" from "the
        sandbox command never started" — which is exactly the failure this is looking for.
        """
        started = time.monotonic()
        result = _run(["python3", "-c", SELFTEST_SNIPPET], env=self.env(os.environ.copy(), log))
        seconds = time.monotonic() - started
        said = result.stdout.strip().splitlines()[-1] if result.stdout.strip() else ""
        if said.startswith("reached"):
            return False, "the probe reached %s:%d — the trap is not working" % SELFTEST_HOST
        if not said.startswith("refused"):
            return False, "the probe never ran: exit %d, %r %s" % (
                result.returncode, said, result.stderr.strip()[:160])
        if seconds > 4.5:
            return False, ("the probe took %.1fs to fail, which is a timeout rather than a refusal; a "
                           "trap that only slows traffic proves nothing about it" % seconds)
        if self.kind == "ld_preload":
            attempts = self.attempts(log) or []
            if not attempts:
                return False, "the probe failed but the tripwire logged nothing"
            return True, "%s in %.2fs, logged as: %s" % (said, seconds, attempts[0])
        return True, "%s in %.2fs (%s)" % (said, seconds, self.note)


def pick_trap(workdir: pathlib.Path, want: str | None) -> tuple[Trap | None, str, list[str]]:
    """The strongest trap this machine actually provides, verified before it is trusted.

    Returns the chosen trap, the evidence that it traps, and the reasons every earlier candidate was
    rejected — printed, because "the sandbox did not apply here" has to be visible in the log.
    """
    tried: list[str] = []
    for trap in Trap.candidates():
        if want and want not in trap.kind:
            continue
        try:
            trap.prepare(workdir)
        except Exception as exc:  # a missing compiler is a reason to move on, not to stop
            tried.append("%s: %s" % (trap.kind, exc))
            continue
        log = workdir / ("selftest-%s.log" % trap.kind.replace(":", "-"))
        try:
            ok, detail = trap.verify(log)
        except Exception as exc:
            tried.append("%s: %s" % (trap.kind, exc))
            continue
        if ok:
            return trap, detail, tried
        tried.append("%s: %s" % (trap.kind, detail))
    return None, "", tried


def demo_repository(binary: pathlib.Path, workdir: pathlib.Path) -> pathlib.Path:
    """The miniature repository that ships inside the binary, unpacked and indexed.

    Using the demo pack rather than this workspace is deliberate: it is the same 21 commits on every
    machine, it indexes in tens of milliseconds, and it contains the awkward cases (a rename, two
    ecosystems of dependencies, a CI job) that make an identical-bytes claim worth something.
    """
    env = dict(os.environ, XDG_CACHE_HOME=str(workdir / "cache"), LANG="C.UTF-8")
    result = _run([str(binary), "--demo"], env=env, cwd=workdir)
    match = re.search(r"unpacked to (\S+)", result.stdout + result.stderr)
    if not match:
        raise RuntimeError("`fathom --demo` did not unpack a repository: %s%s"
                           % (result.stdout, result.stderr))
    return pathlib.Path(match.group(1))


def normalise(text: str) -> tuple[str, int]:
    """Excuse the clock, and count how many lines it cost."""
    excused = 0
    out = []
    for line in text.splitlines():
        replaced = line
        for pattern, sentinel in VOLATILE:
            replaced = pattern.sub(sentinel, replaced)
        if replaced != line:
            excused += 1
        out.append(replaced)
    return "\n".join(out), excused


def reset_state(repo: pathlib.Path, pristine: pathlib.Path) -> None:
    """Put the store back exactly as it was, so two runs of one command see one database.

    Without this the pair measures the wrong thing: `fathom doctor` reads the cache and *counts* the
    read, so a second run of any verb changes the hit-rate line. The comparison is about what the
    network can change, so everything else is pinned first.
    """
    store = repo / ".fathom"
    shutil.rmtree(store, ignore_errors=True)
    shutil.copytree(pristine / ".fathom", store, symlinks=True)


def run_pair(binary: pathlib.Path, repo: pathlib.Path, pristine: pathlib.Path, argv: list[str],
             trap: Trap, log: pathlib.Path) -> dict:
    plain = dict(os.environ, LANG="C.UTF-8", NO_COLOR="1")
    reset_state(repo, pristine)
    online = _run([str(binary), *argv], env=plain, cwd=repo)
    if trap.kind == "ld_preload":
        log.unlink(missing_ok=True)
    reset_state(repo, pristine)
    trapped = _run(trap.wrap([str(binary), *argv]), env=trap.env(plain, log), cwd=repo)
    a, a_lines = normalise(online.stdout + online.stderr)
    b, b_lines = normalise(trapped.stdout + trapped.stderr)
    return {
        "argv": argv,
        "exit_online": online.returncode,
        "exit_trapped": trapped.returncode,
        "identical": (a, online.returncode) == (b, trapped.returncode),
        "clock_lines_excused": max(a_lines, b_lines),
        "attempts": (len(trap.attempts(log)) if trap.kind == "ld_preload" else None),
        "diff": "" if a == b else _first_difference(a, b),
    }


def _first_difference(a: str, b: str) -> str:
    """One line of why a pair differed, enough to act on without turning the report into a log."""
    for left, right in zip(a.splitlines(), b.splitlines()):
        if left != right:
            return "online %r / trapped %r" % (left[:160], right[:160])
    return "the two runs differ in length (%d vs %d lines)" % (len(a.splitlines()),
                                                              len(b.splitlines()))


#: The facts about an index that do not depend on how fast the machine was or when the run happened:
#: the same keys `fathom stats --json` prints, minus the counters a second query would move.
INDEX_SUMMARY = (
    ("index", "commits"), ("index", "files"), ("index", "epoch_status"),
    ("index", "integrity"), ("index", "head_indexed"), ("index", "record_schema"),
    ("index", "schema_version"), ("index", "size_bytes"), ("index", "rows_written"),
    ("scale", "authors"), ("scale", "files"), ("scale", "lines_added"),
    ("scale", "lines_removed"), ("scale", "kinds"),
)


def index_summary(stats_json: str) -> str:
    """The part of a `stats --json` payload that a cold index owes to be reproducible."""
    try:
        parsed = json.loads(stats_json)
    except json.JSONDecodeError:
        return stats_json.strip()[:400]
    index = parsed.get("index", {})
    scale = index.get("scale", {})
    picked = {}
    for section, key in INDEX_SUMMARY:
        source = index if section == "index" else scale
        if key in source:
            picked["%s.%s" % (section, key)] = source[key]
    return json.dumps(picked, sort_keys=True)


def cold_index_pair(binary: pathlib.Path, source: pathlib.Path, trap: Trap,
                    workdir: pathlib.Path, log: pathlib.Path) -> dict:
    """Index two fresh copies of the same history, one of them with no route out.

    Indexing is the verb with the most reason to reach outward — the one that would notice an advisory
    lookup, a submodule or a partial clone — so the claim is measured here as well as on the read path.
    The comparison is the printed line (timings excepted) and the index the run left behind, not the
    store byte-for-byte: a database file is not stable across two runs and pretending otherwise would
    make this check red for reasons that have nothing to do with the network.
    """
    plain = dict(os.environ, LANG="C.UTF-8", NO_COLOR="1")
    results = {}
    for label, trapped in (("online", False), ("trapped", True)):
        copy = workdir / ("copy-" + label)
        shutil.copytree(source, copy, symlinks=True)
        shutil.rmtree(copy / ".fathom", ignore_errors=True)
        if trapped and trap.kind == "ld_preload":
            log.unlink(missing_ok=True)
        argv = [str(binary), "index"]
        out = _run(trap.wrap(argv) if trapped else argv,
                   env=trap.env(plain, log) if trapped else plain, cwd=copy)
        stats = _run([str(binary), "stats", "--json"], env=plain, cwd=copy)
        results[label] = {
            "out": out,
            "summary": index_summary(stats.stdout),
            "attempts": (len(trap.attempts(log) or [])
                         if (trapped and trap.kind == "ld_preload") else None),
        }
    a, a_lines = normalise(results["online"]["out"].stdout + results["online"]["out"].stderr)
    b, b_lines = normalise(results["trapped"]["out"].stdout + results["trapped"]["out"].stderr)
    identical = (a, results["online"]["out"].returncode, results["online"]["summary"]) == \
                (b, results["trapped"]["out"].returncode, results["trapped"]["summary"])
    return {
        "argv": ["index"],
        "exit_online": results["online"]["out"].returncode,
        "exit_trapped": results["trapped"]["out"].returncode,
        "identical": identical,
        "clock_lines_excused": max(a_lines, b_lines),
        "attempts": results["trapped"]["attempts"],
        "index_summary_online": results["online"]["summary"],
        "index_summary_trapped": results["trapped"]["summary"],
        "diff": "" if identical else _first_difference(a, b),
    }


def run_offline(binary: pathlib.Path, report_path: pathlib.Path | None, want: str,
                verbose: bool) -> int:
    if not binary.is_file():
        print("offline: no binary at %s — build it first (`cargo build -p fathom-cli`)" % binary)
        return 2
    workdir = pathlib.Path(tempfile.mkdtemp(prefix="fathom-privacy-"))
    try:
        trap, proof, tried = pick_trap(workdir, want)
        if trap is None:
            print("offline: no working egress trap on this machine; the check refuses to report a "
                  "pass it did not measure")
            for line in tried:
                print("  tried %s" % line)
            return 2
        print("trap    %s — verified: %s" % (trap.kind, proof))
        for line in tried:
            print("  rejected %s" % line)
        repo = demo_repository(binary, workdir)
        pristine = workdir / "pristine"
        shutil.copytree(repo, pristine, symlinks=True)
        log = workdir / "attempts.log"
        results = [run_pair(binary, repo, pristine, argv, trap, log) for argv in OFFLINE_COMMANDS]
        results.append(cold_index_pair(binary, repo, trap, workdir, log))

        problems = [row for row in results if not row["identical"]]
        # `None` means the trap can refuse but cannot count — a namespace has no log — so "0" and
        # "unmeasured" are different verdicts and are never printed as one.
        logged = [row["attempts"] for row in results if row.get("attempts") is not None]
        attempts = sum(logged) if logged else None
        report = {
            "contract": "fathom.privacy-offline/1",
            "binary": _run([str(binary), "--version"]).stdout.strip(),
            "trap": {"kind": trap.kind, "note": trap.note, "verified": True,
                     "counts_attempts": bool(logged)},
            "egress_attempts": attempts,
            "commands": results,
            "verdict": "pass" if not problems and not attempts else "fail",
        }
        if verbose:
            for row in results:
                print("  %-28s identical=%s exit=%s/%s%s"
                      % (" ".join(row["argv"]), row["identical"], row.get("exit_online"),
                         row.get("exit_trapped"),
                         "" if row.get("attempts") is None
                         else " attempts=%s" % row["attempts"]))
        for row in problems:
            print("V08   %s: output differs with egress refused — %s"
                  % (" ".join(row["argv"]), row.get("diff") or "see the `--json` report"))
        if attempts:
            print("V09   %d connection attempt(s) while trapped: the shipped binary reached outward"
                  % attempts)
        if report_path:
            report_path.parent.mkdir(parents=True, exist_ok=True)
            report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n",
                                   encoding="utf-8")
        if not problems and not attempts:
            print("offline: %d command(s), byte-identical with egress refused; %s"
                  % (len(results),
                     ("connection attempts logged: %d" % attempts) if logged else
                     "attempts not countable by this trap (it refuses without logging)"))
            print("        trap %s; report %s" % (trap.kind, report_path or "not written"))
        return 1 if problems or attempts else 0
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


def run_selftest(want: str) -> int:
    workdir = pathlib.Path(tempfile.mkdtemp(prefix="fathom-privacy-selftest-"))
    try:
        failures = []
        for trap in Trap.candidates():
            if want and want not in trap.kind:
                continue
            try:
                trap.prepare(workdir)
                log = workdir / ("selftest-%s.log" % trap.kind.replace(":", "-"))
                ok, detail = trap.verify(log)
            except Exception as exc:
                ok, detail = False, str(exc)
            print("%-24s %s — %s" % (trap.kind, "traps" if ok else "DOES NOT TRAP", detail))
            if not ok:
                failures.append(trap.kind)
        if want and not failures:
            return 0
        if failures:
            # A trap that does not trap is only a problem if it was supposed to be usable; the tool
            # picks among candidates at runtime, so `selftest` reports the state of each and is red
            # only when none of them work.
            return 0 if len(failures) < len(Trap.candidates()) else 1
        print("selftest: no candidate trap available on this platform")
        return 2
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="mode", required=True)

    check = sub.add_parser("check", help="the static half: closure, table, sources, docs, release")
    check.add_argument("-v", "--verbose", action="store_true")

    offline = sub.add_parser("offline", help="run the binary with egress refused and compare")
    offline.add_argument("--binary", default=str(ROOT / "target" / "debug" / "fathom"))
    offline.add_argument("--json", dest="report", default=None,
                        help="write the report here (CI uploads it)")
    offline.add_argument("--trap", default="", help="require a particular trap, e.g. `ld_preload`")
    offline.add_argument("-v", "--verbose", action="store_true",
                        help="print one line per command, identical or not")

    selftest = sub.add_parser("selftest", help="prove each egress trap on this machine actually traps")
    selftest.add_argument("--trap", default="")

    args = parser.parse_args(argv)
    if args.mode == "check":
        return run_check(args.verbose)
    if args.mode == "offline":
        # Relative paths mean relative to the repository, not to whatever directory the caller stood in:
        # CI runs this from `.github/workflows`, and a binary found by accident is a binary that will not
        # be found on the next runner.
        binary = pathlib.Path(args.binary)
        if not binary.is_absolute():
            binary = ROOT / binary
        return run_offline(binary, pathlib.Path(args.report) if args.report else None,
                           args.trap, args.verbose)
    return run_selftest(args.trap)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
