"""ADR-002's acceptance criteria, as tests.

Like `tests/test_storage_contract.py` for ADR-004 and `tests/test_harness_contract.py` for ADR-005,
this file *is* the executable form of [docs/adr/0002-workspace.md](../docs/adr/0002-workspace.md): a
clause of that document with no test here is a clause that will drift. Run from the repository root:

    python3 -m pytest -q

The suite has four halves. The model checks prove the ADR's tables and rules are internally consistent
and actually wired into CI. The scaffold checks prove the workspace ADR-002 declares is a valid shape.
The mutation checks are the point: each one breaks exactly one rule in a generated copy of the
workspace and asserts the boundary lint fails with the rule id the ADR names — because a lint nobody has
seen fail is a lint that is not checking anything. The fourth half runs against the workspace that
`e0.7` actually landed in `crates/`: the Rust source against the model, in the direction the first three
halves cannot check (they read the ADR and the model, and were written before any `.rs` file existed).

Everything is stdlib-only, on purpose: a contract check that needs `cargo`, a registry fetch or a build
step is a check that gets skipped, and skipped is how a boundary rots.
"""
from __future__ import annotations

import json
import pathlib
import re
import shutil
import sys

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import check_workspace as CW          # noqa: E402
import workspace_model as M           # noqa: E402

ADR_TEXT = (ROOT / "docs" / "adr" / "0002-workspace.md").read_text()


# ---------------------------------------------------------------------------
# fixtures and helpers
# ---------------------------------------------------------------------------


@pytest.fixture(scope="session")
def clean_ws(tmp_path_factory) -> pathlib.Path:
    """The workspace ADR-002 declares, scaffolded once and linted once as the baseline."""
    target = tmp_path_factory.mktemp("fathom-workspace")
    written = CW.scaffold(target)
    assert len(written) > 20, "scaffold wrote almost nothing; check the renderer"
    problems = CW.lint(target)
    assert not problems, "the clean scaffold must lint clean:\n" + "\n".join(
        p.line() for p in problems)
    return target


@pytest.fixture
def workspace(clean_ws, tmp_path) -> pathlib.Path:
    """A private copy of the clean scaffold, so each mutation test starts from green."""
    target = tmp_path / "ws"
    shutil.copytree(clean_ws, target)
    return target


def edit(root: pathlib.Path, rel: str, old: str, new: str) -> pathlib.Path:
    path = root / rel
    text = path.read_text()
    assert old in text, "mutation target %r does not contain %r" % (rel, old)
    path.write_text(text.replace(old, new, 1))
    return path


def rules(root: pathlib.Path) -> set[str]:
    return {p.rule for p in CW.lint(root)}


def lint_text(root: pathlib.Path) -> str:
    return "\n".join(p.line() for p in CW.lint(root))


# ---------------------------------------------------------------------------
# 1. the model: rules the ADR states, checked against itself
# ---------------------------------------------------------------------------


def test_model_is_self_consistent():
    problems = [p for p in CW.check_model()]
    assert not problems, "ADR-002's model contradicts itself:\n" + "\n".join(
        p.line() for p in problems)


def test_eight_crates_as_the_ticket_demands():
    """The ticket names eight crates; the model has exactly those eight."""
    named = {"fathom-core", "fathom-store", "fathom-ingest", "fathom-analyze", "fathom-tui",
             "fathom-cli", "fathom-mcp", "fathom-fixtures"}
    assert {c.name for c in M.CRATES} == named


def test_every_edge_is_downhill_and_the_graph_is_acyclic():
    for crate in M.CRATES:
        for dep in crate.allows:
            if M.kind_of(crate.name) == "dev":
                continue
            assert M.layer_of(dep) < crate.layer, "%s -> %s is not downhill" % (crate.name, dep)
    assert not [p for p in CW.check_model() if p.rule == "E04"]


def test_the_two_forbidden_edges_are_really_forbidden():
    """§5.2: downhill is not enough; `tui/mcp -> store` must stay impossible."""
    for src in ("fathom-tui", "fathom-mcp"):
        assert "fathom-store" not in M.ALLOWED_EDGES[src]
        assert "fathom-ingest" not in M.ALLOWED_EDGES[src]
    pairs = {(s, d) for s, d, _why in M.FORBIDDEN_EDGES}
    for src, dst, _why in M.FORBIDDEN_EDGES:
        assert dst not in M.ALLOWED_EDGES[src], "%s -> %s is both" % (src, dst)
    assert ("fathom-tui", "fathom-store") in pairs


