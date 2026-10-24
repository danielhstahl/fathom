"""The privacy posture (`e6.8`, reco-h7p), checked from the repo side.

The Rust tests own what the binary prints (`crates/fathom-cli/tests/cli.rs`
`doctor_privacy_says_who_may_open_a_gate_and_who_only_pretended_to`); this file owns the parts that only
exist in the repository: the checker, the tripwire source, the generated SBOM and licence report, the two
workflows that run them, and the page that has to keep saying the same things.

The offline run is executed here as well, when a binary is built: a gate that CI alone runs is a gate
nobody sees locally, and it costs about a second. `$FATHOM_BIN`, else `target/release/fathom`, else
`target/debug/fathom`; with none built the two binary-dependent tests skip, as everywhere in this
directory — a check that needs an install is a check that gets skipped.

Stdlib-only, like every other file here.
"""
import json
import os
import pathlib
import subprocess
import sys

import pytest

import tools.check_privacy as P
import tools.workspace_model as M

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECKER = ROOT / "tools" / "check_privacy.py"
NETPROBE = ROOT / "tools" / "privacy_netprobe.c"
GEN_SBOM = ROOT / "tools" / "gen_sbom.py"
PAGE = ROOT / "docs" / "concepts" / "privacy.md"
PRIVACY_WORKFLOW = ROOT / ".github" / "workflows" / "privacy.yml"
SBOM_WORKFLOW = ROOT / ".github" / "workflows" / "sbom.yml"
CORE_PRIVACY_RS = ROOT / "crates" / "fathom-core" / "src" / "privacy.rs"
DIAGNOSE_RS = ROOT / "crates" / "fathom-cli" / "src" / "diagnose.rs"


def _binary():
    env = os.environ.get("FATHOM_BIN")
    for candidate in ([pathlib.Path(env)] if env else []) + [
            ROOT / "target/release/fathom", ROOT / "target/debug/fathom"]:
        if candidate.is_file():
            return candidate
    return None


def _run(argv, timeout=300):
    return subprocess.run([sys.executable, *argv], cwd=ROOT, capture_output=True, text=True,
                          timeout=timeout)


# ---------------------------------------------------------------------------
# the checker, and the trap it depends on
# ---------------------------------------------------------------------------

def test_the_static_half_is_clean_and_says_which_rules_ran():
    result = _run([str(CHECKER), "check", "--verbose"])
    assert result.returncode == 0, result.stdout + result.stderr
    for rule in ("V01", "V02", "V03", "V04", "V05", "V06", "V07"):
        assert rule in result.stdout, "rule %s reported nothing — a silent pass is not a check" % rule


def test_the_egress_needles_still_trap_a_constructor_and_only_excuse_hanging_up():
    """`V04`'s one exemption is `Shutdown`, and it is exactly that wide.

    The rule greps names, so its whole risk is a narrowing that quietly lets a real constructor past.
    This is the assertion that keeps the exception in `check_privacy.EGRESS_NOT` the size of the claim
    made for it: a Unix-domain socket closing its own connection is not egress, and everything that
    *opens* a path to another host still trips, including on a line that also names `Shutdown`.
    """
    assert P.egress_hit("        let _ = stream.shutdown(std::net::Shutdown::Both);") is None
    for line in ("use std::net::TcpStream;",
                 "let s = TcpStream::connect(host)?;",
                 "let l = std::net::TcpListener::bind(addr)?;",
                 "let u = UdpSocket::bind(\"0.0.0.0:53\")?;",
                 # The shape that would be the quiet way round: open a socket and close it in one line.
                 "TcpStream::connect(a)?.shutdown(std::net::Shutdown::Both)?;"):
        assert P.egress_hit(line) is not None, "%s is egress and the rule no longer says so" % line


def test_the_egress_trap_actually_traps_here():
    """`selftest` refuses to report a pass it did not measure; at least one trap must work here.

    This is the test that keeps the rest honest. On a runner with no user namespaces the preload tripwire
    carries the proof; on a machine with neither, the offline run exits 2 rather than claiming success,
    and this test says so out loud instead of going green.
    """
    result = _run([str(CHECKER), "selftest"])
    if result.returncode == 2:
        pytest.skip("no egress trap available on this platform; `offline` would refuse to run too")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "traps" in result.stdout, result.stdout


