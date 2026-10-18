#!/usr/bin/env python3
"""ADR-002: render its normative tables, and enforce them on a real `crates/` tree.

    python3 tools/check_workspace.py check         # model self-consistency + ADR drift + CI wiring
    python3 tools/check_workspace.py write         # regenerate ADR-002's generated blocks in place
    python3 tools/check_workspace.py lint [DIR]    # the module-boundary lint (fails an uphill import)
    python3 tools/check_workspace.py scaffold DIR  # write the workspace shape ADR-002 declares
    python3 tools/check_workspace.py shipped       # what the default binary links, transitively

`check` and `lint` are what CI runs. `lint` exits 1 on any violation; if the workspace has not been
scaffolded yet (`e0.7` owns that step) it says so and exits 0 — a boundary lint that fails before there
is a workspace teaches nobody anything.

Rule ids are stable and appear in every message: `E#` crate edges, `M#` module boundaries, `F#`
features, `N#` network, `H#` weight, `P#` purity/unsafe, `A#` attributes and lints, `T#` toolchain,
`D#` cargo-deny config, `C#` CI wiring, `U#` workspace shape.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import tomllib
from dataclasses import dataclass, field

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import workspace_model as M  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
ADR = ROOT / "docs" / "adr" / "0002-workspace.md"
KEYMAP = ROOT / ".plan" / "keymap.json"
PLAN = ROOT / ".plan" / "plan.json"
WORKFLOWS = ROOT / ".github" / "workflows"

MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)


@dataclass(frozen=True)
class Problem:
    rule: str
    where: str
    message: str

    def line(self) -> str:
        return "  %-4s %s: %s" % (self.rule, self.where, self.message)


def _esc(text: str) -> str:
    return str(text).replace("|", "\\|")


def _semver(value: str) -> tuple[int, ...]:
    return tuple(int(p) for p in re.findall(r"\d+", value)[:3])


# ---------------------------------------------------------------------------
# renderers: ADR-002's generated blocks
# ---------------------------------------------------------------------------


def render_diagram() -> str:
    lines = ["```mermaid", "graph TD"]
    by_layer: dict[int, list[M.Crate]] = {}
    for crate in M.PROD_CRATES:
        by_layer.setdefault(crate.layer, []).append(crate)
    for layer in sorted(by_layer, reverse=True):
        crates = by_layer[layer]
        label = M.LAYER_LABELS.get(layer, "L%d" % layer)
        lines.append('  subgraph S%d["L%d · %s"]' % (layer, layer, label))
        for crate in sorted(crates, key=lambda c: c.name):
            lines.append('    %s["%s · %s"]' % (crate.name, crate.name, crate.kind))
        lines.append("  end")
    lines.append('  subgraph DEV["dev-only · never shipped"]')
    for crate in sorted(M.DEV_CRATES, key=lambda c: c.name):
        lines.append('    %s["%s · %s"]' % (crate.name, crate.name, crate.kind))
    lines.append("  end")
    for crate in M.CRATES:
        for dep in crate.allows:
            lines.append("  %s --> %s" % (crate.name, dep))
    lines.append('  classDef dev stroke-dasharray: 4')
    lines.append('  class %s dev' % M.FIXTURES_CRATE)
    lines.append("```")
    lines.append("")
    lines.append("Arrows are `[dependencies]`; every arrow points to a strictly lower layer (R1). "
                 "`%s` is dashed because it is a `[dev-dependencies]` crate and nothing else (R7) — "
                 "no production crate depends on it. There is no arrow from `fathom-tui` or "
                 "`fathom-mcp` to `fathom-store` even though the layering would allow it (§5.2), and "
                 "the two arrows that do exist from those crates are narrowed to `insight`/`export` by "
                 "§5.3." % M.FIXTURES_CRATE)
    return "\n".join(lines)


def render_crates() -> str:
    order = sorted(M.CRATES, key=lambda c: (-c.layer, c.name))
    rows = ["| Crate | Kind | Layer | Owner | May depend on | Non-optional externals | `unsafe` |",
            "|---|---|---|---|---|---|---|"]
    for c in order:
        rows.append("| `%s` | %s | L%d | %s | %s | %s | %s |" % (
            c.name, c.kind, c.layer, c.owner,
            ", ".join("`%s`" % a for a in c.allows) or "—",
            ", ".join("`%s`" % d for d in c.default_deps),
            "forbidden" if c.unsafe_policy == "forbid" else "`src/raw/` only, `cfg(mmap)` only"))
    for c in order:
        rows.append("")
        rows.append("**`%s`** (L%d, %s) — %s" % (c.name, c.layer, c.kind, c.purpose))
        for note in c.notes:
            rows.append("")
            rows.append("> %s" % note)
    return "\n".join(rows)


def render_features() -> str:
    rows = ["| Crate | Feature | In default build | Net | Heavy | What it turns on | Runtime consent key "
            "| Ticket |", "|---|---|---|---|---|---|---|---|"]
    for c in M.CRATES:
        for f in c.features:
            rows.append("| `%s` | `%s` | %s | %s | %s | %s | %s | %s |" % (
                c.name, f.name, "**yes**" if f.default else "no",
                "**yes**" if f.net else "—", "yes" if f.heavy else "—", _esc(f.summary),
                "`%s`" % M.NETWORK_CONSENT_KEYS[f.name] if f.name in M.NETWORK_CONSENT_KEYS else "—",
                f.ticket or "—"))
    rows.append("")
    rows.append("Transitive default feature set of the shipped binary "
                "(`cargo build -p %s`, nothing passed):" % M.BIN_CRATE)
    rows.append("")
    rows.append("```")
    rows.extend(item.rstrip("/") for item in sorted(M.cli_shipped_default_closure()))
    rows.append("```")
    rows.append("")
    heavy = sorted(set(M.closure_external_crates(M.cli_shipped_default_closure()))
                   & set(M.HEAVY_CRATES))
    net = sorted(set(M.closure_external_crates(M.cli_shipped_default_closure()))
                 & set(M.NETWORK_CRATES))
    rows.append("Linked by that build: %s. Network-capable crates linked: **%s** (R6)."
                % (", ".join("`%s`" % h for h in heavy) or "nothing heavy",
                   ", ".join("`%s`" % n for n in net) or "none"))
    return "\n".join(rows)


def render_rules() -> str:
    rows = ["| # | Rule | Enforced by |", "|---|---|---|"]
    for rid, text in M.rules_rows():
        rows.append("| **%s** | %s | %s |" % (rid, text, ENFORCED_BY.get(rid, "`check_workspace.py lint`")))
    return "\n".join(rows)


ENFORCED_BY = {
    "R1": "`lint` → `E01`, `E02`, `E03`, `E04`",
    "R2": "`lint` → `M02`, `M03`",
    "R3": "`lint` → `W1`, `W2`, `P02`, `F02`",
    "R4": "`lint` → `P02`, `P03`, `P04`",
    "R5": "`lint` → `F02`, `F03`, `H01`",
    "R6": "`lint` → `N01`–`N05`, `check` → shipped-closure test",
    "R7": "`lint` → `E05`",
    "R8": "`lint` → `T01`, `T06`",
    "R9": "`lint` → `W3`, `W4`",
    "R10": "`lint` → `W5`, `W6`; clippy `unwrap_used`",
    "R11": "`lint` → `F01`",
    "R12": "`check` → `C01`",
}


def render_toolchain() -> str:
    rows = ["| Setting | Value | Declared once in |", "|---|---|---|",
            "| edition | `%s` | `[workspace.package] edition` |" % M.EDITION,
            "| MSRV (`rust-version`) | `%s` | `[workspace.package] rust-version` |" % M.MSRV,
            "| edition floor | `%s` | edition 2024's own minimum, checked by `check` |"
            % M.EDITION_MIN_RUST,
            "| cargo resolver | `%s` — v2 semantics + MSRV-aware selection | `[workspace] resolver` |"
            % M.RESOLVER,
            "| local toolchain | pinned to the MSRV in `rust-toolchain.toml` | repo root |",
            "| clippy | `-D warnings` plus §8's strict set | `[workspace.lints]` |",
            "| rustfmt | `cargo fmt --check` is a hard failure | `rustfmt.toml` |",
            "| cargo-deny | licences + advisories + sources | `deny.toml` (§8) |"]
    rows.append("")
    rows.append("| MSRV policy clause | Statement |")
    rows.append("|---|---|")
    for clause, statement in M.MSRV_POLICY:
        rows.append("| %s | %s |" % (_esc(clause), _esc(statement)))
    rows.append("")
    rows.append("Observed toolchain while writing this ADR: `rustc -vV` reports release **1.99.0** "
                "(commit date 2026-09-28, `aarch64-unknown-linux-gnu`), and `gcc`/`cc` **are** present, "
                "so `sqlite-bundled` and the tree-sitter grammars build here. MSRV = latest − 2 = "
                "**1.97.0**.")
    return "\n".join(rows)


def render_tickets() -> str:
    keymap = json.loads(KEYMAP.read_text()) if KEYMAP.exists() else {}
    types, titles = {}, {}
    if PLAN.exists():
        for node in json.loads(PLAN.read_text())["nodes"]:
            types[node["key"]] = node.get("type", "task")
            titles[node["key"]] = node.get("title", "")
    rows = ["| Plan key | Bead | Type | Title | Lands in |", "|---|---|---|---|---|"]
    for key in sorted(M.TICKETS, key=M.plan_sort_key):
        rows.append("| `%s` | %s | %s | %s | %s |" % (
            key, keymap.get(key, "?"), types.get(key, "?"), titles.get(key, "?"),
            ", ".join("`%s`" % c for c in M.TICKETS[key])))
    return "\n".join(rows)


def render_exit_codes() -> str:
    rows = ["| Code | Name | Meaning |", "|---|---|---|"]
    for code, name, meaning in M.EXIT_CODES:
        rows.append("| `%d` | `%s` | %s |" % (code, name, _esc(meaning)))
    return "\n".join(rows)


def render_teams() -> str:
    rows = ["| # | Team | Crate | Modules owned | Scope |", "|---|---|---|---|---|"]
    for i, (slug, crate, modules, scope) in enumerate(M.TEAMS, 1):
        rows.append("| %d | `%s` | `%s` | %s | %s |" % (i, slug, crate, _esc(modules), _esc(scope)))
    return "\n".join(rows)


def render_ci() -> str:
    rows = ["| Required step | Text that must appear in `.github/workflows/*.yml` |", "|---|---|"]
    for name, needle in M.CI_REQUIRED_STEPS:
        rows.append("| `%s` | `%s` |" % (name, needle))
    return "\n".join(rows)


RENDERERS = {
    "diagram": render_diagram,
    "crates": render_crates,
    "features": render_features,
    "rules": render_rules,
    "toolchain": render_toolchain,
    "tickets": render_tickets,
    "exit-codes": render_exit_codes,
    "teams": render_teams,
    "ci": render_ci,
}


def render_adr(text: str) -> tuple[str, list[str]]:
    problems = []
    for name, renderer in RENDERERS.items():
        begin = "<!-- BEGIN GENERATED: %s -->" % name
        end = "<!-- END GENERATED: %s -->" % name
        if begin not in text or end not in text:
            problems.append("ADR-002 is missing generated block %r" % name)
            continue
        body = renderer()
        new = "%s\n%s\n%s" % (begin, body, end)
        text = re.sub(re.escape(begin) + r".*?" + re.escape(end), lambda _m, n=new: n, text, flags=re.S)
    return text, problems


# ---------------------------------------------------------------------------
# model self-consistency (`check`)
# ---------------------------------------------------------------------------


def check_model() -> list[Problem]:
    problems: list[Problem] = []
    add = problems.append

    for c in M.CRATES:
        where = c.name
        if not c.name.startswith("fathom-"):
            add(Problem("U01", where, "crate names are `fathom-*`"))
        if c.module != c.name.replace("-", "_"):
            add(Problem("U01", where, "the module name must be the crate name with underscores"))
        if not c.purpose.strip():
            add(Problem("U03", where, "every crate states its purpose in the ADR"))
        if c.kind == "bin" and not M.default_features(c):
            add(Problem("F03", where, "the binary crate must declare default features: they are the "
                        "shipped product (R5)"))
        if c.kind == "lib" and M.default_features(c):
            add(Problem("F02", where, "library crates declare `default = []` (R5), found %s"
                        % ", ".join(M.default_features(c))))
        if c.unsafe_policy not in ("forbid", "allow-in-raw"):
            add(Problem("P02", where, "unsafe_policy must be `forbid` or `allow-in-raw`"))
        if c.unsafe_policy == "allow-in-raw" and c.name not in M.UNSAFE_ALLOWED_PATHS:
            add(Problem("P02", where, "unsafe is permitted but no directory is declared for it"))
        if c.unsafe_policy == "forbid" and c.name in M.UNSAFE_ALLOWED_PATHS:
            add(Problem("P02", where, "unsafe is forbidden but a directory allows it"))
        for dep in c.allows:
            if dep not in M.CRATES_BY_NAME:
                add(Problem("E01", where, "allowed edge names unknown crate `%s`" % dep))
            elif M.kind_of(c.name) != "dev" and M.layer_of(dep) >= c.layer:
                add(Problem("E02", where, "allowed edge `%s -> %s` is not downhill (L%d -> L%d)"
                            % (where, dep, c.layer, M.layer_of(dep))))
        for dep in c.default_deps:
            if dep in M.NETWORK_CRATES:
                add(Problem("N01", where, "non-optional dependency `%s` can open a socket (R6)" % dep))
            if dep in M.HEAVY_CRATES:
                add(Problem("H01", where, "non-optional dependency `%s` is heavy; it must sit behind a "
                            "published feature (R5)" % dep))
        feats = M.features_by_name(c)
        if len(feats) != len(c.features):
            add(Problem("F01", where, "duplicate feature name"))
        for f in c.features:
            if f.net != f.name.startswith(M.NET_FEATURE_PREFIX):
                add(Problem("F04", where, "feature `%s`: `net-*` naming and the net flag disagree (R6)"
                            % f.name))
            if f.net and f.name not in M.NETWORK_CONSENT_KEYS:
                add(Problem("N06", where, "net feature `%s` has no runtime consent key (R6)" % f.name))
            for dep in f.deps:
                if f.net and dep not in M.NETWORK_CRATES:
                    add(Problem("N07", where, "net feature `%s` lists `%s`, which the model does not "
                                "classify as network-capable" % (f.name, dep)))
                if not f.net and dep in M.NETWORK_CRATES:
                    add(Problem("N01", where, "feature `%s` pulls network-capable `%s` without being "
                                "`net-*` (R6)" % (f.name, dep)))
                if dep in M.HEAVY_CRATES and not f.heavy:
                    add(Problem("H02", where, "feature `%s` pulls heavy `%s` but is not marked heavy"
                                % (f.name, dep)))
            for target in f.enables:
                head, _, sub = target.partition("/")
                if head not in M.CRATES_BY_NAME:
                    add(Problem("F06", where, "feature `%s` forwards to `%s`: not a crate in the model "
                                "(use `crate/feature`)" % (f.name, target)))
                elif sub and sub not in M.features_by_name(M.CRATES_BY_NAME[head]):
                    add(Problem("F06", where, "feature `%s` forwards to unknown feature `%s`"
                                % (f.name, target)))

    for src, dst, _why in M.FORBIDDEN_EDGES:
        if dst in M.ALLOWED_EDGES.get(src, ()):
            add(Problem("E03", src, "`-> %s` is both allowed and forbidden; pick one" % dst))
        if src not in M.CRATES_BY_NAME or dst not in M.CRATES_BY_NAME:
            add(Problem("E03", src, "forbidden edge names an unknown crate `%s`" % dst))

    visiting: list[str] = []
    done: set[str] = set()

    def visit(node: str) -> None:
        if node in visiting:
            add(Problem("E04", node, "dependency cycle: %s" % " -> ".join(visiting + [node])))
            return
        if node in done:
            return
        for dep in M.ALLOWED_EDGES.get(node, ()):
            visit(dep)
        done.add(node)

    for c in M.CRATES:
        visit(c.name)

    for facade_crate, facades in M.IMPORT_FACADES.items():
        for dep, prefixes in facades.items():
            if dep not in M.ALLOWED_EDGES.get(facade_crate, ()):
                add(Problem("M01", facade_crate, "facade declared for `%s`, which is not an allowed "
                            "dependency" % dep))
    named = {prefix for facades in M.IMPORT_FACADES.values() for prefixes in facades.values()
             for prefix in prefixes}
    for crate_name, module, _ticket in M.FACADE_MODULES:
        if "%s::%s" % (M.CRATES_BY_NAME[crate_name].module, module) not in named:
            add(Problem("M01", crate_name, "facade module `%s` is declared but no consumer is restricted "
                        "to it, so the boundary has nothing to enforce" % module))
    if _semver(M.MSRV) < _semver(M.EDITION_MIN_RUST):
        add(Problem("T01", "MSRV", "%s is below edition 2024's floor %s"
                    % (M.MSRV, M.EDITION_MIN_RUST)))
    codes = [code for code, _n, _m in M.EXIT_CODES]
    if len(set(codes)) != len(codes) or 0 not in codes:
        add(Problem("C02", "exit codes", "exit codes must be unique and must include 0"))
    if len(M.TEAMS) != 11:
        add(Problem("U04", "teams", "the plan is sized for eleven teams of one; %d declared"
                    % len(M.TEAMS)))
    for slug, crate, modules, _scope in M.TEAMS:
        if crate not in M.CRATES_BY_NAME:
            add(Problem("U04", slug, "team owns unknown crate `%s`" % crate))
        if not modules.strip():
            add(Problem("U04", slug, "team owns no modules, so nothing is actually owned"))

    shipped = M.cli_shipped_default_closure()
    for item in sorted(shipped):
        crate_name, _, feat = item.partition("/")
        crate = M.CRATES_BY_NAME.get(crate_name)
        feature = M.features_by_name(crate).get(feat) if crate else None
        if feature and feature.net:
            add(Problem("N03", M.BIN_CRATE, "the default build enables `net-*` feature `%s` (R6)"
                        % item))
    linked = M.closure_external_crates(shipped)
    net_linked = sorted(linked & set(M.NETWORK_CRATES))
    if net_linked:
        add(Problem("N03", M.BIN_CRATE, "the default build links network-capable %s (R6)"
                    % ", ".join("`%s`" % n for n in net_linked)))

    problems += check_ticket_map()
    problems += check_ci_wiring()
    if (ROOT / "deny.toml").exists():
        problems += lint_deny_toml(ROOT)
    return problems


def check_ticket_map() -> list[Problem]:
    if not (PLAN.exists() and KEYMAP.exists()):
        return [Problem("T02", ".plan", "plan.json/keymap.json missing, so the ticket->crate map cannot "
                        "be checked")]
    nodes = json.loads(PLAN.read_text())["nodes"]
    types = {n["key"]: n.get("type", "task") for n in nodes}
    problems: list[Problem] = []
    for key, crates in M.TICKETS.items():
        if key not in types:
            problems.append(Problem("T03", key, "mapped to a crate but absent from the plan"))
        for crate in crates:
            if crate != "workspace" and crate not in M.CRATES_BY_NAME:
                problems.append(Problem("T03", key, "mapped to unknown crate `%s`" % crate))
    for node in nodes:
        key, ntype = node["key"], node.get("type", "task")
        if ntype in M.TICKET_MAP_EXEMPT_TYPES or key in M.TICKETS:
            continue
        problems.append(Problem("T04", key, "a %s ticket has no crate in ADR-002 §11; add it to TICKETS "
                                "in the same change that adds the ticket" % ntype))
    used = set().union(*M.TICKETS.values())
    for crate in M.CRATES_BY_NAME:
        if crate not in used:
            problems.append(Problem("T05", crate, "no ticket in the plan builds this crate — why does "
                                    "it exist?"))
    return problems


def check_ci_wiring() -> list[Problem]:
    if not WORKFLOWS.exists():
        return [Problem("C01", ".github/workflows", "no CI workflow exists, so nothing can fail on an "
                        "uphill import; ADR-002 §8 is a hard gate, not a nicety")]
    text = "\n".join(p.read_text() for p in sorted(WORKFLOWS.glob("*.yml")))
    problems = []
    for name, needle in M.CI_REQUIRED_STEPS:
        if needle not in text:
            problems.append(Problem("C01", name, "`%s` appears in no workflow: the boundary lint, "
                                    "cargo-deny, fmt, clippy and the MSRV build must run on every PR"
                                    % needle))
    declared = re.findall(r'MSRV:\s*"([\d.]+)"', text)
    for value in declared:
        if value != M.MSRV:
            problems.append(Problem("T07", ".github/workflows", "workflow pins MSRV %s but ADR-002 §2 "
                                    "declares %s; there is one number, in the model" % (value, M.MSRV)))
    if not declared:
        problems.append(Problem("T07", ".github/workflows", "no `MSRV: ` value in any workflow, so the "
                                "MSRV build is building whatever the runner feels like"))
    return problems


# ---------------------------------------------------------------------------
# the boundary lint (`lint`)
# ---------------------------------------------------------------------------

DEP_SECTIONS = {"dependencies": "normal", "dev-dependencies": "dev", "build-dependencies": "build"}
#: parsed in this order so that a dependency declared in both `[dependencies]` and
#: `[dev-dependencies]` ends up recorded with its strongest (non-dev) kind; both sections are kept in
#: `kinds`, which is what rule `E05` needs.
DEP_PARSE_ORDER = ("build-dependencies", "dev-dependencies", "dependencies")
RUST_TEST_ATTR = re.compile(r"#\[[^\]]*\bcfg\s*\(\s*test\s*\)\s*\)")
CRATE_REF = re.compile(r"\bfathom_[a-z0-9_]+(?:::[a-zA-Z0-9_]+)*(?:::\{[^}]*\})?")
UNSAFE_RE = re.compile(r"\bunsafe\s+(fn|impl|block|extern|trait)\b|\bunsafe\s*\{")


def read_toml(path: pathlib.Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


class UnreadableManifest(Exception):
    """A `Cargo.toml` that does not parse: reported as `U07`, never as a traceback."""


@dataclass
class Manifest:
    crate: str
    path: pathlib.Path
    data: dict
    kind: str = "lib"
    deps: dict = field(default_factory=dict)
    features: dict = field(default_factory=dict)


def parse_manifest(crate_dir: pathlib.Path, ws: dict) -> Manifest:
    try:
        data = read_toml(crate_dir / "Cargo.toml")
    except Exception as exc:                    # a manifest cargo cannot read tells nobody anything
        raise UnreadableManifest("%s: %s" % (crate_dir.name, exc)) from exc
    name = data.get("package", {}).get("name", crate_dir.name)
    wsdeps = ws.get("workspace", {}).get("dependencies", {}) or {}
    deps: dict[str, dict] = {}
    for section in DEP_PARSE_ORDER:
        kind = DEP_SECTIONS[section]
        for dep_name, spec in (data.get(section) or {}).items():
            if isinstance(spec, str):
                spec = {"version": spec}
            base = dict(wsdeps.get(dep_name) or {}) if spec.get("workspace") else {}
            kinds = list(deps[dep_name]["kinds"]) if dep_name in deps else []
            if kind in kinds:
                continue
            kinds.append(kind)
            deps[dep_name] = {
                "kind": kind,
                "kinds": kinds,
                "section": "[%s]" % section,
                "optional": bool(spec.get("optional", False)),
                "features": list(spec.get("features", base.get("features", []))),
                "default_features": spec.get("default-features",
                                             base.get("default-features", True)),
                "internal": dep_name.startswith("fathom-"),
            }
    features = {k: list(v) for k, v in (data.get("features") or {}).items()}
    kind = "bin" if (crate_dir / "src" / "main.rs").exists() else "lib"
    if name == M.FIXTURES_CRATE:
        kind = "dev"
    return Manifest(crate=name, path=crate_dir, data=data, kind=kind, deps=deps, features=features)


def resolve_build(manifests: dict[str, Manifest], root: str) -> tuple[set, set, set]:
    """What `cargo build -p root` (default features) compiles: (crates, crate-features, externals)."""
    compiled: set[str] = set()
    enabled: set[tuple[str, str]] = set()
    externals: set[str] = set()

    def push_ref(crate_name: str, ref: str) -> None:
        head, _, sub = ref.partition("/")
        head = head[4:] if head.startswith("dep:") else head
        manifest = manifests.get(crate_name)
        if head.startswith("fathom-"):
            externals.discard(head)
            if sub:
                stack.append((head, sub))
            else:
                stack.append((head, "__default__"))
        elif manifest is not None and not sub and head in manifest.features:
            stack.append((crate_name, head))
        else:
            externals.add(head)

    stack: list[tuple[str, str]] = [(root, "__default__")]
    while stack:
        crate_name, feature = stack.pop()
        manifest = manifests.get(crate_name)
        if manifest is None:
            continue
        if feature == "__default__":
            if crate_name in compiled:
                continue
            compiled.add(crate_name)
            for dep, spec in manifest.deps.items():
                if spec["kind"] == "dev" or spec["optional"]:
                    continue
                if spec["internal"]:
                    stack.append((dep, "__default__"))
                else:
                    externals.add(dep)
            for ref in manifest.features.get("default", []):
                push_ref(crate_name, ref)
            continue
        if (crate_name, feature) in enabled:
            continue
        enabled.add((crate_name, feature))
        for ref in manifest.features.get(feature, []):
            push_ref(crate_name, ref)
    return compiled, enabled, externals


def _check_edge(crate: str, target: str, where: str, *, dev: bool = False,
                from_manifest: bool = False) -> list[Problem]:
    if target == crate or target not in M.CRATES_BY_NAME or crate not in M.CRATES_BY_NAME:
        return []
    model = M.CRATES_BY_NAME[crate]
    allowed = set(model.allows)
    if target in allowed:
        return []
    if target == M.FIXTURES_CRATE:
        if dev and not from_manifest:
            return []          # a fixture used from tests/ is the whole point of the crate
        return [Problem("E05", where, "`%s` is the fixture crate: `[dev-dependencies]` and `tests/` "
                        "only (R7)" % target)]
    direction = "uphill" if M.layer_of(target) >= M.layer_of(crate) else "forbidden"
    why = next((w for s, d, w in M.FORBIDDEN_EDGES if s == crate and d == target), "")
    detail = "" if direction == "uphill" else " (the layering allows it; ADR-002 §5 does not)"
    return [Problem("E01", where, "`%s` references `%s`: an %s edge (L%d -> L%d)%s. %s"
                    % (crate, target, direction, M.layer_of(crate), M.layer_of(target), detail, why))]


def _check_facade(crate: str, ref: str, where: str) -> list[Problem]:
    facades = M.IMPORT_FACADES.get(crate, {})
    parts = re.split(r"::|\{", ref)
    if len(parts) < 2:
        return []
    target = parts[0].replace("_", "-")
    if target not in facades:
        return []
    allowed = facades[target]
    second = parts[1]
    for prefix in allowed:
        if prefix.split("::")[1] == second:
            return []
    return [Problem("M02", where, "`%s` names `%s`: this crate may use `%s` only through %s (R2 — one "
                    "reality, ADR-001 §7.10)"
                    % (crate, ref, target, " or ".join("`::%s`" % p.split("::")[1] for p in allowed)))]


def scan_sources(manifest: Manifest) -> list[Problem]:
    problems: list[Problem] = []
    model = M.CRATES_BY_NAME.get(manifest.crate)
    if model is None:
        return []
    entry_name = "src/main.rs" if manifest.kind == "bin" else "src/lib.rs"
    for path in iter_rust_files(manifest.path):
        rel = path.relative_to(manifest.path).as_posix()
        in_tests = rel.startswith(("tests/", "benches/", "examples/"))
        try:
            lines = path.read_text().splitlines()
        except UnicodeDecodeError:
            continue
        depth = 0
        armed_at: int | None = None
        in_test_at: int | None = None
        for lineno, line in enumerate(lines, 1):
            where = "%s:%d" % (rel, lineno)
            in_test_block = in_test_at is not None
            stripped = line.strip()
            if not stripped.startswith("//"):
                for match in CRATE_REF.finditer(line):
                    ref = match.group(0).split("::{")[0]
                    target = ref.split("::")[0].replace("_", "-")
                    if not target.startswith("fathom-"):
                        continue
                    problems += _check_edge(manifest.crate, target, where, dev=in_tests,
                                            from_manifest=False)
                    problems += _check_facade(manifest.crate, ref, where)
                for rule in M.SOURCE_RULES:
                    if rule.applies != ("*",) and manifest.crate not in rule.applies:
                        continue
                    if rel in rule.skip_files:
                        continue
                    if rule.tests_exempt and (in_tests or in_test_block):
                        continue
                    if re.search(rule.pattern, line):
                        problems.append(Problem(rule.rule, "%s %s" % (manifest.crate, where), rule.why))
                if UNSAFE_RE.search(line) and not stripped.startswith("#![") and not in_tests \
                        and not in_test_block:
                    problems += _check_unsafe(manifest, rel, where)
            if RUST_TEST_ATTR.search(line):
                armed_at = depth
            depth += line.count("{") - line.count("}")
            if armed_at is not None and depth > armed_at:
                in_test_at, armed_at = armed_at, None
            if in_test_at is not None and depth <= in_test_at:
                in_test_at = None
    entry = manifest.path / entry_name
    if not entry.exists():
        problems.append(Problem("U01", manifest.crate, "no %s" % entry_name))
    else:
        problems += _check_attrs(manifest, entry, model)
    return problems


def _check_unsafe(manifest: Manifest, rel: str, where: str) -> list[Problem]:
    model = M.CRATES_BY_NAME[manifest.crate]
    if model.unsafe_policy == "forbid":
        return [Problem("P02", "%s %s" % (manifest.crate, where),
                        "`unsafe` in a crate that must be `#![forbid(unsafe_code)]` (R4)")]
    allowed = M.UNSAFE_ALLOWED_PATHS.get(manifest.crate, ())
    if not any(rel.startswith(prefix) for prefix in allowed):
        return [Problem("P02", "%s %s" % (manifest.crate, where),
                        "`unsafe` outside %s (R4)" % ", ".join("`%s`" % a for a in allowed))]
    body = (manifest.path / rel).read_text()
    where_full = "%s %s" % (manifest.crate, where)
    if 'feature = "mmap"' not in body:
        return [Problem("P03", where_full, "the unsafe-bearing file is not gated by "
                        "`#[cfg(feature = \"mmap\")]`; a default build must compile zero unsafe (R4)")]
    if "// SAFETY:" not in body:
        return [Problem("P04", where_full, "no `// SAFETY:` comment in an unsafe-bearing file")]
    return []


def _check_attrs(manifest: Manifest, entry: pathlib.Path, model) -> list[Problem]:
    text = re.sub(r"\s+", "", entry.read_text()[:6000])
    where = "%s %s" % (manifest.crate, entry.relative_to(manifest.path).as_posix())
    problems: list[Problem] = []
    wanted = list(M.REQUIRED_INNER_ATTRS["**"]) + list(M.REQUIRED_INNER_ATTRS.get(model.name, ()))
    for attr in wanted:
        if attr.replace(" ", "") not in text:
            problems.append(Problem("A01", where, "required inner attribute `#![...%s]` missing "
                                    "(ADR-002 §3, §7)" % attr))
    if "lints" not in manifest.data or str(manifest.data["lints"].get("workspace")).lower() != "true":
        problems.append(Problem("A02", "%s [lints]" % manifest.crate,
                                "`[lints] workspace = true` missing: the crate opts itself out of the "
                                "shared clippy policy (ADR-002 §7)"))
    return problems


def lint_manifest_shape(manifest: Manifest, manifests: dict[str, Manifest]) -> list[Problem]:
    problems: list[Problem] = []
    model = M.CRATES_BY_NAME.get(manifest.crate)
    if model is None:
        return [Problem("U01", manifest.crate, "not a crate ADR-002 §4 declares; change the ADR and the "
                        "model in the same PR that adds a crate")]
    pkg = manifest.data.get("package", {})
    for field_name, expected in (("edition", M.EDITION), ("rust-version", M.MSRV)):
        value = pkg.get(field_name)
        if value is None or isinstance(value, dict):
            continue          # a table means `edition.workspace = true`: inherited, which is the rule
        if str(value) != expected:
            problems.append(Problem("T01", manifest.crate, "`%s = \"%s\"` but ADR-002 §2 says `%s`; "
                                    "inherit it from `[workspace.package]`" % (field_name, value,
                                                                               expected)))
    for dep_name, spec in manifest.deps.items():
        if not dep_name.startswith("fathom-"):
            continue
        where = "%s %s" % (manifest.crate, spec["section"])
        if dep_name == M.FIXTURES_CRATE:
            non_dev = sorted(k for k in spec["kinds"] if k != "dev")
            if non_dev:
                problems.append(Problem("E05", where, "`%s` declared in %s (kind %s): the fixture crate "
                                        "is a dev-dependency and nothing else (R7)"
                                        % (dep_name, spec["section"], "/".join(non_dev))))
            continue
        problems += _check_edge(manifest.crate, dep_name, where, dev=spec["kind"] == "dev",
                                from_manifest=True)

    model_features = set(M.features_by_name(model))
    real_features = set(manifest.features) - {"default"}
    for extra in sorted(real_features - model_features):
        problems.append(Problem("F01", manifest.crate, "feature `%s` is not in ADR-002 §6's published "
                                "list (R11)" % extra))
    for missing in sorted(model_features - real_features):
        problems.append(Problem("F01", manifest.crate, "ADR-002 §6 publishes feature `%s` but the "
                                "manifest does not declare it (R11)" % missing))
    default = manifest.features.get("default", [])
    if model.kind == "lib" and default:
        problems.append(Problem("F02", manifest.crate, "a library crate declares `default = %s` (R5)"
                                % default))
    if model.kind == "bin" and set(default) != set(M.default_features(model)):
        problems.append(Problem("F03", manifest.crate, "the shipped default set is %s but ADR-002 §6 "
                                "publishes %s" % (sorted(default),
                                                  sorted(M.default_features(model)))))
    for ref in default:
        head = ref.partition("/")[0]
        if head not in manifest.features and head not in manifest.deps:
            problems.append(Problem("F07", manifest.crate, "`default` names `%s`, which is neither a "
                                    "feature nor a dependency" % ref))
    for fname, refs in manifest.features.items():
        for ref in refs:
            head, _, sub = ref.partition("/")
            head = head[4:] if head.startswith("dep:") else head
            if not head.startswith("fathom-"):
                continue
            target = manifests.get(head)
            if target is None:
                continue
            if sub and sub not in target.features:
                problems.append(Problem("F07", "%s [features]" % manifest.crate,
                                        "`%s` forwards to `%s`, which %s does not declare"
                                        % (fname, ref, head)))
        expanded, stack = set(), list(refs)
        while stack:
            ref = stack.pop()
            head = ref[4:] if ref.startswith("dep:") else ref.partition("/")[0]
            expanded.add(head)
            if head in manifest.features and head != fname:
                stack.extend(manifest.features[head])
        for group in ({"sqlite-bundled", "sqlite-system"},):
            if group <= expanded:
                problems.append(Problem("F05", "%s [features] `%s`" % (manifest.crate, fname),
                                        "features %s are mutually exclusive but both are enabled"
                                        % sorted(group)))

    compiled, enabled, externals = resolve_build(manifests, manifest.crate)
    net_linked = sorted(externals & set(M.NETWORK_CRATES))
    if net_linked:
        problems.append(Problem("N03", manifest.crate, "the default build links network-capable %s (R6)"
                                % ", ".join("`%s`" % n for n in net_linked)))
    heavy_linked = sorted(externals & set(M.HEAVY_CRATES))
    for dep_name, spec in manifest.deps.items():
        where = "%s %s" % (manifest.crate, spec["section"])
        if dep_name in M.NETWORK_CRATES:
            if not spec["optional"]:
                problems.append(Problem("N01", where, "`%s` is non-optional and can open a socket; it "
                                        "must be optional and behind a `net-*` feature (R6)" % dep_name))
            hosts = sorted(f for f, refs in manifest.features.items()
                           if any(r.partition("/")[0] == dep_name or r == dep_name for r in refs))
            if not all(h.startswith(M.NET_FEATURE_PREFIX) for h in hosts):
                problems.append(Problem("N02", where, "`%s` is enabled by non-`net-*` features %s (R6)"
                                        % (dep_name, hosts or "none")))
        if dep_name == "tokio":
            bad = [f for f in spec["features"] if f not in M.ALLOWED_TOKIO_FEATURES]
            if bad:
                problems.append(Problem("N04", where, "tokio features %s are off the allow-list; `net` "
                                        "in particular opens the socket the whole policy exists to keep "
                                        "shut" % bad))
            if spec["default_features"] is not False:
                problems.append(Problem("N04", where, "tokio must be `default-features = false`"))
        if dep_name == "gix":
            if spec["default_features"] is not False:
                problems.append(Problem("N05", where, "gix must be `default-features = false`: fathom "
                                        "reads the local object database and never contacts a remote"))
            bad = [f for f in spec["features"] if f not in M.GIX_ALLOWED_FEATURES]
            if bad:
                problems.append(Problem("N05", where, "gix features %s are off the local-only allow-list"
                                        % bad))
        if dep_name in M.HEAVY_CRATES and not spec["optional"] and dep_name in heavy_linked:
            problems.append(Problem("H01", where, "`%s` is heavy and non-optional; gate it behind a "
                                    "published feature (R5)" % dep_name))
    problems += scan_sources(manifest)
    return problems


def lint_deny_toml(root: pathlib.Path) -> list[Problem]:
    path = root / "deny.toml"
    if not path.exists():
        return [Problem("D01", "deny.toml", "missing: cargo-deny is the licence/advisory gate (§8)")]
    data = read_toml(path)
    problems: list[Problem] = []
    licences = data.get("licenses", {})
    allow = set(licences.get("allow", []))
    missing = sorted({"MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib"}
                     - allow)
    if missing:
        problems.append(Problem("D02", "deny.toml", "[licenses] allow is missing %s" % ", ".join(missing)))
    # cargo-deny 0.20 removed `[licenses] deny` (EmbarkStudios/cargo-deny#611), so an allow-list that
    # omits a copyleft licence *is* the refusal. The rule moved with the tool instead of continuing to
    # ask for a key the tool rejects: the licence must be absent from `allow` **and** named somewhere in
    # the file, so the decision is written down rather than implied. Recorded in ADR-002 §10.3 by
    # `e0.7`, which is the ticket that pinned a real cargo-deny and ran it.
    deny_text = path.read_text()
    for copyleft in ("AGPL-3.0", "GPL-3.0"):
        if copyleft in allow:
            problems.append(Problem("D03", "deny.toml", "`%s` is in [licenses] allow: copyleft is "
                                    "refused by this project (ADR-002 §8.4)" % copyleft))
        elif copyleft not in set(licences.get("deny", [])) and copyleft not in deny_text:
            problems.append(Problem("D03", "deny.toml", "`%s` is nowhere in this file: an allow-list "
                                    "refuses a licence by omission, and an omission a reviewer cannot "
                                    "find is the same as one nobody made" % copyleft))
    if "confidence-threshold" not in licences:
        problems.append(Problem("D04", "deny.toml", "[licenses] confidence-threshold missing"))
    if data.get("advisories", {}).get("yanked") in (False, "warn"):
        problems.append(Problem("D05", "deny.toml", "[advisories] yanked must deny: a yanked dependency "
                                "is a supply-chain event, not a style opinion"))
    sources = data.get("sources", {})
    for key in ("unknown-git", "unknown-registry"):
        if sources.get(key) not in (None, "deny", "error"):
            problems.append(Problem("D06", "deny.toml", "[sources] %s must deny" % key))
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        key = re.match(r"^\s*(ignore|exception)\s*=\s*\[(.*)\]\s*$", line)
        if key and re.findall(r'"([^"]+)"', key.group(2)) and "#" not in line:
            problems.append(Problem("D07", "deny.toml:%d" % lineno,
                                    "a `%s` entry has no `# why:` comment; every suppressed advisory or "
                                    "licence exception is signed, because an unsigned ignore is how a "
                                    "temporary waiver becomes permanent" % key.group(1)))
    banned = json.dumps(data.get("bans", {}).get("deny", []))
    for never in ("libgit2-sys", "openssl-sys", "git2"):
        if never not in banned:
            problems.append(Problem("D08", "deny.toml", "[bans] deny must name `%s`: one path to git "
                                    "objects (gix) and no TLS stack that could reach a remote (§4.2)"
                                    % never))
    return problems


def lint(root: pathlib.Path) -> list[Problem]:
    crates_dir = root / "crates"
    if not crates_dir.is_dir():
        print("note: %s has no crates/ directory — the workspace is scaffolded by e0.7 (`reco-e77`), "
              "so the boundary lint has nothing to check yet" % root)
        return []
    ws = read_toml(root / "Cargo.toml") if (root / "Cargo.toml").exists() else {}
    if ws:
        wst = ws.get("workspace", {})
        fatal = []
        if str(wst.get("resolver")) != M.RESOLVER:
            fatal.append(Problem("T06", "Cargo.toml", "[workspace] resolver must be \"%s\" "
                                 "(MSRV-aware selection), found %r" % (M.RESOLVER, wst.get("resolver"))))
        ws_pkg = wst.get("package", {})
        if str(ws_pkg.get("edition")) != M.EDITION:
            fatal.append(Problem("T01", "Cargo.toml", "[workspace.package] edition must be %s"
                                 % M.EDITION))
        if str(ws_pkg.get("rust-version")) != M.MSRV:
            fatal.append(Problem("T01", "Cargo.toml", "[workspace.package] rust-version must be %s"
                                 % M.MSRV))
        if "lints" not in ws.get("workspace", {}):
            fatal.append(Problem("A02", "Cargo.toml", "[workspace.lints] missing: the shared clippy "
                                 "policy lives in exactly one place (§7)"))
        if not (crates_dir / M.BIN_CRATE).is_dir():
            fatal.append(Problem("U06", "Cargo.toml", "the workspace must contain `crates/%s`: one "
                                 "binary, one version, one artefact (§4)" % M.BIN_CRATE))
        if fatal:
            return fatal
    members = ws.get("workspace", {}).get("members", [])
    manifests: dict[str, Manifest] = {}
    for crate_dir in sorted(p for p in crates_dir.iterdir() if (p / "Cargo.toml").is_file()):
        if "crates/*" not in members and ("crates/%s" % crate_dir.name) not in members:
            return [Problem("U05", "Cargo.toml", "workspace.members does not cover crates/%s"
                            % crate_dir.name)]
        try:
            manifest = parse_manifest(crate_dir, ws)
        except UnreadableManifest as exc:
            return [Problem("U07", "crates/%s" % crate_dir.name,
                            "Cargo.toml does not parse: %s" % exc)]
        if manifest.crate != crate_dir.name:
            return [Problem("U07", "crates/%s" % crate_dir.name,
                            "package name `%s` does not match the directory name: the lint, the "
                            "error messages and `cargo tree` all key on the crate name"
                            % (manifest.crate))]
        if manifest.crate in manifests:
            return [Problem("U07", "crates/%s" % crate_dir.name,
                            "package name `%s` is declared by two crate directories"
                            % (manifest.crate))]
        manifests[manifest.crate] = manifest
    for name in sorted(set(M.CRATES_BY_NAME) - set(manifests)):
        print("note: ADR-002 §4 declares `%s` and crates/ does not have it yet" % name)
    problems: list[Problem] = []
    for manifest in manifests.values():
        problems += lint_manifest_shape(manifest, manifests)
    problems += lint_deny_toml(root)
    return problems


def iter_rust_files(crate_dir: pathlib.Path):
    for subdir in ("src", "tests", "benches", "examples"):
        base = crate_dir / subdir
        if base.is_dir():
            for path in sorted(base.rglob("*.rs")):
                yield path


# ---------------------------------------------------------------------------
# scaffold: emit the shape ADR-002 declares
# ---------------------------------------------------------------------------

DENY_TOML = '''# cargo-deny configuration — the licence, advisory, source and ban gate ADR-002 §8 requires.
# Written by hand and checked by `python3 tools/check_workspace.py lint` (rules D01-D08); CI runs
# `cargo deny check` over the resolved lockfile. Every ignore/exception below must carry a `# why:`.

[graph]
targets = [
  "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
  "x86_64-apple-darwin", "aarch64-apple-darwin",
  "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc",
]
all-features = true

[advisories]
version = 2
yanked = "deny"
ignore = []

[licenses]
version = 2
confidence-threshold = 0.8
allow = [
  "MIT",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Unicode-3.0",
  "Zlib",
  "MPL-2.0",
  "MIT OR Apache-2.0",
]
deny = ["AGPL-3.0", "GPL-3.0", "SSPL-1.0"]

[licenses.private]
ignore = true    # why: our own crates are `publish = false` and carry the workspace licence

[bans]
multiple-versions = "warn"
wildcards = "deny"
highlight = "all"
deny = [
  { name = "git2",         reason = "ADR-002 §4.2: gix is the only path to git objects" },
  { name = "libgit2-sys",  reason = "ADR-002 §4.2: libgit2 links a transport stack we would have to audit" },
  { name = "openssl-sys",  reason = "ADR-002 §6: no TLS stack outside a `net-*` feature" },
  { name = "native-tls",   reason = "ADR-002 §6: platform TLS only inside a `net-*` feature" },
  { name = "hyper",        reason = "ADR-002 §6: an HTTP stack may only arrive via `net-http-transport`" },
]
skip = []

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
'''

TOOLCHAIN_TOML = '''# Local builds use the MSRV, so a dependency that needs newer Rust is a local discovery and not a
# user's bug report. CI additionally builds on `stable` and `beta` (ADR-002 §2, §8).
[toolchain]
channel = "%s"
components = ["rustfmt", "clippy"]
'''


def _feature_snippet(crate: M.Crate, feature: M.Feature) -> tuple[list[str], set[str]]:
    """(feature references, crates that must be optional) for one feature line."""
    refs: list[str] = []
    optional: set[str] = set()
    for target in feature.enables:
        head, _, sub = target.partition("/")
        if sub:
            refs.append("%s/%s" % (head, sub))
        else:
            refs.append("dep:%s" % head)
            optional.add(head)
    for dep in sorted(feature.deps):
        optional.add(dep)
        refs.append(dep if refs else "dep:%s" % dep)
    return refs, optional


def scaffold(root: pathlib.Path, force: bool = False) -> list[str]:
    written: list[str] = []

    def write(path: pathlib.Path, text: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        written.append(path.relative_to(root).as_posix())

    if root.exists() and any(root.iterdir()) and not force:
        raise SystemExit("refusing to scaffold into a non-empty %s (pass --force)" % root)

    externals = sorted({d for c in M.CRATES for d in c.default_deps} |
                       {d for c in M.CRATES for f in c.features for d in f.deps})
    optional_externals = {d for c in M.CRATES for f in c.features for d in f.deps}
    optional_internal: dict[str, set[str]] = {}
    for crate in M.CRATES:
        for feature in crate.features:
            for target in feature.enables:
                head, _, sub = target.partition("/")
                if not sub and head in crate.allows:
                    optional_internal.setdefault(crate.name, set()).add(head)

    lines = ["[workspace]", 'resolver = "%s"' % M.RESOLVER, 'members = ["crates/*"]', "",
             "[workspace.package]", 'edition = "%s"' % M.EDITION,
             'rust-version = "%s"' % M.MSRV, 'version = "0.1.0"',
             'license = "MIT OR Apache-2.0"', "publish = false", "", "[workspace.dependencies]"]
    for crate in M.CRATES:
        if crate.kind == "bin":
            continue
        lines.append('%s = { path = "crates/%s", version = "0.1.0" }' % (crate.name, crate.name))
    for dep in externals:
        lines.append('%s = { version = "0.0.0", default-features = false }  # e0.7 pins the real version'
                      % dep)
    lines += ["", "[workspace.lints.rust]", "unsafe_op_in_unsafe_fn = \"deny\"",
              "missing_docs = \"warn\"", "", "[workspace.lints.clippy]",
              "unwrap_used = \"warn\"", "expect_used = \"warn\"",
              "undocumented_unsafe_blocks = \"deny\"", "dbg_macro = \"deny\"",
              "print_stdout = \"warn\"", "# ADR-002 §8: CI runs the strict set with `-D warnings`"]
    write(root / "Cargo.toml", "\n".join(lines) + "\n")
    write(root / "rust-toolchain.toml", TOOLCHAIN_TOML % M.MSRV)
    write(root / "rustfmt.toml", "edition = \"%s\"\n" % M.EDITION)
    write(root / "deny.toml", DENY_TOML)

    for crate in M.CRATES:
        optional = optional_externals & set(M.declared_external_deps(crate)) | \
            optional_internal.get(crate.name, set())
        for feature in crate.features:
            _refs, opt = _feature_snippet(crate, feature)
            optional |= opt
        man = ["[package]", 'name = "%s"' % crate.name, "version.workspace = true",
               "edition.workspace = true", "rust-version.workspace = true",
               "license.workspace = true", "publish = false", "", "[lints]", "workspace = true",
               "", "[dependencies]"]
        for dep in crate.default_deps:
            if dep not in optional:
                man.append("%s.workspace = true" % dep)
        for dep in crate.allows:
            if dep in optional:
                man.append('%s = { workspace = true, optional = true }' % dep)
            else:
                man.append("%s.workspace = true" % dep)
        for dep in sorted(optional):
            if dep not in crate.default_deps and dep not in crate.allows:
                man.append('%s = { workspace = true, optional = true }' % dep)
        if crate.features:
            man += ["", "[features]", "default = [%s]"
                    % ", ".join('"%s"' % f for f in M.default_features(crate))]
            for feature in crate.features:
                refs, _opt = _feature_snippet(crate, feature)
                man.append("%s = [%s]" % (feature.name, ", ".join('"%s"' % r for r in refs)))
        if crate.kind in ("lib", "dev"):
            man += ["", "[dev-dependencies]"]
            if crate.name != M.FIXTURES_CRATE:
                man.append("%s.workspace = true" % M.FIXTURES_CRATE)
        write(root / "crates" / crate.name / "Cargo.toml", "\n".join(man) + "\n")

        entry = "main.rs" if crate.kind == "bin" else "lib.rs"
        unsafe_attr = ("#![deny(unsafe_code)]" if crate.unsafe_policy == "allow-in-raw"
                       else "#![forbid(unsafe_code)]")
        body = ["//! `%s` — %s" % (crate.name, re.sub(r"\s+", " ", crate.purpose)),
                "//!", "//! Shape emitted by `python3 tools/check_workspace.py scaffold` (ADR-002 §10).",
                "//! The prose, the module map and the doc comments are `e0.7`'s to write.", "",
                unsafe_attr, "#![deny(unsafe_op_in_unsafe_fn)]", ""]
        if crate.name == "fathom-store":
            body += ['pub(crate) mod raw;', ""]
            write(root / "crates/fathom-store/src/raw/mod.rs",
                  "//! The only place in this workspace where `unsafe` is permitted (ADR-002 §3, R4).\n"
                  "\n#[cfg(feature = \"mmap\")]\npub(crate) mod mmap;\n")
            write(root / "crates/fathom-store/src/raw/mmap.rs",
                  "//! mmap-backed index reads (ADR-004 §13.1, ADR-005 §10).\n"
                  "//!\n"
                  "//! This file is the workspace's only `unsafe` and it is feature-gated, so a default\n"
                  "//! build compiles zero unsafe (ADR-002 R4).\n"
                  "#![cfg(feature = \"mmap\")]\n\n"
                  "// SAFETY: the caller guarantees `len > 0`, that the mapping is dropped before the\n"
                  "// file is closed, and that the returned pointer is never dereferenced past `len`\n"
                  "// (ADR-005 §10's probe runs first and refuses a host that cannot honour the map).\n"
                  "pub unsafe fn map(_path: &str, _len: usize) -> *mut u8 {\n"
                  "    core::ptr::null_mut()\n"
                  "}\n")
        if crate.name == "fathom-analyze":
            body += ["pub mod insight;", "pub mod export;", ""]
            for facade_crate, module, ticket in M.FACADE_MODULES:
                if facade_crate != crate.name:
                    continue
                write(root / "crates" / crate.name / ("src/%s.rs" % module),
                      "//! `%s::%s` — the facade ADR-002 §5 publishes (%s).\n"
                      % (crate.module, module, ticket))
        if crate.kind == "bin":
            body += ["fn main() -> eyre::Result<()> {", "    Ok(())", "}", ""]
        write(root / "crates" / crate.name / "src" / entry, "\n".join(body))

        if crate.kind == "lib":
            write(root / "crates" / crate.name / "tests" / "boundary.rs",
                  "//! ADR-002 §5 (R7): the fixture crate is reachable from here and from nowhere in\n"
                  "//! `src/`. `tools/check_workspace.py lint` fails the other two ways.\n"
                  "use fathom_fixtures as _;\n\n#[test]\nfn fixtures_link()\n{}\n")
    return written


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def _report(problems: list[Problem], label: str, quiet_ok: bool = False) -> int:
    if not problems:
        if not quiet_ok:
            print("%s OK" % label)
        return 0
    print("%s: %d problem(s)" % (label, len(problems)))
    for problem in problems:
        print(problem.line())
    return 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="check_workspace.py", description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    sub.add_parser("check")
    write_p = sub.add_parser("write")
    write_p.add_argument("--check-only", action="store_true")
    lint_p = sub.add_parser("lint")
    lint_p.add_argument("root", nargs="?", default=str(ROOT))
    lint_p.add_argument("--with-model", action="store_true",
                        help="also run the model self-checks (CI does)")
    sc_p = sub.add_parser("scaffold")
    sc_p.add_argument("root")
    sc_p.add_argument("--force", action="store_true")
    sub.add_parser("shipped")
    args = parser.parse_args(argv[1:])

    if args.mode == "check":
        problems = check_model()
        rc = _report(problems, "ADR-002 model")
        if not ADR.exists():
            print("docs/adr/0002-workspace.md does not exist yet")
            return rc or 1
        original = ADR.read_text()
        updated, render_problems = render_adr(original)
        if render_problems:
            print("\n".join("  " + p for p in render_problems))
            rc = 1
        if updated != original:
            print("  ADR001: docs/adr/0002-workspace.md has drifted from tools/workspace_model.py; "
                  "run `python3 tools/check_workspace.py write`")
            rc = 1
        if rc == 0:
            print("check_workspace check OK: %d crates, %d features, %d teams, %d tickets mapped, "
                  "%d generated blocks in sync"
                  % (len(M.CRATES), sum(len(c.features) for c in M.CRATES), len(M.TEAMS),
                     len(M.TICKETS), len(RENDERERS)))
        return rc

    if args.mode == "write":
        original = ADR.read_text()
        updated, render_problems = render_adr(original)
        if args.check_only:
            return _report([Problem("R01", p, "") for p in render_problems], "render")
        ADR.write_text(updated)
        print("rendered %s: %s" % (ADR.relative_to(ROOT), ", ".join(RENDERERS)))
        for p in render_problems:
            print("warning: " + p)
        return 1 if render_problems else 0

    if args.mode == "lint":
        problems = lint(pathlib.Path(args.root))
        if args.with_model:
            problems = problems + check_model()
        return _report(problems, "workspace boundary lint (%s)" % args.root)

    if args.mode == "scaffold":
        written = scaffold(pathlib.Path(args.root), force=args.force)
        print("scaffolded %d files under %s" % (len(written), args.root))
        for name in written:
            print("  " + name)
        return 0

    closure = M.cli_shipped_default_closure()
    print("cargo build -p %s (default features) enables:" % M.BIN_CRATE)
    for item in sorted(closure):
        print("  " + item.rstrip("/"))
    print("\nexternal crates linked:")
    for dep in sorted(M.closure_external_crates(closure)):
        tag = []
        if dep in M.NETWORK_CRATES:
            tag.append("NETWORK")
        if dep in M.HEAVY_CRATES:
            tag.append("heavy")
        print("  %s%s" % (dep, (" [%s]" % ",".join(tag)) if tag else ""))
    print("\nnetwork-capable crates in the default build: %s (R6)"
          % (sorted(set(M.closure_external_crates(closure)) & set(M.NETWORK_CRATES)) or "none"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(list(sys.argv)))