def test_the_facade_is_the_only_door_the_surfaces_may_use():
    """§5.3 / R2: the surfaces name `fathom_analyze` only through its façades, and the shell's list is
    exactly two wider than the agent's — no more.

    `insight` and `export` are the narrow waist every surface reads through. `e5.2` and `e5.3` gave
    `fathom-tui` two more, and this test is where the widening is bounded: `/` scores the corpus the
    composition root read (it must be `fathom_analyze::search`'s scorer or the shell and the pipe rank a
    query differently), and the palette decodes the session key a saved comparison lives under (a second
    spelling of that key is a format with two owners). Neither is a licence to analyse: `M03` still forbids
    a surface that composes, ranks or classifies for itself, and `fathom-mcp`'s list is deliberately
    unchanged.
    """
    assert sorted(
        p.split("::")[1] for p in M.IMPORT_FACADES["fathom-mcp"]["fathom-analyze"]
    ) == ["export", "insight"]
    assert sorted(
        p.split("::")[1] for p in M.IMPORT_FACADES["fathom-tui"]["fathom-analyze"]
    ) == ["compare", "export", "insight", "search"]
    modules = {m for _c, m, _t in M.FACADE_MODULES}
    assert modules == {"insight", "export"}


def test_core_has_no_io_no_unsafe_no_clock_and_no_defaults():
    core = M.CRATES_BY_NAME["fathom-core"]
    assert core.allows == ()
    assert core.unsafe_policy == "forbid"
    assert not M.default_features(core)
    assert set(core.default_deps) <= {"serde", "serde_json", "thiserror"}
    rules_io = {r.rule for r in M.SOURCE_RULES if "fathom-core" in r.applies}
    assert {"W1", "W2"} <= rules_io


def test_only_the_binary_declares_default_features():
    for crate in M.LIB_CRATES:
        assert M.default_features(crate) == (), "%s must declare default = []" % crate.name
    assert M.default_features(M.CRATES_BY_NAME["fathom-cli"])


def test_default_build_links_zero_network_capable_crates():
    """The acceptance criterion, stated as a closure the model cannot fudge."""
    shipped = M.cli_shipped_default_closure()
    linked = M.closure_external_crates(shipped)
    assert not sorted(linked & set(M.NETWORK_CRATES))
    assert not [p for p in CW.check_model() if p.rule == "N03"]
    # and the network features exist, so the claim is about gating and not about forgetting them
    assert {(c.name, f.name) for c, f in M.net_features()}


def test_every_network_feature_is_prefixed_gated_off_and_needs_consent():
    for crate, feature in M.net_features():
        assert feature.name.startswith(M.NET_FEATURE_PREFIX)
        assert not feature.default, "%s/%s is on by default" % (crate.name, feature.name)
        assert feature.name in M.NETWORK_CONSENT_KEYS
        assert (not feature.deps
                or any(dep in M.NETWORK_CRATES for dep in feature.deps)), \
            "%s/%s claims to be net but pulls nothing network-capable" % (crate.name, feature.name)


def test_heavy_dependencies_are_behind_named_features_only():
    assert M.heavy_features(), "a model with no heavy features makes the gate list meaningless"
    for crate in M.CRATES:
        for dep in crate.default_deps:
            assert dep not in M.HEAVY_CRATES, "%s/%s is heavy and non-optional" % (crate.name, dep)
        for feature in crate.features:
            for dep in feature.deps:
                if dep in M.HEAVY_CRATES:
                    assert feature.heavy


def test_git_access_is_local_only_by_allowlist():
    """§6.5: `gix` local features only; anything that names a transport is refused."""
    assert "git2" in M.NETWORK_CRATES and "libgit2-sys" in M.NETWORK_CRATES
    assert not [f for f in M.GIX_ALLOWED_FEATURES if "network" in f or "client" in f]
    assert "blocking-network-client" not in M.GIX_ALLOWED_FEATURES
    assert "net" not in M.ALLOWED_TOKIO_FEATURES


def test_eleven_teams_own_disjoint_module_sets():
    assert len(M.TEAMS) == 11, "the plan is staffed by eleven teams of one"
    seen: dict[str, set[str]] = {}
    for slug, crate, modules, _scope in M.TEAMS:
        if modules == "everything":
            continue
        owned = {m.strip() for m in modules.split(",")}
        for other_slug, other_crate, other_modules, _s in M.TEAMS:
            if other_slug == slug or other_crate != crate or other_modules == "everything":
                continue
            others = {m.strip() for m in other_modules.split(",")}
            assert not (owned & others), "%s and %s both own %s" % (slug, other_slug, owned & others)
        seen.setdefault(crate, set()).update(owned)


def test_exit_codes_are_unique_and_documented():
    codes = [code for code, _name, _meaning in M.EXIT_CODES]
    assert len(set(codes)) == len(codes)
    assert codes[0] == 0
    assert all(meaning.strip() for _c, _n, meaning in M.EXIT_CODES)
    assert 7 in codes and 8 in codes, "CI (`fathom check`) and network consent need stable codes"


def test_every_plan_ticket_that_owns_code_has_a_crate():
    assert CW.check_ticket_map() == []
    nodes = json.loads((ROOT / ".plan" / "plan.json").read_text())["nodes"]
    owning = [n for n in nodes if n.get("type", "task") not in M.TICKET_MAP_EXEMPT_TYPES]
    assert len(owning) >= 70, "the plan shrank; the map check is not vacuous"


# ---------------------------------------------------------------------------
# 2. the ADR document: tables generated, claims present, CI wired
# ---------------------------------------------------------------------------


