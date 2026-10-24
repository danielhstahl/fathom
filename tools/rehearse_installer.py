#!/usr/bin/env python3
"""Install the release the way a stranger does, from a mirror on localhost (`e7.4`, **reco-3q4**).

The acceptance criterion is a sentence about machines: "*curl installer works on clean ubuntu, macOS and
windows VMs and installs the right artifact*". Three of those four words are impossible for a repository
to supply — a *clean VM* is somebody's cloud bill — but the part that is a *mechanism* can be run here,
repeatedly, and it is the part that breaks silently:

* does the installer `dist` generates resolve this machine to the artefact that was built for it? (The
  failure mode is a 404 on a triple, or worse, an archive that downloads and a binary that will not
  run.)
* does the installed binary report the version and feature set the release claims?
* does the checksum a user is told to verify actually match the bytes on the page?

So this is that rehearsal, and `docs/concepts/release.md` §4 is the prose around it:

    dist build --artifacts=local            # the archive for *this* machine's triple
    dist build --artifacts=global           # the installers, the source tarball, sha256.sum
    mirror target/distrib/ over http on 127.0.0.1
    FATHOM_CLI_DOWNLOAD_URL=http://127.0.0.1:<port> sh installer.sh
    run what landed in $FATHOM_CLI_INSTALL_DIR, and say what it said

`cargo-dist`'s installers take their download URL from the environment for exactly this reason
(`FATHOM_CLI_DOWNLOAD_URL`; the table is generated into the release document from the captured plan).

What it does **not** prove, and says out loud at the end: that the artefact another OS receives works.
`dist` refuses to cross-compile for the platforms it cannot reach, so the archive this installs is the one
*this* machine's triple gets. The three-OS claim stays with §3 step 5 of the runbook, on three machines,
with a ledger row each — the honest version of a claim is one that names who can actually make it.

    python3 tools/rehearse_installer.py --self-test      # the four failures the checks must catch

Exit: 0 when the install worked and the artefact matched; 1 otherwise. `dist` must be on `PATH` (or
`--dist`) unless `--skip-build` says the artefacts are already there: a rehearsal that quietly skips when
its tool is missing is not a rehearsal.
"""
from __future__ import annotations

import argparse
import contextlib
import functools
import hashlib
import http.server
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import threading

ROOT = pathlib.Path(__file__).resolve().parent.parent
DISTRIB = ROOT / "target" / "distrib"
CONFIG = ROOT / "dist-workspace.toml"
VERSION_LINE = re.compile(r"^fathom (\S+)")
#: The `[[dist.extra-artifacts]]` table and everything up to the next table header, which is what
#: `--no-extra-artifacts` temporarily lifts out of the config.
EXTRA_BLOCK = re.compile(r"^\[\[dist\.extra-artifacts\]\][\s\S]*?(?=^\[)", re.M)


@contextlib.contextmanager
def no_extra_artifacts(enabled: bool):
    """Run a block with `[[dist.extra-artifacts]]` lifted out of `dist-workspace.toml`, then put it back.

    `dist` refuses a global build whose listed extra artefacts are missing — correct for a release, where
    every Linux archive exists and so every `.deb` and `.rpm` must — and that is a wall for a rehearsal of
    the *installers* on a machine that has one architecture. So a CI job that has one archive strips the
    table, builds the installers, and puts the file back; the system packages are built and verified by
    the steps around it, from the same archives (`tools/package_release.py`, then `dpkg-deb`/`rpm`).

    The file is restored from the bytes read at entry, in a `finally`, and the restore is verified by
    re-reading: a rehearsal must not be able to leave the tree mutated, so "we meant to put it back" is not
    enough — the assert is the part that makes it true.
    """
    if not enabled:
        yield
        return
    original = CONFIG.read_text()
    stripped = EXTRA_BLOCK.sub("", original)
    if stripped == original:
        raise Failure("no [[dist.extra-artifacts]] in dist-workspace.toml to strip; the flag is a "
                      "no-op here and the flag exists to work around that table")
    CONFIG.write_text(stripped)
    try:
        yield
    finally:
        CONFIG.write_text(original)
        if CONFIG.read_text() != original:
            raise Failure("dist-workspace.toml did not come back byte-identical; the tree is dirty and "
                          "every later step in this job is now untrustworthy")


class Failure(Exception):
    """One thing the rehearsal found. Reported with the verdict, not raised as a traceback."""