@pytest.mark.skipif(_binary() is None, reason="no fathom binary built; set FATHOM_BIN")
def test_the_shipped_binary_behaves_identically_with_no_egress():
    """The acceptance criterion, run end to end: no egress, identical bytes, zero attempts."""
    binary = _binary()
    report = pathlib.Path(os.environ.get("TMPDIR", "/tmp")) / "fathom-privacy-report.json"
    result = _run([str(CHECKER), "offline", "--binary", str(binary), "--json", str(report)])
    assert result.returncode == 0, result.stdout + result.stderr
    payload = json.loads(report.read_text())
    assert payload["verdict"] == "pass", payload
    assert payload["trap"]["verified"] is True
    assert payload["egress_attempts"] == 0 or payload["egress_attempts"] is None
    commands = {tuple(row["argv"]): row for row in payload["commands"]}
    assert all(row["identical"] for row in payload["commands"]), payload["commands"]
    # The claim is about the whole pipeline, not only about reads: the cold index is in the set.
    assert ("index",) in commands
    assert ("doctor", "--privacy") in commands


# ---------------------------------------------------------------------------
# the table, the page and the binary agree
# ---------------------------------------------------------------------------

def consent_rows():
    """The published consent table, parsed straight out of the Rust source."""
    text = CORE_PRIVACY_RS.read_text()
    rows = {}
    for match in __import__("re").finditer(r"NetFeature\s*\{([^}]*)\}", text, __import__("re").S):
        fields = dict(__import__("re").findall(r'(\w+)\s*:\s*"([^"]*)"', match.group(1)))
        if "feature" in fields:
            rows[fields["feature"]] = fields
    return rows


def test_the_consent_table_has_one_row_per_net_feature():
    rows = consent_rows()
    assert set(rows) == set(M.NETWORK_CONSENT_KEYS), "the Rust table and the model are two tables"
    for feature, fields in rows.items():
        assert fields["consent_key"] == M.NETWORK_CONSENT_KEYS[feature]


def test_the_page_names_every_gate_and_the_check_that_backs_it():
    page = PAGE.read_text()
    for feature, fields in consent_rows().items():
        assert "`%s`" % feature in page
        assert "`%s`" % fields["consent_key"] in page
    for claim in ("V01", "V07", "doctor --privacy", "check_privacy.py", "privacy_netprobe.c",
                  "no endpoint", "user-scoped"):
        assert claim in page, "%s is missing from the privacy page" % claim


def test_doctor_privacy_prints_both_gates_and_the_local_only_claim():
    """The prose the page quotes is the prose the binary prints; a drift here breaks both tests and docs."""
    source = DIAGNOSE_RS.read_text()
    for column in ("COMPILED", "CONSENT", "STATE", "CONSENT KEY", "SET IN"):
        assert column in source, "doctor --privacy no longer has a %s column" % column
    for line in ("consent is user-scoped", "local records:", "usage metrics:",
                 "outbound features compiled in"):
        assert line in source, "doctor --privacy no longer prints %r" % line


@pytest.mark.skipif(_binary() is None, reason="no fathom binary built; set FATHOM_BIN")
def test_the_binary_reports_the_posture_the_page_describes(tmp_path):
    binary = _binary()
    # A repository that tries to consent on its owner's behalf, and a user file that declines one key.
    home = tmp_path / "home"
    (home / ".config" / "fathom").mkdir(parents=True)
    (home / ".config" / "fathom" / "config.toml").write_text("[privacy]\nllm = false\n")
    (tmp_path / "fathom.toml").write_text("[privacy]\nadvisories = true\n")
    out = subprocess.run([str(binary), "doctor", "--privacy"], cwd=tmp_path,
                         env={**os.environ, "HOME": str(home),
                              "XDG_CONFIG_HOME": str(home / ".config")},
                         capture_output=True, text=True)
    text = out.stdout
    assert "ignored: this repository's fathom.toml sets privacy.advisories" in text, text
    assert "consent is user-scoped" in text
    assert "usage metrics: nothing leaves this machine" in text
    row = next(line for line in text.splitlines() if line.startswith("net-llm"))
    assert " user " in row, "an explicit refusal is attributed to the user layer: %s" % row