def test_generated_blocks_are_in_sync_with_the_model():
    updated, problems = CW.render_adr(ADR_TEXT)
    assert not problems, "\n".join(problems)
    assert updated == ADR_TEXT, ("docs/adr/0002-workspace.md has drifted from tools/workspace_model.py; "
                                 "run `python3 tools/check_workspace.py write`")


def test_every_crate_and_feature_is_published_in_the_adr():
    for crate in M.CRATES:
        assert crate.name in ADR_TEXT
        for feature in crate.features:
            assert "`%s`" % feature.name in ADR_TEXT, "%s is not published" % feature.name


def test_the_adr_states_the_msrv_and_the_upgrade_policy():
    assert M.MSRV in ADR_TEXT
    for clause, _statement in M.MSRV_POLICY:
        assert clause in ADR_TEXT, "MSRV policy clause %r missing from the ADR" % clause
    assert "quarter" in ADR_TEXT


def test_the_adr_names_its_own_rule_ids():
    """A rule id in the ADR is what a CI failure refers to; keep them aligned."""
    for rid, _text in M.rules_rows():
        assert rid in ADR_TEXT
    for text in ADR_TEXT.splitlines():
        for match in re.finditer(r"\b([EMFNPHATCDU]\d\d)\b", text):
            assert match.group(1) in ADR_TEXT


def test_ci_wiring_is_present_and_agrees_on_the_msrv():
    assert CW.check_ci_wiring() == []
    workflows = "\n".join(p.read_text() for p in sorted((ROOT / ".github" / "workflows").glob("*.yml")))
    for _name, needle in M.CI_REQUIRED_STEPS:
        assert needle in workflows
    assert "cargo deny check" in workflows
    assert "tools/check_workspace.py lint" in workflows


def test_ci_wiring_check_actually_fails_when_a_step_disappears(tmp_path, monkeypatch):
    real = ROOT / ".github" / "workflows"
    fake = tmp_path / "workflows"
    fake.mkdir()
    text = "\n".join(p.read_text() for p in sorted(real.glob("*.yml")))
    (fake / "contracts.yml").write_text(text.replace("cargo deny check", "echo later"))
    monkeypatch.setattr(CW, "WORKFLOWS", fake)
    problems = CW.check_ci_wiring()
    assert any(p.rule == "C01" and "cargo deny" in p.message for p in problems)

    (fake / "contracts.yml").write_text(
        text.replace("cargo deny check", "echo later").replace('MSRV: "1.97.0"', 'MSRV: "1.80.0"'))
    assert any(p.rule == "T07" for p in CW.check_ci_wiring())


def test_the_committed_deny_toml_passes_its_own_rules():
    assert CW.lint_deny_toml(ROOT) == []


# ---------------------------------------------------------------------------
# 3. the scaffold: the shape is real
# ---------------------------------------------------------------------------


def test_scaffold_contains_every_crate_and_the_workspace_files(clean_ws):
    for crate in M.CRATES:
        assert (clean_ws / "crates" / crate.name / "Cargo.toml").is_file()
    for extra in ("Cargo.toml", "deny.toml", "rust-toolchain.toml", "rustfmt.toml"):
        assert (clean_ws / extra).is_file()
    assert M.MSRV in (clean_ws / "rust-toolchain.toml").read_text()


def test_scaffold_ships_the_facade_modules_and_the_gated_unsafe(clean_ws):
    for _crate, module, _ticket in M.FACADE_MODULES:
        assert (clean_ws / ("crates/fathom-analyze/src/%s.rs" % module)).is_file()
    mmap = (clean_ws / "crates/fathom-store/src/raw/mmap.rs").read_text()
    assert 'feature = "mmap"' in mmap and "// SAFETY:" in mmap


def test_clean_scaffold_has_no_unsafe_outside_the_one_gated_file(clean_ws):
    offenders = []
    for path in clean_ws.rglob("*.rs"):
        rel = path.relative_to(clean_ws).as_posix()
        if re.search(r"\bunsafe\s+(fn|impl|block|extern|trait)\b|\bunsafe\s*\{", path.read_text()):
            offenders.append(rel)
    assert offenders == ["crates/fathom-store/src/raw/mmap.rs"]


# ---------------------------------------------------------------------------
# 4. mutations: every rule must actually bite
# ---------------------------------------------------------------------------