def host_triple() -> str:
    """The triple this machine is, from the compiler that would have built for it."""
    try:
        out = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError) as err:
        raise Failure("cannot ask rustc for the host triple (%s); pass --target" % err)
    for line in out.splitlines():
        if line.startswith("host: "):
            return line.split(" ", 1)[1].strip()
    raise Failure("`rustc -vV` did not say what its host is; pass --target")


def dist_binary(args: argparse.Namespace) -> pathlib.Path:
    found = args.dist or os.environ.get("DIST_BIN") or shutil.which("dist")
    if not found:
        raise Failure(
            "`dist` is not on PATH. Install the pinned one: curl --proto '=https' --tlsv1.2 -LsSf "
            "https://github.com/axodotdev/cargo-dist/releases/download/v0.33.0/cargo-dist-installer.sh | sh")
    return pathlib.Path(found)


def run_dist(dist: pathlib.Path, *args: str) -> None:
    result = subprocess.run([str(dist), *args], cwd=ROOT, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise Failure("`dist %s` failed (exit %d):\n%s\n%s"
                      % (" ".join(args), result.returncode, result.stdout.strip(), result.stderr.strip()))


class Mirror(http.server.BaseHTTPRequestHandler):
    """A file server that remembers what was asked for, because that is the actual question."""

    def __init__(self, *args, directory: pathlib.Path, **kwargs):
        self.directory = directory
        super().__init__(*args, **kwargs)

    def do_GET(self) -> None:  # noqa: N802 - the http.server spelling
        path = self.directory / self.path.lstrip("/")
        if path.is_file():
            body = path.read_bytes()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()
        self.server.requests.append(self.path)  # type: ignore[attr-defined]

    def log_message(self, *args) -> None:
        pass  # the server's own logging would interleave with the installer's output


def serve(directory: pathlib.Path) -> tuple[http.server.HTTPServer, threading.Thread]:
    handler = functools.partial(Mirror, directory=directory)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    server.requests = []  # type: ignore[attr-defined]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def rehearse(args: argparse.Namespace) -> list[str]:
    """Run the install; return the notes that make up the ledger row."""
    triple = args.target or host_triple()
    distrib = pathlib.Path(args.distrib)
    archive = distrib / ("fathom-cli-%s.tar.xz" % triple)
    installer = distrib / "fathom-cli-installer.sh"
    notes: list[str] = []

    if args.skip_build:
        notes.append("reused %s (--skip-build)" % distrib)
    else:
        dist = dist_binary(args)
        run_dist(dist, "build", "--artifacts=local", "--target=%s" % triple)
        with no_extra_artifacts(args.no_extra_artifacts):
            run_dist(dist, "build", "--artifacts=global")
        notes.append("built the local archive and the global artefacts with `dist`%s"
                     % (" (system-package stage lifted out for the run: --no-extra-artifacts)"
                        if args.no_extra_artifacts else ""))

    if not installer.is_file():
        raise Failure("no %s; `dist build --artifacts=global` did not leave one" % installer)
    if not archive.is_file():
        raise Failure(
            "no %s. This machine is %s and that triple is not in dist-workspace.toml's targets, so "
            "there is nothing for the installer to fetch — rehearse on a machine whose own triple ships, "
            "or pass --target a triple you have built." % (archive, triple))

    # The checksum the release page offers, checked before anything is trusted: this is the one command a
    # user is told to run, so it gets run here against the bytes the installer will fetch.
    side = pathlib.Path(str(archive) + ".sha256")
    if not side.is_file():
        raise Failure("%s is missing, so nothing can verify the download" % side.name)
    recorded = side.read_text().split()[0]
    if recorded != sha256(archive):
        raise Failure("%s says %s, the archive is %s" % (side.name, recorded, sha256(archive)))
    notes.append("%s matches the archive (%s…)" % (side.name, recorded[:12]))

    with tempfile.TemporaryDirectory(prefix="fathom-rehearsal-") as work:
        stage = pathlib.Path(work)
        mirror = stage / "mirror"
        mirror.mkdir()
        for artefact in distrib.iterdir():
            if artefact.is_file():
                shutil.copyfile(artefact, mirror / artefact.name)
        server, thread = serve(mirror)
        port = server.server_address[1]
        try:
            install_dir = stage / "install"
            env = dict(os.environ,
                       FATHOM_CLI_DOWNLOAD_URL="http://127.0.0.1:%d" % port,
                       FATHOM_CLI_INSTALL_DIR=str(install_dir),
                       FATHOM_CLI_NO_MODIFY_PATH="1")
            result = subprocess.run(["sh", str(installer)], env=env, capture_output=True, text=True,
                                    check=False)
            notes.extend(_installed(server, result, install_dir, triple))
        finally:
            server.shutdown()
            thread.join(timeout=5)
    return notes


def _installed(server, result, install_dir: pathlib.Path, triple: str) -> list[str]:
    """What the installer did, and whether the thing it did was right."""
    notes: list[str] = []
    wanted = "fathom-cli-%s.tar.xz" % triple
    asked = [name.lstrip("/") for name in getattr(server, "requests", [])]
    notes.append("the installer asked for: %s" % (", ".join(asked) or "nothing"))
    if wanted not in asked:
        raise Failure("the installer never fetched %s on a %s machine; it fetched %s"
                      % (wanted, triple, asked or "nothing"))
    if any(name.endswith(".404") or "no such file" in name for name in asked):
        raise Failure("the installer hit a missing artefact: %s" % asked)
    if result.returncode != 0:
        raise Failure("the installer exited %d:\n%s\n%s"
                      % (result.returncode, result.stdout.strip(), result.stderr.strip()))
    binary = install_dir / "bin" / "fathom"
    if not binary.is_file():
        # `dist`'s shell installer honours FATHOM_CLI_INSTALL_DIR as the directory the binaries go in
        # on some paths and as the prefix on others; look for either before declaring failure.
        candidates = sorted(install_dir.glob("**/fathom"))
        if not candidates:
            raise Failure("nothing was installed under %s\n%s" % (install_dir, result.stdout.strip()))
        binary = candidates[0]
    notes.append("installed %s (%s)" % (binary, binary.stat().st_size // 1024))

    check = subprocess.run([str(binary), "--version"], capture_output=True, text=True, check=False)
    if check.returncode != 0:
        raise Failure("the installed binary would not answer `--version`: %s" % check.stderr.strip())
    line = check.stdout.strip()
    notes.append("`fathom --version` says: %s" % line)
    found = VERSION_LINE.match(line)
    if not found:
        raise Failure("the installed binary reported %r, which is not a fathom version line" % line)
    workspace = subprocess.run(["python3", "-c",
                                "import tomllib,pathlib;print(tomllib.loads(pathlib.Path("
                                "'Cargo.toml').read_text())['workspace']['package']['version'])"],
                               cwd=ROOT, capture_output=True, text=True, check=False).stdout.strip()
    if found.group(1) != "v%s" % workspace and found.group(1) != workspace:
        raise Failure("the installed binary is %s but the workspace is v%s — the mirror served a "
                      "different release than the tree says" % (found.group(1), workspace))
    if "features:" not in line:
        raise Failure("`--version` did not state the feature set, so a bug report would be missing it")
    notes.append("the version matches Cargo.toml (v%s) and the feature set is stated" % workspace)

    help_out = subprocess.run([str(binary), "--help"], capture_output=True, text=True, check=False)
    if help_out.returncode != 0:
        raise Failure("the installed binary's `--help` exited %d" % help_out.returncode)
    completions = subprocess.run([str(binary), "completions", "bash"], capture_output=True, text=True,
                                 check=False)
    if completions.returncode != 0 or "fathom" not in completions.stdout:
        raise Failure("the installed binary cannot print its own completions")
    notes.append("`--help` and `completions bash` both work from the installed binary")
    return notes


def self_test(args: argparse.Namespace) -> list[str]:
    """Corrupt the local release page four ways, and require the rehearsal to catch each one.

    A rehearsal that cannot fail is not a test, and the four failures below are the four ways an installer
    story breaks in real life: the digest and the bytes disagree, the artefact for this machine is not on
    the page, the installer fetches something else, and the binary that lands is a different release than
    the one the tree is building. Each case runs the *real* `rehearse()` against a throwaway mirror, so
    what is being tested is the checks, not a mock of them.

    It needs the artefacts to exist (they are what gets corrupted), so it runs after a real rehearsal.
    """
    distrib = pathlib.Path(args.distrib)
    triple = args.target or host_triple()
    archive = distrib / ("fathom-cli-%s.tar.xz" % triple)
    if not archive.is_file() or not (distrib / "fathom-cli-installer.sh").is_file():
        raise Failure("the self-test corrupts real artefacts; run the rehearsal once (no --skip-build) "
                      "so %s has something to corrupt" % distrib)

    cases = [
        ("the .sha256 on the page does not match the archive", _wrong_checksum),
        ("this machine's archive is not on the page", _missing_archive),
        ("the installer fetches something else", _wrong_installer),
        ("the installed binary is a different release", _wrong_version),
    ]
    notes = []
    for name, corrupt in cases:
        with tempfile.TemporaryDirectory(prefix="fathom-selftest-") as work:
            mirror = pathlib.Path(work) / "distrib"
            mirror.mkdir()
            for artefact in distrib.iterdir():
                if artefact.is_file():
                    shutil.copyfile(artefact, mirror / artefact.name)
            corrupt(mirror, triple)
            probe = argparse.Namespace(dist=args.dist, target=triple, distrib=str(mirror),
                                       skip_build=True, no_extra_artifacts=True)
            try:
                rehearse(probe)
            except Failure as caught:
                notes.append("%s -> caught: %s" % (name, str(caught).splitlines()[0]))
            else:
                raise Failure("the case '%s' PASSED the rehearsal. The check that should have caught it "
                              "is decorative, which is worse than having no rehearsal" % name)
    return notes


def _wrong_checksum(mirror: pathlib.Path, triple: str) -> None:
    (mirror / ("fathom-cli-%s.tar.xz.sha256" % triple)).write_text(
        "%064d  fathom-cli-%s.tar.xz\n" % (0xdeadbeef, triple))


def _missing_archive(mirror: pathlib.Path, triple: str) -> None:
    (mirror / ("fathom-cli-%s.tar.xz" % triple)).unlink()
    (mirror / ("fathom-cli-%s.tar.xz.sha256" % triple)).unlink()


def _wrong_installer(mirror: pathlib.Path, triple: str) -> None:
    """An installer that installs *something* — the wrong something, with a clean exit code."""
    (mirror / "fathom-cli-installer.sh").write_text(
        "#!/bin/sh\nfetch() { :; }\necho \"installed, surely\"\nexit 0\n")


def _wrong_version(mirror: pathlib.Path, triple: str) -> None:
    """Rebuild the archive with a `fathom` that reports a version the workspace never cut."""
    import tarfile

    archive = mirror / ("fathom-cli-%s.tar.xz" % triple)
    stub = ("#!/bin/sh\n[ \"$1\" = \"--version\" ] && echo \"fathom 9.9.9 (self-test)\" && exit 0\n"
            "exit 0\n").encode()
    staged = archive.parent / "repack"
    staged.mkdir()
    with tarfile.open(archive) as source:
        members = source.getmembers()
        # The archive's one top-level directory, whichever member describes it first.
        top = members[0].name.split("/")[0] if members else ""
        for member in members:
            # Everything but the binary is copied out as-is — directories included, which is the difference
            # between a repack and a tree where `fathom-cli-x` is a file. The sidecar is recomputed after,
            # so the *only* thing lying to the rehearsal is the version the binary prints.
            if member.isfile() and member.name.rsplit("/", 1)[-1] == "fathom":
                target = staged / member.name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(stub)
                os.chmod(target, 0o755)
            else:
                source.extract(member, staged)
    with tarfile.open(archive, "w:xz") as out:
        out.add(staged / top if top else staged, arcname=top or ".")
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (mirror / (archive.name + ".sha256")).write_text("%s  %s\n" % (digest, archive.name))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="rehearse_installer.py", description=__doc__.splitlines()[0])
    parser.add_argument("--dist", help="path to the `dist` binary (default: PATH or $DIST_BIN)")
    parser.add_argument("--target", help="the triple to rehearse (default: this machine's)")
    parser.add_argument("--distrib", default=str(DISTRIB), help="where the artefacts are (default target/distrib)")
    parser.add_argument("--skip-build", action="store_true",
                        help="do not run `dist build`; use what is already in --distrib")
    parser.add_argument("--no-extra-artifacts", action="store_true",
                        help="lift [[dist.extra-artifacts]] out of dist-workspace.toml for the build "
                             "(see `no_extra_artifacts`); CI uses it to rehearse the installers on a "
                             "one-architecture runner")
    parser.add_argument("--self-test", action="store_true",
                        help="corrupt a copy of the release page four ways and require each to be caught")
    args = parser.parse_args(argv)
    try:
        if args.self_test:
            notes = self_test(args)
        else:
            notes = rehearse(args)
    except Failure as failure:
        print("  R01   %s" % failure)
        print("\ninstall rehearsal FAILED")
        return 1
    print("install rehearsal — one row of docs/concepts/release.md §5's shape:")
    for note in notes:
        print("  note  %s" % note)
    if args.self_test:
        print("\ninstall rehearsal OK: all four ways an installer breaks were caught")
        return 0
    print("\ninstall rehearsal OK: the curl installer found this machine's artefact, checked it, and "
          "installed a binary that knows its own version")
    print("not proven here: the artefacts another OS receives. That is §3 step 5, on that OS.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
