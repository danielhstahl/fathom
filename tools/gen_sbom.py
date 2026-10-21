#!/usr/bin/env python3
"""Generate the SBOM and the licence report that a release publishes (ticket `e6.8`, reco-h7p).

"What is in the binary you are about to run" has two answers people actually ask for: the **inventory**
(every crate, pinned, with its digest) and the **licence position** (what each of them lets you do).
Both are derivable from this repository, so both are generated rather than curated:

```bash
python3 tools/gen_sbom.py write    # sbom/fathom-<version>.cdx.json + sbom/fathom-<version>-licenses.md
python3 tools/gen_sbom.py check    # regenerate and fail on any drift — what CI runs
python3 tools/gen_sbom.py show     # the summary, on stdout
```

Four sources of truth, and none of them is a paragraph:

| what | from | why that one |
|---|---|---|
| which crates, which versions, which digests | `Cargo.lock` | the lockfile *is* the pin, and a digest is what turns an inventory into evidence |
| which licences a crate declares | `cargo metadata --locked` | a licence is a property of the published crate, and `cargo` is the only thing that reads it correctly |
| what the shipped binary actually contains | `tools/workspace_model.py`'s default-feature closure | the default build is the product (ADR-002 §6.1); an SBOM of every optional feature would overstate what a user received |
| what this project allows | `deny.toml` | the licence policy is one file; this report reads it instead of restating it, so a policy change moves the report rather than a paragraph somewhere else |

The output is **deterministic**: sorted, no timestamps, and the CycloneDX `serialNumber` is a UUIDv5
over the lockfile digest instead of a random one. That is what makes `check` meaningful — a diff in the
committed artefacts means a dependency moved, and nothing else — and it is the same argument
`docs/concepts/release.md` makes about every generated table in this repository.

The licence report is not `cargo deny check licenses` and does not replace it. deny decides whether a PR
may merge, on every platform the policy names; this reports what a *release* ships, in a form a
distributor can attach and a user can read. Both read the same `deny.toml`, which is the only way the two
can never disagree about what is allowed.

Needs `cargo` (for the licence declarations) and nothing else. Exit: 0 clean, 1 on drift or on a
licence the policy does not allow, 2 when the inputs could not be read.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tomllib
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import workspace_model as M  # noqa: E402

SBOM_DIR = ROOT / "sbom"
CARGO_LOCK = ROOT / "Cargo.lock"
DENY_TOML = ROOT / "deny.toml"

#: Which CycloneDX version the artefact claims. 1.5 is what every consumer in the ecosystem reads
#: today (`cargo-cyclonedx`, `syft`, Dependency-Track); nothing here needs 1.6's signing block.
SPEC_VERSION = "1.5"

#: The SPDX shape `deny.toml`'s allow-list can express: `A AND B`, `A OR B`, `A WITH exception`, and
#: nothing else in this workspace. Parentheses would need a real parser; saying so out loud is cheaper
#: than writing one, and the `unknown` verdict below is what catches a crate that disagrees.
CLAUSE = re.compile(r"\s+(OR|AND)\s+")


def workspace_version() -> str:
    """The one version number, from `[workspace.package]` (ADR-002 §10, rule R05)."""
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"', text, re.S | re.M)
    if not match:
        raise SystemExit("Cargo.toml: no [workspace.package] version, so the SBOM has no version")
    return match.group(1)


def workspace_license() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\[workspace\.package\].*?^license\s*=\s*"([^"]+)"', text, re.S | re.M)
    return match.group(1) if match else "UNLICENSED"


def lock_packages() -> dict[tuple[str, str], dict]:
    """`Cargo.lock` as data: `(name, version)` to its source and checksum."""
    lock = tomllib.loads(CARGO_LOCK.read_text(encoding="utf-8"))
    out: dict[tuple[str, str], dict] = {}
    for package in lock.get("package", []):
        out[(package["name"], package["version"])] = {
            "source": package.get("source", ""),
            "checksum": package.get("checksum", ""),
        }
    return out


def cargo_metadata() -> dict:
    """Every package's declared licence, from cargo itself.

    `--offline` first, because describing a release should not need the network and the registry cache
    is warm after any build; the online call is the fallback for a manifest this machine never resolved
    (a target-specific dependency a local build never fetched), which is a normal thing on a fresh CI
    runner and should not be a failure.
    """
    for flags in (["--offline"], []):
        result = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked", *flags],
            cwd=str(ROOT), capture_output=True, text=True)
        if result.returncode == 0:
            return json.loads(result.stdout)
    raise SystemExit("cargo metadata failed: %s" % result.stderr.strip()[:400])


def allowed_licences() -> list[str]:
    """`deny.toml`'s `[licenses] allow`, verbatim. The single source of what is permitted here."""
    deny = tomllib.loads(DENY_TOML.read_text(encoding="utf-8"))
    return list(deny.get("licenses", {}).get("allow", []))