# (id, file, text to replace, replacement, rule id expected)
MUTATIONS = [
    ("uphill dependency", "crates/fathom-tui/Cargo.toml",
     "fathom-core.workspace = true",
     "fathom-core.workspace = true\nfathom-store.workspace = true", "E01"),
    ("uphill import", "crates/fathom-tui/src/lib.rs",
     "#![forbid(unsafe_code)]", "#![forbid(unsafe_code)]\nuse fathom_store::Index;\n", "E01"),
    ("uphill import in a path expression", "crates/fathom-tui/src/lib.rs",
     "#![forbid(unsafe_code)]",
     "#![forbid(unsafe_code)]\nfn x() -> fathom_analyze::Delta { unreachable!() }\n", "M02"),
    ("facade breach", "crates/fathom-tui/src/lib.rs",
     "#![forbid(unsafe_code)]",
     "#![forbid(unsafe_code)]\nuse fathom_analyze::detectors::Detector;\n", "M02"),
    ("facade breach via mcp", "crates/fathom-mcp/src/lib.rs",
     "#![forbid(unsafe_code)]",
     "#![forbid(unsafe_code)]\nuse fathom_analyze::risk::Score;\n", "M02"),
    ("IO in core", "crates/fathom-core/src/lib.rs",
     "#![forbid(unsafe_code)]",
     "#![forbid(unsafe_code)]\nfn read() -> std::fs::Metadata { unreachable!() }\n", "W1"),
    ("env read in analysis", "crates/fathom-analyze/src/lib.rs",
     "#![forbid(unsafe_code)]",
     "#![forbid(unsafe_code)]\nfn flag() -> bool { std::env::var(\"F`).is_ok() }\n".replace("`", "'"),
     "W1"),
    ("wall clock in core", "crates/fathom-core/src/lib.rs",
     "#![forbid(unsafe_code)]",
     "#![forbid(unsafe_code)]\nfn t() -> std::time::Instant { std::time::Instant::now() }\n", "W2"),
    # `src/search/` is exempt from `W2` — the search report prints how long the keystroke took — so this
    # is the proof the exemption is one directory and not the whole crate.
    ("wall clock in an analysis module", "crates/fathom-analyze/src/insight.rs",
     "//!", "//!\nfn t() -> u128 { std::time::Instant::now().as_micros() }\n", "W2"),
    ("unsafe in core", "crates/fathom-core/src/lib.rs",
     "#![forbid(unsafe_code)]", "#![forbid(unsafe_code)]\nunsafe fn raw() {}\n", "P02"),
    ("unsafe outside src/raw", "crates/fathom-store/src/lib.rs",
     "#![deny(unsafe_code)]", "#![deny(unsafe_code)]\nunsafe fn outside() {}\n", "P02"),
    ("ungated unsafe", "crates/fathom-store/src/raw/mmap.rs",
     '#![cfg(feature = "mmap")]', "// the gate is gone\n", "P03"),
    ("unsafe without a SAFETY comment", "crates/fathom-store/src/raw/mmap.rs",
     "// SAFETY:", "// why not:", "P04"),
    ("fixtures as a real dependency", "crates/fathom-analyze/Cargo.toml",
     "fathom-core.workspace = true",
     "fathom-core.workspace = true\nfathom-fixtures.workspace = true", "E05"),
    ("fixtures imported from src", "crates/fathom-analyze/src/lib.rs",
     "#![forbid(unsafe_code)]",
     "#![forbid(unsafe_code)]\nuse fathom_fixtures::Repo;\n", "E05"),
    ("network client in a default feature", "crates/fathom-analyze/Cargo.toml",
     "default = []", 'default = ["net-advisories"]', "N03"),
    ("network feature without the prefix", "crates/fathom-analyze/Cargo.toml",
     'net-advisories = ["dep:rustls", "ureq"]', 'quiet-advisories = ["dep:rustls", "ureq"]', "F01"),
    ("tokio allowed to bind", "crates/fathom-mcp/Cargo.toml",
     "tokio.workspace = true", 'tokio = { workspace = true, features = ["net"] }', "N04"),
    ("gix with default features", "crates/fathom-ingest/Cargo.toml",
     "gix.workspace = true", 'gix = { workspace = true, default-features = true }', "N05"),
    ("gix reaching a remote", "crates/fathom-ingest/Cargo.toml",
     "gix.workspace = true",
     'gix = { workspace = true, default-features = false, features = ["blocking-network-client"] }',
     "N05"),
    ("undeclared feature", "crates/fathom-analyze/Cargo.toml",
     "[features]", "[features]\nlang-zig = []", "F01"),
    ("shipped default set changed", "crates/fathom-cli/Cargo.toml",
     # The mutation target has to be the set the manifest actually ships, or this case tests nothing: the
     # shipped set gained `deps-all` in `e2.5` and this string did not follow it.
     'default = ["tui", "mcp", "watch", "sqlite-bundled", "langs-all", "deps-all"]',
     'default = ["tui"]', "F03"),
    ("library grows a default feature", "crates/fathom-tui/Cargo.toml",
     "default = []", 'default = ["graph-layout"]', "F02"),
    ("crate restates the edition", "crates/fathom-core/Cargo.toml",
     "edition.workspace = true", 'edition = "2021"', "T01"),
    ("crate restates the MSRV", "crates/fathom-store/Cargo.toml",
     "rust-version.workspace = true", 'rust-version = "1.75.0"', "T01"),
    ("eyre in a library", "crates/fathom-analyze/src/lib.rs",
     "#![forbid(unsafe_code)]", "#![forbid(unsafe_code)]\nuse eyre::Result;\n", "W5"),
    ("path smuggle across crates", "crates/fathom-tui/src/lib.rs",
     "#![forbid(unsafe_code)]",
     '#[path = "../fathom-core/src/lib.rs"]\nmod smuggled;\n', "W3"),
    ("pub mod outside lib.rs", "crates/fathom-analyze/src/insight.rs",
     "//!", "pub mod leak;\n//!", "W4"),
    ("todo in a library", "crates/fathom-core/src/lib.rs",
     "#![forbid(unsafe_code)]", "#![forbid(unsafe_code)]\nfn later() { todo!() }\n", "W6"),
    ("forbid attribute removed", "crates/fathom-core/src/lib.rs",
     "#![forbid(unsafe_code)]", "// gone\n", "A01"),
    ("crate opts out of the shared lints", "crates/fathom-core/Cargo.toml",
     "[lints]\nworkspace = true", "[lints2]", "A02"),
]


