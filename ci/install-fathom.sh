#!/bin/sh
# Put a `fathom` on PATH for a CI job (`m2`, **reco-by2**).
#
# This is the script `docs/recipes/gate-a-pipeline.md` has been telling people to run since `e6.6`
# shipped `fathom check`, and it did not exist. That is the whole reason it exists now: a documented
# command line that resolves to nothing is worse than no documentation, and the 1.0 gate's second
# criterion — the gate running inside *someone else's* pipeline — needs one install path several
# repositories can share rather than six hand-copied lists of steps.
#
# Three ways to get a binary, tried in this order in `auto` mode:
#
# 1. **already installed** — an image or an earlier step already put one on PATH and it answers
#    `--version` with the version you asked for. Nothing to do, and saying so is the point: a gate that
#    reinstalls itself in every step cannot be cached.
# 2. **the release installer** — the `cargo-dist` installer for a published tag, the one-liner from
#    `docs/concepts/release.md`. It is still behind a placeholder remote (ADR-002 §10.3), so when the tag
#    is not there the script says exactly that, in the docs' own words, and falls through.
# 3. **build from source** — a clone, or the checkout named by `--src`, built with the *headless CI*
#    feature set `.github/workflows/ci.yml` builds and `fathom check` needs:
#    `sqlite-bundled,langs-all,deps-all`. No `tui`, no `mcp`: a gate must not depend on a terminal or on a
#    protocol server, and this script's own check below is what keeps that true instead of merely intended.
#
# The verification is not decorative. A binary built `--no-default-features --features minimal` installs
# happily, prints a version, and then reports four of the six checks as *blocked* for want of a store: a
# green pipeline that checked nothing. So a binary without a store backend is refused unless you say you
# meant it (`--allow-degraded`), and the refusal names every missing feature rather than the first.
#
# Usage:
#   ci/install-fathom.sh [--version X.Y.Z] [--ref REF] [--mode auto|release|source] [--src PATH]
#                        [--prefix DIR] [--features LIST] [--allow-degraded] [--offline]
#                        [--check PATH] [--print-path] [--help]
#
# Environment defaults: FATHOM_VERSION, FATHOM_REF, FATHOM_MODE, FATHOM_SRC, FATHOM_PREFIX, FATHOM_FEATURES,
# FATHOM_DIST_REPO (the owner/repo the release installer is fetched from).
#
# `--check PATH` answers one narrower question and does nothing else: is *that* binary usable as a CI
# gate? It is the seam the contract test in `tests/test_ci_gate_contract.py` pokes, because the refusal
# below has to be watched failing on a store-less binary before its silence can be trusted.
#
# Exit codes are this script's, not the CLI's contract (that is `docs/reference/cli.md`, and `fathom
# check` is the verb that must be able to exit 7): 0 installed or already present, 1 the binary failed its
# own check, 2 bad usage, 3 no route produced a binary.
set -eu

PROG=${0##*/}
VERSION=${FATHOM_VERSION:-}
REF=${FATHOM_REF:-}
MODE=${FATHOM_MODE:-auto}
SRC=${FATHOM_SRC:-}
PREFIX=${FATHOM_PREFIX:-$PWD/.fathom-ci/bin}
FEATURES=${FATHOM_FEATURES:-sqlite-bundled,langs-all,deps-all}
DIST_REPO=${FATHOM_DIST_REPO:-danielstahl/fathom}
DEGRADED=0
CHECK=
OFFLINE=0
PRINT_PATH=0

say() { printf '%s: %s\n' "$PROG" "$1" >&2; }

usage() {
    cat >&2 <<'USAGE'
usage: ci/install-fathom.sh [--version X.Y.Z] [--ref REF] [--mode auto|release|source]
                            [--src PATH] [--prefix DIR] [--features LIST] [--allow-degraded]
                            [--offline] [--check PATH] [--print-path] [--help]
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION=${2:?--version needs a value}; shift 2 ;;
        --ref) REF=${2:?--ref needs a value}; shift 2 ;;
        --mode) MODE=${2:?--mode needs a value}; shift 2 ;;
        --src) SRC=${2:?--src needs a value}; shift 2 ;;
        --prefix) PREFIX=${2:?--prefix needs a value}; shift 2 ;;
        --features) FEATURES=${2:?--features needs a value}; shift 2 ;;
        --allow-degraded) DEGRADED=1; shift ;;
        --offline) OFFLINE=1; shift ;;
        --print-path) PRINT_PATH=1; shift ;;
        --check) CHECK=${2:?--check needs a path}; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) say "unknown argument: $1"; usage; exit 2 ;;
    esac
done

case "$MODE" in
    auto|release|source) ;;
    *) say "--mode must be auto, release or source"; usage; exit 2 ;;
esac

# The features a CI gate needs, in the words the binary itself prints, so the check below can name every
# one that is missing instead of the first (`fathom --version` prints the compiled-in set, `e7.4`).
REQUIRED_FEATURES="sqlite-bundled langs-all deps-all"

# Print the version line of a binary that exists, answers, and can run all six checks; fail otherwise.
check_binary() {
    candidate=$1
    if [ ! -x "$candidate" ]; then
        return 1
    fi
    line=$("$candidate" --version 2>/dev/null) || return 1
    case "$line" in
        fathom\ *) ;;
        *) return 1 ;;
    esac
    if [ "$DEGRADED" -eq 1 ]; then
        printf '%s\n' "$line"
        return 0
    fi
    missing=
    for want in $REQUIRED_FEATURES; do
        case " $line " in
            *" $want "*) ;;
            *) missing="$missing $want" ;;
        esac
    done
    if [ -n "$missing" ]; then
        say "refusing a binary built without:$missing"
        say "  four of the six checks would report themselves blocked; the run would be green and empty"
        say "  (this binary says: $line — pass --allow-degraded if a store-less build is what you want)"
        return 1
    fi
    printf '%s\n' "$line"
}