def banned_crates() -> list[str]:
    deny = tomllib.loads(DENY_TOML.read_text(encoding="utf-8"))
    return sorted(entry["name"] for entry in deny.get("bans", {}).get("deny", []))


def licence_verdict(expression: str, allow: list[str]) -> str:
    """`allowed`, `violation` or `unknown`, the way `cargo deny` would reach it.

    An `OR` branch needs one acceptable arm; an `AND` needs all of them; `/` is the old spelling of the
    same licence and is treated as `OR`. An expression this function cannot parse — parentheses, or an
    operator it has never seen — is `unknown`, and `unknown` is not a pass: an unparseable licence is
    exactly the thing a report must not wave through, and `cargo deny`'s own `unknown-license = deny`
    says the same thing in the other language.
    """
    expression = (expression or "").strip()
    if not expression or expression == "unknown":
        return "unknown"
    allow_set = set(allow)
    if "(" in expression or ")" in expression:
        return "unknown"

    def arm_ok(arm: str) -> bool:
        terms = [term.strip() for term in re.split(r"\s+AND\s+", arm) if term.strip()]
        return bool(terms) and all(term in allow_set for term in terms)

    arms = [arm.strip() for arm in re.split(r"\s+OR\s+|/", expression) if arm.strip()]
    if not arms:
        return "unknown"
    return "allowed" if any(arm_ok(arm) for arm in arms) else "violation"


def shipped_components() -> list[tuple[str, str, dict]]:
    """The `(name, version, metadata)` pairs the **default** binary is built from.

    Both halves of the closure are here — this repository's own crates and the third-party ones — because
    an SBOM that lists only the dependencies leaves the auditor to guess about the half the project
    wrote, and about whether the binary is one crate or seven.
    """
    metadata = cargo_metadata()
    by_name: dict[str, list[dict]] = {}
    for package in metadata["packages"]:
        by_name.setdefault(package["name"], []).append(package)
    lock = lock_packages()

    closure = M.cli_shipped_default_closure()
    in_closure = {item.partition("/")[0] for item in closure}
    workspace_names = sorted(name for name in in_closure if name in M.CRATES_BY_NAME)
    external_names = sorted(M.closure_external_crates(closure))

    out: list[tuple[str, str, dict]] = []
    for name in workspace_names:
        candidates = by_name.get(name, [])
        package = next((p for p in candidates if p.get("source") is None),
                       candidates[0] if candidates else None)
        if package is None:
            print("gen_sbom: %s is in the default closure but not in cargo metadata" % name,
                  file=sys.stderr)
            continue
        out.append((name, package["version"], {"package": package, "first_party": True,
                                              "checksum": ""}))
    for name in external_names:
        for package in by_name.get(name, []):
            version = package["version"]
            lock_row = lock.get((name, version), {})
            out.append((name, version, {"package": package, "first_party": False,
                                        "checksum": lock_row.get("checksum", "")}))
    return sorted(out, key=lambda row: (row[0], row[1]))


def purl(name: str, version: str) -> str:
    return "pkg:cargo/%s@%s" % (name, version)