# ---------------------------------------------------------------------------
# the SBOM and the licence report
# ---------------------------------------------------------------------------

def test_sbom_and_licence_report_are_current():
    result = _run([str(GEN_SBOM), "check"])
    assert result.returncode == 0, result.stdout + result.stderr


def sbom_document():
    paths = sorted((ROOT / "sbom").glob("*.cdx.json"))
    assert len(paths) == 1, "exactly one SBOM per release skeleton, found %s" % paths
    return json.loads(paths[0].read_text())


def test_sbom_covers_the_default_build_and_nothing_network_capable():
    document = sbom_document()
    assert document["bomFormat"] == "CycloneDX"
    assert document["serialNumber"].startswith("urn:uuid:")
    components = document["components"]
    assert components, "an empty SBOM describes nothing"
    names = {component["name"] for component in components}
    closure = M.closure_external_crates(M.cli_shipped_default_closure())
    assert closure <= names, "shipped crates missing from the SBOM: %s" % sorted(closure - names)
    assert not (names & set(M.NETWORK_CRATES)), "the SBOM would be reporting a network-capable binary"
    for component in components:
        assert component["purl"].startswith("pkg:cargo/")
        assert component["licenses"], "%s has no licence entry" % component["name"]
        assert component["version"], "%s is unpinned" % component["name"]
    first_party = {name for name in names if name.startswith("fathom-")}
    assert first_party, "the project's own crates belong in its SBOM"
    for component in components:
        if component["name"] in closure:
            assert component["hashes"], "%s has no digest: an inventory without a pin is a rumour" \
                % component["name"]


def test_licence_report_says_what_is_allowed_and_what_is_not():
    reports = [path for path in sorted((ROOT / "sbom").glob("*.md")) if "licen" in path.name]
    assert len(reports) == 1, reports
    text = reports[0].read_text()
    for verdict in ("`allowed`", "`violation`", "`unknown`", "`workspace`"):
        assert verdict in text, "the verdict table lost a row: %s" % verdict
    assert "0 component(s)" in text.split("## Every component")[0], \
        "the summary must state that nothing is unlicensed or disallowed"
    assert "no — the default binary links no HTTP client" in text


# ---------------------------------------------------------------------------
# the wiring: a check nobody runs is a rumour
# ---------------------------------------------------------------------------

def test_the_privacy_workflow_runs_the_offline_proof():
    text = PRIVACY_WORKFLOW.read_text()
    assert "check_privacy.py selftest" in text, "the trap is not proven before it is trusted"
    assert "check_privacy.py offline" in text
    assert "cargo build --locked -p fathom-cli" in text, "the default binary, not a feature build"
    assert "upload-artifact" in text, "the report goes nowhere"


def test_the_sbom_workflow_publishes_per_release():
    text = SBOM_WORKFLOW.read_text()
    assert "release:" in text and "published" in text, "not triggered by a release"
    assert "gen_sbom.py check" in text
    assert "gh release upload" in text, "nothing is attached to the release"


def test_the_tripwire_traps_every_way_out_and_only_the_way_out():
    """The C shim's scope, asserted from its source: egress refused, loopback allowed."""
    text = NETPROBE.read_text()
    for symbol in ('dlsym(RTLD_NEXT, "connect")', 'dlsym(RTLD_NEXT, "bind")',
                   'dlsym(RTLD_NEXT, "sendto")', 'dlsym(RTLD_NEXT, "getaddrinfo")'):
        assert symbol in text, "the tripwire no longer intercepts %s" % symbol
    assert "EACCES" in text and "EAI_FAIL" in text
    assert "IN6_IS_ADDR_LOOPBACK" in text and "0x7f000000u" in text, \
        "loopback has to stay allowed or the tool cannot run at all"