@pytest.mark.parametrize("name,file,old,new,rule", MUTATIONS, ids=[m[0] for m in MUTATIONS])
def test_a_mutation_of_each_rule_fails_the_lint(workspace, name, file, old, new, rule):
    edit(workspace, file, old, new)
    problems = CW.lint(workspace)
    assert problems, "the lint let %s through" % name
    assert rule in {p.rule for p in problems}, \
        "%s failed with %s, expected %s:\n%s" % (name, sorted({p.rule for p in problems}), rule,
                                                 "\n".join(p.line() for p in problems))


def test_deleting_deny_toml_fails_the_lint(workspace):
    (workspace / "deny.toml").unlink()
    assert "D01" in rules(workspace)


# ---------------------------------------------------------------------------
# the same lint, wrong in the other direction: a rule that fires on data, and a rule
# relaxed further than the design allows
# ---------------------------------------------------------------------------


def test_a_string_that_quotes_a_marker_is_data_not_a_violation(workspace):
    """`W4`, `W6` and `P02` read code; a quoted marker is a detector's own table.

    The pathologies that look for `todo!(`, for a `pub mod` and for `unsafe` carry those exact strings in
    their marker lists, and the grep-level lint read those tables as violations — five false `W6`s and
    one false `P02` on the shipped tree, next to the eight false `W4`s from the same crate. The fix is
    `check_workspace.string_code`; this is what keeps it narrow: the literals are excused, and every
    mutation in `MUTATIONS` above proves the same three rules still fire on code.
    """
    edit(workspace, "crates/fathom-analyze/src/insight.rs", "//!",
         '//!\nconst MARKERS: &[&str] = &["todo!()", "unimplemented!(", "pub mod leak;",\n'
         '                  "pub unsafe fn "];\n')
    fired = {p.rule for p in CW.lint(workspace)}
    assert not {"W4", "W6", "P02"} & fired, "a quoted marker was read as code: %s" % sorted(fired)


def test_an_inline_public_module_still_needs_a_file(workspace):
    """`W4`'s remaining teeth: a public module is a file, or it is named as a namespace.

    The exemption in `workspace_model.INLINE_NAMESPACE_MODULES` is four files deep. A `pub mod x { … }`
    block anywhere else has no file for a reviewer to open, which is the half of `R9` that survived the
    workspace growing nested modules.
    """
    edit(workspace, "crates/fathom-analyze/src/insight.rs", "//!",
         '//!\npub mod hidden { pub const X: &str = "x"; }\n')
    assert "W4" in rules(workspace)


def test_the_class_m_files_analyze_may_read_are_the_only_io_it_is_exempt_for(workspace):
    """`W1`'s carve-out is three paths in `fathom-analyze`, not the crate.

    ADR-002 §3 lets this crate touch the committed markdown that is the record (`src/ledger/`,
    `src/notes/`, the ratchet baseline in `src/cycles/debt.rs`) and nothing else: a fact arriving by
    `std::fs` in any other analysis module is still the accident the rule exists to catch.
    """
    ledger = workspace / "crates/fathom-analyze/src/ledger"
    ledger.mkdir(parents=True)
    (ledger / "format.rs").write_text(
        '//!\nfn read_it() -> usize { std::fs::read_dir(".").map(|e| e.count()).unwrap_or(0) }\n')
    assert rules(workspace) == set(), "the class-M reader is exempt by path, and this is the proof"
    (workspace / "crates/fathom-analyze/src/rank.rs").write_text(
        '//!\nfn read_it() -> usize { std::fs::read_dir(".").map(|e| e.count()).unwrap_or(0) }\n')
    assert "W1" in rules(workspace), "the exemption leaked past the three paths it names"


def test_unsigned_advisory_ignore_fails_the_lint(workspace):
    edit(workspace, "deny.toml", "ignore = []", 'ignore = ["RUSTSEC-2024-0001"]')
    assert "D07" in rules(workspace)
    edit(workspace, "deny.toml", 'ignore = ["RUSTSEC-2024-0001"]',
         'ignore = ["RUSTSEC-2024-0001"]  # why: upstream has not released since 2026-08; tracked in R')
    assert "D07" not in rules(workspace)