def bom() -> dict:
    version = workspace_version()
    lock_digest = hashlib.sha256(CARGO_LOCK.read_bytes()).hexdigest()
    components = []
    for name, package_version, info in shipped_components():
        package = info["package"]
        expression = package.get("license") or "unknown"
        entry = {
            "bom-ref": "%s@%s" % (name, package_version),
            "name": name,
            "version": package_version,
            "type": "library" if not info["first_party"] else "library",
            "purl": purl(name, package_version),
            "licenses": [{"license": {"expression": expression}}],
            "properties": [
                {"name": "fathom:first-party", "value": "true" if info["first_party"] else "false"},
                {"name": "fathom:licence-verdict",
                 "value": ("workspace" if info["first_party"]
                           else licence_verdict(expression, allowed_licences()))},
            ],
        }
        if info["checksum"]:
            entry["hashes"] = [{"alg": "SHA-256", "content": info["checksum"]}]
        if package.get("description"):
            entry["description"] = package["description"]
        components.append(entry)

    return {
        "bomFormat": "CycloneDX",
        "specVersion": SPEC_VERSION,
        "version": 1,
        # A UUIDv5 over the lockfile digest, not a random one: two runs over the same lock must produce
        # the same artefact, or `check` is a coin toss and nobody keeps it.
        "serialNumber": "urn:uuid:" + str(uuid.uuid5(
            uuid.UUID("6ba7b810-9dad-11d1-80b4-00c04fd430c8"),
            "fathom-sbom:" + lock_digest)),
        "metadata": {
            "component": {
                "type": "application",
                "name": "fathom",
                "version": version,
                "bom-ref": "fathom@%s" % version,
                "description": "Fathom: the terminal UI that keeps a human's mental model of a "
                               "codebase a coding agent is writing.",
                "licenses": [{"license": {"expression": workspace_license()}}],
                "properties": [
                    {"name": "fathom:build", "value": "default feature set (ADR-002 §6.1)"},
                    {"name": "fathom:cargo-lock-sha256", "value": lock_digest},
                    {"name": "fathom:network-crates", "value": "none"},
                ],
            },
            "tools": [{"vendor": "fathom", "name": "tools/gen_sbom.py", "version": version}],
        },
        "components": components,
    }


def licence_report(document: dict) -> str:
    """The markdown half: the same inventory, read as obligations rather than as parts."""
    version = workspace_version()
    allow = allowed_licences()
    rows = []
    for name, package_version, info in shipped_components():
        expression = info["package"].get("license") or "unknown"
        verdict = "workspace" if info["first_party"] else licence_verdict(expression, allow)
        rows.append((name, package_version, expression, verdict,
                     "yes" if info["checksum"] else "no"))
    counts: dict[str, int] = {}
    for _n, _v, _e, verdict, _d in rows:
        counts[verdict] = counts.get(verdict, 0) + 1
    expressions: dict[str, int] = {}
    for _n, _v, expression, _verdict, _d in rows:
        expressions[expression] = expressions.get(expression, 0) + 1

    lines = [
        "# Licence report: `fathom %s`" % version,
        "",
        "- **Generated by**: `python3 tools/gen_sbom.py write`. Do not hand-edit; `check` re-generates "
        "and fails on the difference.",
        "- **Covers**: the crates a **default-feature build** of `fathom-cli` links (ADR-002 §6.1), "
        "which is the artefact the installers ship. Optional features, dev-dependencies and the Python "
        "development tooling are not in a user's binary and so are not here; `Cargo.lock` names them all.",
        "- **Allow-list**: `deny.toml` `[licenses] allow`, read at generation time — %d expression(s) "
        "declared." % len(allow),
        "- **Digests**: a SHA-256 is present for crates from crates.io (`Cargo.lock`'s own checksum); "
        "this repository's own crates are path dependencies and have no published digest.",
        "",
        "## The verdict, in one table",
        "",
        "| Verdict | Meaning |",
        "|---|---|",
    ]
    meaning = {
        "allowed": "every arm of the expression is on `deny.toml`'s allow-list",
        "violation": "an expression with no acceptable arm — this must not be in a release",
        "unknown": "no licence declared, or a shape this report cannot parse; it is not a pass",
        "workspace": "one of Fathom's own crates, under the workspace licence `%s`"
                     % workspace_license(),
    }
    for verdict in ("workspace", "allowed", "unknown", "violation"):
        lines.append("| `%s` | %s — %d component(s) |" % (verdict, meaning[verdict],
                                                          counts.get(verdict, 0)))
    lines += [
        "",
        "## Every component",
        "",
        "| Crate | Version | Declared licence | Verdict | Digest |",
        "|---|---|---|---|---|",
    ]
    lines += ["| `%s` | %s | `%s` | `%s` | %s |" % row for row in rows]
    lines += [
        "",
        "## Licence expressions, counted",
        "",
        "| Expression | Components |",
        "|---|---|",
    ]
    lines += ["| `%s` | %d |" % pair for pair in
              sorted(expressions.items(), key=lambda pair: (-pair[1], pair[0]))]

    banned_present = sorted({name for name, _v, info in shipped_components()
                             if not info["first_party"]} & set(banned_crates()))
    net_present = sorted({name for name, _v, info in shipped_components()
                          if not info["first_party"]} & set(M.NETWORK_CRATES))
    lines += [
        "",
        "## The two lists that have to be empty",
        "",
        "| Question | Answer |",
        "|---|---|",
        "| Any crate `deny.toml` `[bans] deny` is in this build? | %s |"
        % ("**yes**: " + ", ".join(banned_present) if banned_present
           else "no — `%s` are all absent" % "`, `".join(banned_crates())),
        "| Any network-capable crate in this build? | %s |"
        % ("**yes**: " + ", ".join(net_present) if net_present
           else "no — the default binary links no HTTP client, TLS stack or git transport (ADR-002 R6)"),
        "",
        "The second row is the privacy posture read from the inventory instead of from a promise, and "
        "it is the row `fathom doctor --privacy` and `python3 tools/check_privacy.py check` exist to keep "
        "honest ([the privacy page](../docs/concepts/privacy.md)).",
        "",
    ]
    return "\n".join(lines)


