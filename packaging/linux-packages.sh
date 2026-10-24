#!/bin/sh
# The command `dist` runs in its global phase to build the Linux system packages (`e7.4`, **reco-3q4**).
#
# `cargo-dist` has no deb or rpm target, so the packages are `[[dist.extra-artifacts]]` in
# `dist-workspace.toml` and this script is the `build` line. It is a shell script rather than a Python
# entry point for one reason: the first thing a global runner is missing is `rpmbuild`, and saying so in
# the release log in plain words is worth more than a Python traceback about a missing binary. The actual
# work is in `tools/package_release.py`; everything here is provisioning and one decision.
#
# By the time `dist` gets here, every per-target archive has been built and downloaded back into
# `target/distrib/`, which is what lets the packages be *repacked from the release* rather than recompiled
# by a toolchain that happens to be on this runner (§1.3 of docs/concepts/release.md).
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# `dist` installs `[dist.dependencies]` for the *per-target* jobs (that is where `musl-tools` comes from);
# nothing provisions the global job, so the packager's one external dependency is provisioned here. Every
# package manager that might own this image is tried, in the order their images are the common ones; a
# runner with none of them is a runner where the message below is the whole answer.
if ! command -v rpmbuild >/dev/null 2>&1; then
    echo "rpmbuild is missing; installing it (the global runner image does not carry it)"
    if command -v apt-get >/dev/null 2>&1; then
        sudo apt-get update -qq || true
        sudo apt-get install -y --no-install-recommends rpmbuild || true
    elif command -v dnf >/dev/null 2>&1; then
        sudo dnf install -y rpm-build || true
    elif command -v yum >/dev/null 2>&1; then
        sudo yum install -y rpm-build || true
    elif command -v zypper >/dev/null 2>&1; then
        sudo zypper --non-interactive install rpm-build || true
    fi
    if ! command -v rpmbuild >/dev/null 2>&1; then
        echo "note: rpm not installed; the .rpm will not be read back"
    fi
fi

# The one decision, and it is the release's, not a rehearsal's. `dist` lists the `.rpm` files as artefacts
# it will upload, so a build that silently produced none would publish a release with two of four system
# packages and no signal. `--skip-rpm` exists for a rehearsal on a machine that cannot get the tooling, and
# saying so out loud here is what keeps that flag from ever being what a release runs with.
skip_rpm=""
if ! command -v rpmbuild >/dev/null 2>&1; then
    if [ "${FATHOM_PACKAGES_ALLOW_NO_RPM:-}" = "1" ]; then
        echo "rpmbuild still missing; continuing with --skip-rpm because FATHOM_PACKAGES_ALLOW_NO_RPM=1"
        skip_rpm="--skip-rpm"
    else
        echo "error: rpmbuild is not installed and the release needs it to write the .rpm files."
        echo "       On Debian/Ubuntu: apt-get install -y --no-install-recommends rpmbuild"
        echo "       To rehearse without them: FATHOM_PACKAGES_ALLOW_NO_RPM=1 packaging/linux-packages.sh (the release will not)"
        exit 1
    fi
fi

exec python3 tools/package_release.py $skip_rpm "$@"