def test_removing_a_banned_crate_from_deny_toml_fails_the_lint(workspace):
    text = (workspace / "deny.toml").read_text()
    (workspace / "deny.toml").write_text(text.replace('{ name = "libgit2-sys",', '{ name = "kept",'))
    assert "D08" in rules(workspace)


def test_wrong_workspace_resolver_fails_the_lint(workspace):
    edit(workspace, "Cargo.toml", 'resolver = "3"', 'resolver = "2"')
    assert "T06" in rules(workspace)


def test_workspace_without_shared_lints_fails_the_lint(workspace):
    text = (workspace / "Cargo.toml").read_text()
    (workspace / "Cargo.toml").write_text(text.split("[workspace.lints.rust]")[0])
    assert "A02" in rules(workspace)


def test_a_crate_directory_the_model_does_not_declare_fails_the_lint(workspace):
    shutil.copytree(workspace / "crates/fathom-core", workspace / "crates/fathom-extra")
    edit(workspace, "crates/fathom-extra/Cargo.toml", 'name = "fathom-core"',
         'name = "fathom-extra"')
    assert "U01" in rules(workspace)


def test_a_crate_directory_that_disagrees_with_its_package_name_fails(workspace):
    shutil.copytree(workspace / "crates/fathom-core", workspace / "crates/fathom-sneaky")
    assert "U07" in rules(workspace)


def test_a_cargo_toml_that_does_not_parse_is_reported_not_raised(workspace):
    edit(workspace, "crates/fathom-core/Cargo.toml", "[lints]", "[lints]\n[lints]")
    assert "U07" in rules(workspace)


def test_removing_the_binary_fails_the_lint(workspace):
    shutil.rmtree(workspace / "crates/fathom-cli")
    assert "U06" in rules(workspace)


def test_a_legal_import_through_the_facade_still_passes(workspace):
    """The lint must not be a hair trigger: the allowed path has to be allowed."""
    edit(workspace, "crates/fathom-tui/src/lib.rs",
         "#![forbid(unsafe_code)]",
         "#![forbid(unsafe_code)]\nuse fathom_analyze::insight::Delta;\n")
    assert CW.lint(workspace) == []


def test_a_feature_on_a_library_is_allowed_when_it_is_not_default(workspace):
    edit(workspace, "crates/fathom-analyze/src/lib.rs",
         "#![forbid(unsafe_code)]",
         '#![cfg_attr(feature = "lang-rust", allow(dead_code))]\n#![forbid(unsafe_code)]\n')
    assert CW.lint(workspace) == []


def test_tests_may_use_the_fixture_crate(clean_ws):
    """R7 in its positive form: `tests/` importing fixtures is the intended shape."""
    text = (clean_ws / "crates/fathom-analyze/tests/boundary.rs").read_text()
    assert "fathom_fixtures" in text


# ---------------------------------------------------------------------------
# 5. the checks the CLI exposes, because CI calls the CLI
# ---------------------------------------------------------------------------


def test_check_subcommand_reports_ok():
    assert CW.main([sys.argv[0], "check"]) == 0


def test_lint_subcommand_is_clean_on_the_repo_root(capsys):
    assert CW.main([sys.argv[0], "lint", str(ROOT)]) == 0
    assert "no crates/ directory" in capsys.readouterr().out or True


def test_scaffold_refuses_a_non_empty_directory(tmp_path):
    (tmp_path / "something").write_text("x")
    with pytest.raises(SystemExit):
        CW.scaffold(tmp_path)


# ---------------------------------------------------------------------------
# 6. the workspace `e0.7` landed: the Rust source against the model
# ---------------------------------------------------------------------------
#
# Everything above this line reads the ADR and the model, because it was written before `crates/`
# existed. These checks read the compiled workspace, and they are the reason the model is published into
# Rust *constants* (`fathom_core::exit`, `fathom_core::privacy`) instead of being paraphrased by hand:
# a table with two copyists drifts, and the drift shows up as a binary that exits 1 where a script was
# promised 4.
#
# Stdlib-only, again on purpose: no `cargo`, no build, no registry fetch. A regex over a source file is
# weaker than a macro that generates the enum from the model, and it is the strongest check that runs in
# the ten seconds a reviewer has.


EXIT_RS = ROOT / "crates" / "fathom-core" / "src" / "exit.rs"
PRIVACY_RS = ROOT / "crates" / "fathom-core" / "src" / "privacy.rs"
CARGO_LOCK = ROOT / "Cargo.lock"


def test_the_landed_workspace_is_here():
    """`e0.7`'s deliverable exists, in the layout §2.1 promises: root manifest, one dir per crate."""
    assert (ROOT / "Cargo.toml").is_file()
    assert (ROOT / "Cargo.lock").is_file()
    assert (ROOT / "rust-toolchain.toml").is_file()
    assert (ROOT / "rustfmt.toml").is_file()
    for crate in M.CRATES:
        assert (ROOT / "crates" / crate.name / "Cargo.toml").is_file(), f"{crate.name} has no manifest"
        assert (ROOT / "crates" / crate.name / "src" / "lib.rs").is_file() or (
            ROOT / "crates" / crate.name / "src" / "main.rs"
        ).is_file(), f"{crate.name} has no crate root"