def artefacts() -> tuple[pathlib.Path, pathlib.Path]:
    version = workspace_version()
    return (SBOM_DIR / ("fathom-%s.cdx.json" % version),
            SBOM_DIR / ("fathom-%s-licenses.md" % version))


def render() -> tuple[str, str]:
    document = bom()
    return (json.dumps(document, indent=2, sort_keys=True) + "\n", licence_report(document))


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("mode", choices=("write", "check", "show"))
    args = parser.parse_args(argv)

    json_text, markdown = render()
    json_path, markdown_path = artefacts()
    violations = [line for line in markdown.splitlines() if line.startswith("| `violation`")]
    unknown = [line for line in markdown.splitlines() if line.startswith("| `unknown`")]

    if args.mode == "write":
        SBOM_DIR.mkdir(exist_ok=True)
        json_path.write_text(json_text, encoding="utf-8")
        markdown_path.write_text(markdown, encoding="utf-8")
        components = len(json.loads(json_text)["components"])
        print("wrote %s and %s (%d components, %d allowed-licence expression(s))"
              % (json_path.relative_to(ROOT), markdown_path.relative_to(ROOT), components,
                 len(allowed_licences())))
        return 0
    if args.mode == "show":
        print(markdown)
        return 0

    problems = []
    for path, text in ((json_path, json_text), (markdown_path, markdown)):
        if not path.exists():
            problems.append("%s is missing — run `python3 tools/gen_sbom.py write`"
                            % path.relative_to(ROOT))
        elif path.read_text(encoding="utf-8") != text:
            problems.append("%s is stale — the generator disagrees with the committed artefact "
                            "(run `python3 tools/gen_sbom.py write`)" % path.relative_to(ROOT))
    for problem in problems:
        print("gen_sbom: %s" % problem)
    if not problems and (violations[0].split("—")[-1].strip() != "0 component(s) |"
                         or unknown[0].split("—")[-1].strip() != "0 component(s) |"):
        print("gen_sbom: the inventory contains components the licence policy does not allow "
              "(see the verdict table)")
        return 1
    if problems:
        return 1
    print("gen_sbom: SBOM and licence report are current and every component's licence is allowed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