# `--check` is the whole script minus the installing: verify and report, then stop.
if [ -n "$CHECK" ]; then
    if line=$(check_binary "$CHECK"); then
        printf '%s: usable as a CI gate: %s\n' "$PROG" "$line"
        exit 0
    fi
    printf '%s: not usable as a CI gate: %s\n' "$PROG" "$CHECK" >&2
    exit 1
fi

install_from_release() {
    if [ "$OFFLINE" -eq 1 ]; then
        say "--offline: not fetching the release installer"
        return 3
    fi
    if [ -z "$VERSION" ]; then
        say "no version to install from a release (pass --version)"
        return 3
    fi
    url="https://github.com/$DIST_REPO/releases/download/v$VERSION/fathom-cli-installer.sh"
    say "trying the release installer for v$VERSION"
    tmp=$(mktemp -t fathom-installer.XXXXXX)
    if ! curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$tmp"; then
        rm -f "$tmp"
        say "$url is not there"
        say "  expected until the public remote exists: ADR-002 §10.3 keeps package.repository"
        say "  placeholder and cargo-dist bakes the installer URL out of it (docs/concepts/release.md)"
        return 3
    fi
    if FATHOM_VERSION="$VERSION" sh "$tmp"; then
        rm -f "$tmp"
        return 0
    fi
    rm -f "$tmp"
    say "the release installer ran and failed"
    return 3
}

install_from_source() {
    if [ -z "$SRC" ]; then
        if [ "$OFFLINE" -eq 1 ]; then
            say "--offline: not cloning (pass --src to build a checkout you already have)"
            return 3
        fi
        SRC=$(mktemp -d -t fathom-src.XXXXXX)
        # `--ref` wins over `--version` here: the version says which *release* to install, and when the
        # release is not published yet the operator who knows which commit they want says so with --ref.
        want=${REF:-${VERSION:+v$VERSION}}
        if [ -n "$want" ]; then
            say "cloning $DIST_REPO at $want"
            if ! git clone --quiet --depth 1 --branch "$want" "https://github.com/$DIST_REPO" "$SRC"; then
                say "the clone failed; there is nothing to build"
                return 3
            fi
        else
            say "cloning $DIST_REPO at its default branch"
            if ! git clone --quiet --depth 1 "https://github.com/$DIST_REPO" "$SRC"; then
                say "the clone failed; there is nothing to build"
                return 3
            fi
        fi
    fi
    if [ ! -d "$SRC/.git" ] && [ ! -f "$SRC/Cargo.toml" ]; then
        say "$SRC is neither a clone nor a cargo workspace"
        return 3
    fi
    say "building the headless CI binary from $SRC (features: $FEATURES)"
    # `--locked`, because a CI install that silently resolves to newer dependencies is a gate whose
    # behaviour changed without a commit saying so.
    ( cd "$SRC" && cargo build --locked --release -p fathom-cli --no-default-features --features "$FEATURES" )
    built="$SRC/target/release/fathom"
    if [ ! -f "$built" ]; then
        say "the build produced no $built"
        return 3
    fi
    mkdir -p "$PREFIX"
    cp "$built" "$PREFIX/fathom"
    chmod 755 "$PREFIX/fathom"
}

report_and_exit() {
    where=$1
    if [ "$PRINT_PATH" -eq 1 ]; then
        printf '%s\n' "$where"
    fi
    # A GitHub runner gets the directory on PATH for the *later* steps of the same job. Other runners
    # have their own way of saying it and are not assumed to have this one.
    if [ -n "${GITHUB_PATH:-}" ]; then
        echo "$where" >> "$GITHUB_PATH"
        say "appended $where to \$GITHUB_PATH"
    fi
    exit 0
}

# 1 · already on PATH, and the right version?
if [ "$MODE" != "source" ] && command -v fathom >/dev/null 2>&1; then
    existing=$(command -v fathom)
    if line=$(check_binary "$existing"); then
        if [ -z "$VERSION" ] || printf '%s' "$line" | grep -q -- "$VERSION"; then
            say "using the fathom already on PATH: $line ($existing)"
            report_and_exit "$(dirname "$existing")"
        fi
        say "the fathom on PATH is not the version asked for: $line"
    else
        say "the fathom on PATH cannot serve as a CI gate (reason above)"
    fi
fi

# 2 · the release, then the source build.
if [ "$MODE" = "auto" ] || [ "$MODE" = "release" ]; then
    if install_from_release; then
        hash -r 2>/dev/null || true
        if command -v fathom >/dev/null 2>&1; then
            if line=$(check_binary "$(command -v fathom)"); then
                say "installed from the release: $line"
                report_and_exit "$(dirname "$(command -v fathom)")"
            fi
        fi
        say "the release installer completed, but no usable fathom answered --version"
        if [ "$MODE" = "release" ]; then
            exit 1
        fi
    elif [ "$MODE" = "release" ]; then
        exit 3
    fi
fi

if [ "$MODE" = "auto" ] || [ "$MODE" = "source" ]; then
    if ! install_from_source; then
        say "no permitted route produced a binary"
        exit 3
    fi
    line=$(check_binary "$PREFIX/fathom") || exit 1
    say "installed: $line at $PREFIX/fathom"
    report_and_exit "$PREFIX"
fi

say "mode $MODE produced nothing"
exit 3