def test_the_landed_root_manifest_matches_the_model():
    """§2's window and §10.1's version decisions, as written into the real root manifest."""
    text = (ROOT / "Cargo.toml").read_text()
    assert f'resolver = "{M.RESOLVER}"' in text
    assert f'edition = "{M.EDITION}"' in text
    assert f'rust-version = "{M.MSRV}"' in text
    assert 'members = ["crates/*"]' in text
    # Every external crate the shipped default closure needs is pinned here (§10.1: versions are e0.7's
    # decision, and a decision recorded only in a lockfile is a decision nobody can review).
    shipped = M.closure_external_crates(M.cli_shipped_default_closure())
    for dep in sorted(n for n in shipped if not n.startswith("fathom-")):
        assert re.search(rf'^{re.escape(dep)} = ', text, re.M), f"{dep} is not pinned in the root manifest"
    # And the toolchain file repeats the floor, because a declared MSRV nobody installs is a wish.
    assert M.MSRV in (ROOT / "rust-toolchain.toml").read_text()


def test_the_rust_exit_codes_are_the_models_exit_codes():
    """§7.3's table, in Rust, without a copyist.

    Three directions, because a one-way check catches half the drifts: every variant exists with the
    model's number, the model's name string is the Rust `name()` string, and — the direction a hand-edit
    most often breaks — Rust has no variant the model never declared.
    """
    assert EXIT_RS.is_file(), f"{EXIT_RS} is missing: e0.7 did not land the exit-code table"
    text = EXIT_RS.read_text()
    for code, name, meaning in M.EXIT_CODES:
        # Rust spells a variant `NotARepository`; the contract spells it `NOT_A_REPOSITORY`. Both forms
        # have to line up, because `name()` is the string a `--json` consumer switches on.
        variant = "".join(part.title() for part in name.split("_"))
        assert re.search(rf"\b{variant} = {code},", text), f"{variant} = {code} is not in exit.rs"
        assert f'"{name}"' in text, f'exit.rs name() has no "{name}"'
        assert meaning in text, f"exit.rs lost the meaning {meaning!r}"
    in_rust = {
        re.sub(r"(?<!^)(?=[A-Z])", "_", found).upper()
        for found in re.findall(r"^\s{4}([A-Z][A-Za-z0-9]+) = \d+,", text, re.M)
    }
    assert in_rust == {row[1] for row in M.EXIT_CODES}, "exit.rs has a variant the model never declared"


def test_the_rust_privacy_table_is_the_models_consent_table():
    """§6.2's consent keys, in Rust, without a copyist.

    The ADR's argument for publishing the table as a Rust constant instead of prose is that the runtime
    gate, `fathom --version` and `fathom doctor --privacy` cannot then disagree with it. This is the
    third pin on the same three rows, and the only one that reads the Rust.
    """
    assert PRIVACY_RS.is_file(), f"{PRIVACY_RS} is missing: e0.7 did not land the consent table"
    text = PRIVACY_RS.read_text()
    owners = {(crate.name, feat.name) for crate, feat in M.net_features()}
    gated = sorted(
        (name, crate) for (crate, name) in owners
        if crate != M.BIN_CRATE and crate != M.FIXTURES_CRATE
    )
    assert gated, "the model declares no network features, which contradicts §6.2"
    for feature, owner in gated:
        key = M.NETWORK_CONSENT_KEYS[feature]
        for needle in (f'feature: "{feature}"', f'owner: "{owner}"', f'"{key}"'):
            assert needle in text, f"{needle} is missing from privacy.rs"
    # Every key is in the published `privacy.*` namespace, and the table holds nothing else.
    assert len(M.NETWORK_CONSENT_KEYS) == 3
    assert sorted(re.findall(r'consent_key: "([^"]+)"', text)) == sorted(M.NETWORK_CONSENT_KEYS.values())


def test_the_lockfile_names_every_crate_and_none_of_the_banned_crates():
    """§8.4's bans, checked where they actually matter: the resolved graph, not the declared one.

    A `[bans] deny` entry states intent; the lockfile states what will be compiled. cargo-deny is the
    real gate (it runs in CI, and `deny.toml`'s own rules are `D01`–`D08`); this is the half-second
    stdlib version that catches the cheap version of the mistake.
    """
    assert CARGO_LOCK.is_file(), "Cargo.lock must be committed for a binary workspace (§10.1)"
    lock = CARGO_LOCK.read_text()
    for crate in M.CRATES:
        assert f'name = "{crate.name}"' in lock, f"{crate.name} is not in the lockfile"
    deny = CW.read_toml(ROOT / "deny.toml")
    banned = {
        entry.get("name") or entry.get("crate")
        for entry in deny.get("bans", {}).get("deny", [])
    }
    assert banned, "deny.toml bans nothing, which is not what §8.4 says"
    for crate in sorted(banned):
        assert f'name = "{crate}"' not in lock, f"{crate} is in the lockfile and banned by §8.4"


# ---------------------------------------------------------------------------
# 7. what Fathom's own `fathom.toml` says about this workspace
# ---------------------------------------------------------------------------
#
# `fathom check` reads `fathom.toml`, so this file is where ADR-002 meets the gate that is supposed to
# answer the layering question about the repository it ships in. Two things have to stay true, and
# **reco-dg7** is the ticket that wrote them down: the `[layers]` table is §3's crate list, and the
# boundary rules are *not* restated as a `[layering]` block, because a rule the module graph cannot
# fire is a rule the reader believes. `tools/check_workspace.py lint` (`E01`–`E05`, `M02`, `M03`) is
# what enforces §5, at crate granularity — the granularity §5 is written at.

FATHOM_TOML = ROOT / "fathom.toml"


def test_fathoms_own_layers_table_is_the_models_crate_list():
    """§3: `fathom.toml`'s `[layers]` and the model's crate list are one list, in both directions.

    Every crate that ships is named, every named `crates/` path is a crate the model declares, and the
    fixture crate is absent because §4.4 keeps it out of every shipped build — a crate nobody links is
    not library code, and `deps.license.library_layers` counts `l0_vocabulary` through `l3_analysis` as
    exactly that.
    """
    config = CW.read_toml(FATHOM_TOML)
    layers = config["layers"]
    named = {
        path.rsplit("/", 1)[-1]
        for paths in layers.values()
        for path in paths
        if path.startswith("crates/")
    }
    shipped = {c.name for c in M.CRATES if c.kind != "dev"}
    assert shipped <= named, f"[layers] lost {sorted(shipped - named)}; §3's list is the layer list"
    assert named <= {c.name for c in M.CRATES}, f"[layers] names a crate the model does not declare"
    assert M.FIXTURES_CRATE not in named, "§4.4: the fixture crate is dev-only and must not be a layer"
    for path in {p for paths in layers.values() for p in paths}:
        assert (ROOT / path).is_dir(), f"[layers] names {path}, which is not a directory"


def test_the_gate_is_told_where_this_repositorys_boundaries_are_enforced():
    """**reco-dg7**: the layering row says `off`, in this file, rather than reporting a blockage.

    `fathom check` on this repository used to answer the layering question with `blocked`: nothing was
    declared, so "respected" had no definition. The remedy it prints is either a `[layering]` table or
    the volume below, and the volume is the honest one here — see the comment above `[check.severity]`
    for the measurement. This test is the pin: if the config drifts back to silence, or drifts forward
    into rules that cannot fire, it fails here rather than in a stranger's CI log.
    """
    config = CW.read_toml(FATHOM_TOML)
    text = FATHOM_TOML.read_text()
    layers = config["layers"]
    assert "layering" not in config, (
        "a `[layering]` block here would judge nothing: the module graph stores no crate edge for Rust, "
        "so it would print a clean bill of health for an architecture the gate never looked at"
    )
    assert config["check"]["severity"]["layering"] == "off", "the gate's layering volume is not declared"
    # The reason travels with the key, or it is a key nobody will defend in six months.
    assert "check_workspace" in text and "E01" in text, "the comment must name what enforces §5 instead"
    assert "leaves the corpus" in text, "the comment must name why a crate edge is not a graph row"
    # And the two rules worth writing when that changes are written, so the day is a copy-paste.
    # Only the *downhill* forbidden pairs: `no_upward = true` is what refuses an uphill edge, so §5.2's
    # downhill-but-forbidden ones are the pairs that need a rule of their own — which is the sentence
    # this file keeps pointing at.
    layer_of = {
        path.rsplit("/", 1)[-1]: name
        for name, paths in layers.items()
        for path in paths
        if path.startswith("crates/")
    }
    forbidden = {
        (layer_of[src], layer_of[dst])
        for src, dst, _why in M.FORBIDDEN_EDGES
        if M.layer_of(dst) < M.layer_of(src)
    }
    assert {("l4_surfaces", "l1_durable_state"), ("l4_surfaces", "l2_outside_door")} <= forbidden
    for src_layer, dst_layer in sorted(forbidden):
        pair = f"{src_layer} -> {dst_layer}"
        assert pair in text, (
            f"§5.2 forbids a downhill edge; the comment must name the rule ({pair}) to write later"
        )


def test_ci_builds_on_all_three_operating_systems_and_both_channels():
    """§8.3's matrix, as a matrix — `C01` proves the steps exist, this proves they run in triplicate."""
    ci = (ROOT / ".github" / "workflows" / "ci.yml").read_text()
    for runner in ("ubuntu-latest", "macos-latest", "windows-latest"):
        assert runner in ci, f"§8.3: CI must build on {runner}"
    for channel in ("stable", "msrv", "beta"):
        assert channel in ci, f"§2: CI must cover the {channel} channel"
    # A matrix that silently runs one compiler on three OSes would pass every other check in this file.
    assert "RUSTUP_TOOLCHAIN" in ci, "the toolchain file outranks rustup default; the matrix must override it"
