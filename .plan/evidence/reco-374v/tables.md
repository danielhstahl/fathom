# The census in full (`reco-374v.1`)

Window 2026-10-16 → 2026-11-15 · 172 runs · 83 red · 70 green · 19 skipped · 3366.9 measured runner-minutes · 0 red re-run to green on the same commit.

## Per workflow

| workflow | trigger | jobs | steps | runs | red | red % | runner-min | mean min | p95 min | max min |
|---|---|---|---|---|---|---|---|---|---|---|
| `ci.yml` | pull_request; push:master | 12 | 88 | 19 | 19 | 100% | 2365 | 48.2 | 362.8 | 363.3 |
| `release.yml` | pull_request; dispatch | 5 | 31 | 18 | 18 | 100% | 452 | 9.6 | 14.7 | 17.9 |
| `fuzz.yml` | pull_request (paths: fathom-analyze, fathom-ingest, fuzz); schedule 17 3 * * *; dispatch | 2 | 19 | 19 | 7 | 37% | 403 | 9.8 | 12.8 | 13.0 |
| `perf.yml` | pull_request; push:master | 1 | 10 | 19 | 17 | 89% | 98 | 5.2 | 6.9 | 6.9 |
| `contracts.yml` | pull_request; push:master | 1 | 42 | 19 | 19 | 100% | 24 | 1.3 | 3.4 | 3.6 |
| `dogfood.yml` | schedule 23 7 * * 2 (weekly); dispatch | 1 | 7 | 3 | 3 | 100% | 11 | 3.9 | 4.0 | 4.0 |
| `privacy.yml` | pull_request; push:master | 2 | 10 | 19 | 0 | 0% | 10 | 1.3 | 2.0 | 2.1 |
| `sbom.yml` | pull_request; push:master; release:published | 2 | 5 | 19 | 0 | 0% | 2 | 0.3 | 0.4 | 0.5 |
| `docs.yml` | pull_request; push:master | 2 | 10 | 18 | 0 | 0% | 2 | 0.3 | 0.3 | 0.3 |
| `complexity.yml` | pull_request (24-path filter); schedule 17 3 * * *; dispatch | 1 | 7 | 0 | 0 | — | — | 0.0 | 0.0 | 0.0 |
| `fathom-check.yml` | workflow_call only (the reusable gate) | 1 | 9 | 0 | 0 | — | — | 0.0 | 0.0 | 0.0 |
| `publish.yml` | workflow_run (ci completed); dispatch | 1 | 5 | 19 | 0 | 0% | — | 0.0 | 0.2 | 0.2 |
| `self-review.yml` | release:published; dispatch | 1 | 12 | 0 | 0 | — | — | 0.0 | 0.0 | 0.0 |

## Per job

| workflow · job | observed runs | red | mean min | p95 min | max min | total min |
|---|---|---|---|---|---|---|
| `ci.yml` · build+test (ubuntu-latest · stable) | 13 | 13 | 61.2 | 360.2 | 360.2 | 796.0 |
| `ci.yml` · beta (allowed to fail) | 19 | 9 | 40.9 | 360.3 | 360.3 | 777.9 |
| `ci.yml` · check-gate (the CI gate, on its own repository | 19 | 0 | 8.7 | 17.1 | 17.7 | 165.5 |
| `release.yml` · build-local-artifacts (x86_64-apple-darwin) | 17 | 0 | 9.3 | 14.4 | 17.4 | 158.4 |
| `fuzz.yml` · smoke (paths) | 15 | 3 | 9.4 | 12.8 | 13.0 | 141.2 |
| `fuzz.yml` · smoke (hooks) | 15 | 3 | 9.2 | 12.4 | 12.4 | 138.2 |
| `fuzz.yml` · smoke (lockfile) | 15 | 6 | 9.1 | 13.0 | 13.5 | 137.2 |
| `ci.yml` · packaging (deb, rpm, and the installer that fe | 19 | 9 | 5.5 | 10.4 | 10.6 | 104.7 |
| `perf.yml` · perf budgets (small class) | 19 | 17 | 5.2 | 6.8 | 6.9 | 98.2 |
| `release.yml` · build-local-artifacts (x86_64-pc-windows-msvc) | 17 | 10 | 5.4 | 7.7 | 8.7 | 91.1 |
| `ci.yml` · structural extractor core and tail reproduce ( | 19 | 2 | 4.3 | 5.3 | 6.3 | 82.1 |
| `ci.yml` · build+test (windows-latest · stable) | 12 | 12 | 6.8 | 11.7 | 11.8 | 81.6 |
| `ci.yml` · build+test (macos-latest · stable) | 12 | 12 | 6.7 | 10.9 | 15.0 | 80.8 |
| `ci.yml` · Guardrail 3 calibration reproduces (three real | 19 | 4 | 3.4 | 4.1 | 4.3 | 64.8 |
| `release.yml` · build-local-artifacts (x86_64-unknown-linux-mu | 17 | 0 | 3.7 | 4.4 | 6.0 | 62.3 |
| `release.yml` · build-local-artifacts (x86_64-unknown-linux-gn | 17 | 0 | 3.3 | 4.0 | 5.0 | 55.6 |
| `release.yml` · build-local-artifacts (aarch64-unknown-linux-g | 17 | 0 | 2.8 | 4.1 | 4.5 | 47.7 |
| `ci.yml` · spike s1 reproduces (six grammars) | 19 | 2 | 2.1 | 2.4 | 3.3 | 39.8 |
| `ci.yml` · build+test (windows-latest · msrv) | 12 | 12 | 3.1 | 3.6 | 3.6 | 37.7 |
| `ci.yml` · out-of-tree-plugin (the example plugin builds  | 19 | 0 | 1.6 | 2.8 | 3.1 | 30.9 |
| `ci.yml` · build+test (macos-latest · msrv) | 12 | 12 | 2.6 | 3.1 | 3.2 | 30.8 |
| `release.yml` · build-local-artifacts (aarch64-apple-darwin) | 17 | 10 | 1.8 | 5.0 | 5.8 | 29.9 |
| `ci.yml` · build+test (ubuntu-latest · msrv) | 13 | 10 | 1.9 | 2.5 | 3.1 | 25.0 |
| `contracts.yml` · docs + workspace contracts | 18 | 18 | 1.3 | 3.4 | 3.6 | 24.2 |
| `ci.yml` · clippy (strict) | 19 | 6 | 1.1 | 1.8 | 1.8 | 20.5 |
| `ci.yml` · cargo-deny (licences, advisories, bans, source | 19 | 0 | 0.8 | 0.9 | 1.0 | 15.4 |
| `dogfood.yml` · fathom check over the corpus | 3 | 3 | 3.7 | 3.9 | 3.9 | 11.2 |
| `privacy.yml` · default binary, egress refused | 9 | 0 | 1.0 | 1.2 | 1.2 | 8.9 |
| `release.yml` · plan | 18 | 1 | 0.4 | 0.5 | 0.5 | 7.1 |
| `ci.yml` · release skeleton matches dist-workspace.toml | 19 | 2 | 0.3 | 0.4 | 0.4 | 5.8 |
| `ci.yml` · fmt (rustfmt) | 19 | 0 | 0.3 | 0.5 | 1.9 | 5.7 |
| `sbom.yml` · the committed artefacts match the lockfile | 8 | 0 | 0.3 | 0.3 | 0.3 | 2.3 |
| `docs.yml` · build the html site | 9 | 0 | 0.2 | 0.3 | 0.3 | 1.9 |
| `privacy.yml` · posture (closure, consent table, sources, docs | 9 | 0 | 0.1 | 0.1 | 0.1 | 1.0 |
| `ci.yml` · build+test ( ·) | 6 | 0 | -0.0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts () | 1 | 0 | -0.0 | -0.0 | -0.0 | 0.0 |
| `docs.yml` · publish to github pages | 9 | 0 | -0.0 | 0.0 | 0.0 | -0.1 |
| `release.yml` · build-global-artifacts | 18 | 0 | -0.0 | 0.0 | 0.0 | -0.1 |
| `sbom.yml` · attach them to the release | 8 | 0 | -0.0 | 0.0 | 0.0 | -0.1 |
| `release.yml` · announce | 18 | 0 | -0.0 | 0.0 | 0.0 | -0.2 |
| `release.yml` · host | 18 | 0 | -0.0 | 0.0 | 0.0 | -0.2 |
| `fuzz.yml` · 30 minutes () | 15 | 0 | -0.9 | 0.0 | 0.0 | -13.8 |

## Per step (anything that ran)

| workflow · job · step | executed | failed | mean min | p95 min | total min |
|---|---|---|---|---|---|
| `ci.yml` · build+test (ubuntu-latest · st · test (everything, all features) | 13 | 7 | 58.7 | 358.7 | 762.9 |
| `ci.yml` · beta (allowed to fail) · test (everything, on beta) | 13 | 7 | 58.3 | 358.6 | 758.1 |
| `release.yml` · build-local-artifacts (x86_64- · Build artifacts | 17 | 0 | 8.1 | 12.9 | 137.6 |
| `ci.yml` · check-gate (the CI gate, on it · index | 13 | 0 | 9.8 | 13.3 | 127.7 |
| `release.yml` · build-local-artifacts (x86_64- · Build artifacts | 17 | 10 | 4.6 | 6.9 | 79.0 |
| `fuzz.yml` · smoke (lockfile) · One minute per target | 15 | 3 | 4.6 | 6.6 | 69.2 |
| `perf.yml` · perf budgets (small class) · build the release binary the budgets are cha | 19 | 0 | 3.6 | 4.4 | 68.2 |
| `ci.yml` · Guardrail 3 calibration reprod · G3, the miss rate and every gate reproduce f | 19 | 4 | 3.0 | 3.8 | 57.8 |
| `fuzz.yml` · smoke (paths) · Build the targets | 15 | 3 | 3.9 | 5.4 | 57.8 |
| `fuzz.yml` · smoke (hooks) · Build the targets | 15 | 3 | 3.8 | 5.2 | 56.6 |
| `fuzz.yml` · smoke (lockfile) · Build the targets | 15 | 3 | 3.7 | 5.4 | 55.3 |
| `ci.yml` · structural extractor core and  · e3.1: the committed accuracy-and-mechanism r | 19 | 0 | 2.8 | 4.0 | 53.0 |
| `release.yml` · build-local-artifacts (x86_64- · Build artifacts | 17 | 0 | 3.0 | 3.6 | 50.3 |
| `release.yml` · build-local-artifacts (x86_64- · Build artifacts | 17 | 0 | 2.8 | 3.6 | 48.4 |
| `ci.yml` · packaging (deb, rpm, and the i · cargo build (release, the profile the artefa | 13 | 0 | 3.5 | 4.3 | 45.5 |
| `ci.yml` · packaging (deb, rpm, and the i · dist build (this runner's archive) | 13 | 2 | 3.3 | 4.6 | 42.9 |
| `ci.yml` · build+test (windows-latest · s · test (everything, all features) | 12 | 6 | 3.5 | 8.2 | 41.4 |
| `ci.yml` · build+test (macos-latest · sta · test (everything, all features) | 11 | 11 | 3.7 | 4.5 | 40.5 |
| `release.yml` · build-local-artifacts (aarch64 · Build artifacts | 17 | 0 | 2.3 | 2.7 | 39.7 |
| `ci.yml` · build+test (windows-latest · s · build (everything, all features) | 12 | 6 | 3.0 | 3.4 | 35.8 |
| `release.yml` · build-local-artifacts (aarch64 · Build artifacts | 7 | 0 | 3.7 | 4.6 | 25.8 |
| `ci.yml` · build+test (macos-latest · sta · build (everything, all features) | 11 | 0 | 2.1 | 2.6 | 22.9 |
| `ci.yml` · build+test (windows-latest · m · build the headless `minimal` binary | 12 | 0 | 1.8 | 2.0 | 21.3 |
| `ci.yml` · structural extractor core and  · e3.1b: the harness builds with every grammar | 19 | 2 | 1.0 | 1.2 | 19.1 |
| `ci.yml` · out-of-tree-plugin (the exampl · and against a delta document the freshly bui | 13 | 0 | 1.5 | 1.7 | 19.0 |
| `ci.yml` · spike s1 reproduces (six gramm · spike s1: the harness itself builds with eve | 19 | 2 | 1.0 | 1.3 | 19.0 |
| `ci.yml` · build+test (ubuntu-latest · st · build (everything, all features) | 13 | 0 | 1.4 | 1.7 | 18.5 |
| `ci.yml` · check-gate (the CI gate, on it · the gate, timed against the 60s budget | 13 | 0 | 1.3 | 1.8 | 17.5 |
| `ci.yml` · build+test (macos-latest · msr · build the headless `minimal` binary | 12 | 0 | 1.5 | 1.7 | 17.4 |
| `ci.yml` · spike s1 reproduces (six gramm · spike s1: the committed accuracy report repr | 19 | 0 | 0.9 | 1.6 | 17.4 |
| `ci.yml` · beta (allowed to fail) · build (everything, on beta) | 13 | 0 | 1.2 | 1.6 | 16.1 |
| `release.yml` · build-local-artifacts (x86_64- · Run swatinem/rust-cache@v2 | 17 | 0 | 0.8 | 1.0 | 13.7 |
| `contracts.yml` · docs + workspace contracts · ADR-002 / ADR-004 / ADR-005 / e0.6 / e0.8 /  | 18 | 6 | 0.7 | 2.5 | 12.7 |
| `perf.yml` · perf budgets (small class) · the gate's contract suite (including: a deli | 19 | 10 | 0.7 | 1.1 | 12.6 |
| `perf.yml` · perf budgets (small class) · the budgets, the baseline, and the trend (lo | 19 | 17 | 0.6 | 1.1 | 12.1 |
| `ci.yml` · build+test (ubuntu-latest · ms · build the headless `minimal` binary | 13 | 0 | 0.9 | 1.1 | 11.9 |
| `dogfood.yml` · fathom check over the corpus · install the gate with the installer outsider | 3 | 3 | 3.6 | 3.8 | 10.9 |
| `ci.yml` · clippy (strict) · clippy libs and bins unwrap_used and expect_ | 19 | 4 | 0.5 | 1.0 | 10.2 |
| `ci.yml` · build+test (windows-latest · m · test the headless configuration | 12 | 12 | 0.8 | 1.0 | 10.0 |
| `ci.yml` · build+test (macos-latest · msr · test the headless configuration | 12 | 12 | 0.7 | 0.9 | 8.8 |
| `ci.yml` · check-gate (the CI gate, on it · the JSON document is the contract it claims  | 13 | 0 | 0.7 | 0.9 | 8.7 |
| `ci.yml` · cargo-deny (licences, advisori · Run EmbarkStudios/cargo-deny-action@v2 | 19 | 0 | 0.4 | 0.5 | 8.5 |
| `ci.yml` · check-gate (the CI gate, on it · build the headless binary the gate ships as | 13 | 0 | 0.6 | 1.0 | 8.1 |
| `ci.yml` · packaging (deb, rpm, and the i · build the system packages from the archive d | 13 | 2 | 0.6 | 0.8 | 8.1 |
| `ci.yml` · build+test (ubuntu-latest · ms · test the headless configuration | 13 | 10 | 0.6 | 0.7 | 7.4 |
| `ci.yml` · clippy (strict) · clippy all targets tests say for themselves  | 19 | 2 | 0.4 | 0.6 | 7.2 |
| `privacy.yml` · default binary, egress refused · the binary users get (default features) | 9 | 0 | 0.7 | 0.9 | 6.5 |
| `release.yml` · build-local-artifacts (x86_64- · Run swatinem/rust-cache@v2 | 17 | 0 | 0.3 | 0.5 | 5.6 |
| `ci.yml` · out-of-tree-plugin (the exampl · build and test the plugin in its own workspa | 13 | 0 | 0.4 | 0.6 | 5.4 |
| `ci.yml` · structural extractor core and  · e3.1: the byte contract and the corpus prope | 19 | 0 | 0.3 | 0.4 | 5.4 |
| `fuzz.yml` · smoke (lockfile) · Install cargo-fuzz | 15 | 0 | 0.3 | 0.4 | 4.7 |
| `fuzz.yml` · smoke (paths) · Install cargo-fuzz | 15 | 0 | 0.3 | 0.4 | 4.7 |
| `fuzz.yml` · smoke (hooks) · Install cargo-fuzz | 15 | 0 | 0.3 | 0.3 | 4.6 |
| `release.yml` · build-local-artifacts (x86_64- · Install dependencies | 17 | 0 | 0.3 | 0.3 | 4.3 |
| `ci.yml` · cargo-deny (licences, advisori · Build EmbarkStudios/cargo-deny-action@v2 | 19 | 0 | 0.2 | 0.3 | 4.1 |
| `release.yml` · build-local-artifacts (x86_64- · Run swatinem/rust-cache@v2 | 17 | 0 | 0.2 | 0.3 | 4.1 |
| `fuzz.yml` · smoke (lockfile) · Run rui314/setup-mold@v1 | 15 | 0 | 0.3 | 1.2 | 4.0 |
| `release.yml` · build-local-artifacts (x86_64- · Run swatinem/rust-cache@v2 | 17 | 0 | 0.2 | 0.3 | 4.0 |
| `release.yml` · plan · Run dist plan --output-format=json > plan-di | 18 | 1 | 0.2 | 0.3 | 3.9 |
| `ci.yml` · out-of-tree-plugin (the exampl · and lint it under its own lint set | 13 | 0 | 0.3 | 0.5 | 3.7 |
| `ci.yml` · build+test (windows-latest · m · Run Swatinem/rust-cache@v2 | 12 | 0 | 0.3 | 0.3 | 3.5 |
| `ci.yml` · Guardrail 3 calibration reprod · fetch the pinned corpus | 19 | 0 | 0.2 | 0.2 | 3.4 |
| `ci.yml` · release skeleton matches dist- · dist generate --check (the workflow and the  | 17 | 0 | 0.2 | 0.2 | 3.3 |
| `perf.yml` · perf budgets (small class) · Run Swatinem/rust-cache@v2 | 19 | 0 | 0.2 | 0.2 | 3.0 |
| `fuzz.yml` · smoke (paths) · Install nightly (libFuzzer needs -Zsanitizer | 15 | 0 | 0.2 | 0.2 | 2.8 |
| `release.yml` · build-local-artifacts (aarch64 · Run swatinem/rust-cache@v2 | 17 | 0 | 0.2 | 0.2 | 2.8 |
| `fuzz.yml` · smoke (hooks) · Install nightly (libFuzzer needs -Zsanitizer | 15 | 0 | 0.2 | 0.2 | 2.6 |
| `fuzz.yml` · smoke (lockfile) · Install nightly (libFuzzer needs -Zsanitizer | 15 | 0 | 0.2 | 0.2 | 2.6 |
| `release.yml` · build-local-artifacts (x86_64- · Run actions/checkout@v6 | 17 | 0 | 0.1 | 0.2 | 2.4 |
| `ci.yml` · beta (allowed to fail) · Run Swatinem/rust-cache@v2 | 13 | 0 | 0.2 | 0.3 | 2.3 |
| `ci.yml` · build+test (macos-latest · msr · Run Swatinem/rust-cache@v2 | 12 | 0 | 0.2 | 0.3 | 2.3 |
| `ci.yml` · packaging (deb, rpm, and the i · Run Swatinem/rust-cache@v2 | 13 | 0 | 0.2 | 0.3 | 2.3 |
| `ci.yml` · build+test (ubuntu-latest · ms · Run Swatinem/rust-cache@v2 | 13 | 0 | 0.2 | 0.3 | 2.2 |
| `release.yml` · build-local-artifacts (aarch64 · Install dist | 17 | 0 | 0.1 | 0.4 | 2.2 |
| `ci.yml` · build+test (ubuntu-latest · st · test the headless configuration | 13 | 1 | 0.1 | 0.5 | 1.9 |
| `release.yml` · build-local-artifacts (aarch64 · Run swatinem/rust-cache@v2 | 7 | 0 | 0.3 | 0.4 | 1.8 |
| `release.yml` · build-local-artifacts (x86_64- · Run actions/checkout@v6 | 17 | 0 | 0.1 | 0.1 | 1.8 |
| `ci.yml` · build+test (windows-latest · s · Run Swatinem/rust-cache@v2 | 12 | 0 | 0.1 | 0.2 | 1.6 |
| `sbom.yml` · the committed artefacts match  · regenerate and diff | 8 | 0 | 0.2 | 0.2 | 1.6 |
| `ci.yml` · Guardrail 3 calibration reprod · Run Swatinem/rust-cache@v2 | 19 | 0 | 0.1 | 0.1 | 1.4 |
| `ci.yml` · build+test (windows-latest · m · Run actions/checkout@v4 | 12 | 0 | 0.1 | 0.1 | 1.4 |
| `ci.yml` · spike s1 reproduces (six gramm · Run Swatinem/rust-cache@v2 | 19 | 0 | 0.1 | 0.1 | 1.4 |
| `ci.yml` · structural extractor core and  · Run Swatinem/rust-cache@v2 | 19 | 0 | 0.1 | 0.1 | 1.4 |
| `contracts.yml` · docs + workspace contracts · spike s4: rename and copy detection re-measu | 18 | 2 | 0.1 | 0.1 | 1.4 |
| `ci.yml` · build+test (windows-latest · s · Run actions/checkout@v4 | 12 | 0 | 0.1 | 0.1 | 1.3 |
| `ci.yml` · cargo-deny (licences, advisori · deny.toml's own rules (D01-D08) | 19 | 0 | 0.1 | 0.1 | 1.2 |
| `ci.yml` · out-of-tree-plugin (the exampl · Run Swatinem/rust-cache@v2 | 26 | 0 | 0.0 | 0.1 | 1.2 |
| `contracts.yml` · docs + workspace contracts · e2.4 rename pairing: the shipped detector re | 18 | 3 | 0.1 | 0.1 | 1.2 |
| `ci.yml` · clippy (strict) · Run Swatinem/rust-cache@v2 | 19 | 0 | 0.1 | 0.1 | 1.1 |
| `ci.yml` · fmt (rustfmt) · cargo fmt --all --check | 19 | 0 | 0.1 | 0.1 | 1.1 |
| `release.yml` · build-local-artifacts (x86_64- · Set up job | 17 | 0 | 0.1 | 0.1 | 1.1 |
| `ci.yml` · check-gate (the CI gate, on it · Run Swatinem/rust-cache@v2 | 13 | 0 | 0.1 | 0.1 | 1.0 |
| `ci.yml` · check-gate (the CI gate, on it · and the gate's own test suite | 13 | 0 | 0.1 | 0.1 | 1.0 |
| `privacy.yml` · default binary, egress refused · Run Swatinem/rust-cache@v2 | 9 | 0 | 0.1 | 0.2 | 1.0 |
| `ci.yml` · fmt (rustfmt) · Run Swatinem/rust-cache@v2 | 19 | 0 | 0.0 | 0.1 | 0.9 |
| `release.yml` · build-local-artifacts (x86_64- · Install dist | 17 | 0 | 0.1 | 0.1 | 0.9 |
| `contracts.yml` · docs + workspace contracts · e7.5 docs site: no orphan page, no dangling  | 18 | 0 | 0.0 | 0.1 | 0.8 |
| `release.yml` · build-local-artifacts (x86_64- · Upload artifacts | 17 | 0 | 0.0 | 0.1 | 0.8 |
| `release.yml` · build-local-artifacts (x86_64- · Post Run swatinem/rust-cache@v2 | 17 | 0 | 0.0 | 0.1 | 0.7 |
| `release.yml` · plan · Cache dist | 18 | 0 | 0.0 | 0.1 | 0.7 |
| `ci.yml` · build+test (macos-latest · msr · Run actions/checkout@v4 | 12 | 0 | 0.0 | 0.1 | 0.6 |
| `ci.yml` · build+test (macos-latest · sta · Run actions/checkout@v4 | 11 | 0 | 0.1 | 0.1 | 0.6 |
| `ci.yml` · structural extractor core and  · Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.1 | 0.6 |
| `contracts.yml` · docs + workspace contracts · Run actions/checkout@v4 | 18 | 0 | 0.0 | 0.1 | 0.6 |
| `release.yml` · build-local-artifacts (aarch64 · Upload artifacts | 17 | 0 | 0.0 | 0.1 | 0.6 |
| `release.yml` · build-local-artifacts (x86_64- · Install dist | 17 | 0 | 0.0 | 0.1 | 0.6 |
| `release.yml` · build-local-artifacts (x86_64- · Post Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.1 | 0.6 |
| `release.yml` · build-local-artifacts (x86_64- · Upload artifacts | 17 | 0 | 0.0 | 0.1 | 0.6 |
| `release.yml` · build-local-artifacts (x86_64- · Upload artifacts | 17 | 0 | 0.0 | 0.1 | 0.6 |
| `release.yml` · plan · Run actions/checkout@v6 | 18 | 0 | 0.0 | 0.1 | 0.6 |
| `ci.yml` · Guardrail 3 calibration reprod · Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.5 |
| `ci.yml` · build+test (windows-latest · s · Post Run actions/checkout@v4 | 12 | 0 | 0.0 | 0.1 | 0.5 |
| `ci.yml` · clippy (strict) · Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.5 |
| `ci.yml` · spike s1 reproduces (six gramm · Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.5 |
| `docs.yml` · build the html site · check the site (links, anchors, escaping, no | 9 | 0 | 0.1 | 0.1 | 0.5 |
| `perf.yml` · perf budgets (small class) · Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.5 |
| `release.yml` · build-local-artifacts (x86_64- · Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.0 | 0.5 |
| `release.yml` · build-local-artifacts (x86_64- · Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.1 | 0.5 |
| `ci.yml` · Guardrail 3 calibration reprod · Set up job | 19 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · beta (allowed to fail) · Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · build+test (macos-latest · msr · Set up job | 12 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · build+test (macos-latest · sta · Complete job | 11 | 0 | 0.0 | 0.1 | 0.4 |
| `ci.yml` · build+test (ubuntu-latest · st · Run Swatinem/rust-cache@v2 | 13 | 0 | 0.0 | 0.1 | 0.4 |
| `ci.yml` · build+test (ubuntu-latest · st · Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · build+test (windows-latest · m · Post Run actions/checkout@v4 | 12 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · build+test (windows-latest · m · install the pinned toolchain | 12 | 0 | 0.0 | 0.1 | 0.4 |
| `ci.yml` · cargo-deny (licences, advisori · Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · clippy (strict) · Set up job | 19 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · fmt (rustfmt) · Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · fmt (rustfmt) · Set up job | 19 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · out-of-tree-plugin (the exampl · Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · packaging (deb, rpm, and the i · Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · release skeleton matches dist- · Run actions/checkout@v4 | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · release skeleton matches dist- · install cargo-dist (pinned to dist-workspace | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `fuzz.yml` · smoke (hooks) · Run actions/checkout@v4 | 15 | 0 | 0.0 | 0.0 | 0.4 |
| `fuzz.yml` · smoke (lockfile) · Run actions/checkout@v4 | 15 | 0 | 0.0 | 0.0 | 0.4 |
| `fuzz.yml` · smoke (paths) · Run actions/checkout@v4 | 15 | 0 | 0.0 | 0.1 | 0.4 |
| `perf.yml` · perf budgets (small class) · Set up job | 19 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (aarch64 · Run actions/checkout@v6 | 7 | 0 | 0.1 | 0.1 | 0.4 |
| `release.yml` · build-local-artifacts (aarch64 · Set up job | 7 | 0 | 0.1 | 0.1 | 0.4 |
| `release.yml` · build-local-artifacts (aarch64 · Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (aarch64 · Set up job | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · Fetch local artifacts | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · Set up job | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · enable windows longpaths | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · Install dist | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · Set up job | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · Fetch local artifacts | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · Install dist | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · build-local-artifacts (x86_64- · Set up job | 17 | 0 | 0.0 | 0.0 | 0.4 |
| `release.yml` · plan · Install dist | 18 | 0 | 0.0 | 0.0 | 0.4 |
| `ci.yml` · build+test (macos-latest · msr · Complete job | 12 | 0 | 0.0 | 0.1 | 0.3 |
| `ci.yml` · build+test (macos-latest · sta · Run Swatinem/rust-cache@v2 | 11 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · build+test (macos-latest · sta · Set up job | 11 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · build+test (ubuntu-latest · ms · Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · cargo-deny (licences, advisori · Set up job | 19 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · check-gate (the CI gate, on it · Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · fmt (rustfmt) · Post Run Swatinem/rust-cache@v2 | 19 | 0 | 0.0 | 0.1 | 0.3 |
| `ci.yml` · packaging (deb, rpm, and the i · Set up job | 13 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · packaging (deb, rpm, and the i · install cargo-dist (pinned to dist-workspace | 13 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · release skeleton matches dist- · Set up job | 19 | 2 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · spike s1 reproduces (six gramm · Set up job | 19 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · structural extractor core and  · Set up job | 19 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · structural extractor core and  · e3.1b: the tail contract — go granularity, t | 19 | 0 | 0.0 | 0.0 | 0.3 |
| `docs.yml` · build the html site · write it | 9 | 0 | 0.0 | 0.1 | 0.3 |
| `fuzz.yml` · smoke (lockfile) · Set up job | 15 | 0 | 0.0 | 0.0 | 0.3 |
| `fuzz.yml` · smoke (paths) · Set up job | 15 | 0 | 0.0 | 0.0 | 0.3 |
| `privacy.yml` · default binary, egress refused · Run actions/checkout@v4 | 9 | 0 | 0.0 | 0.1 | 0.3 |
| `privacy.yml` · default binary, egress refused · run it with no network available, and compar | 9 | 0 | 0.0 | 0.0 | 0.3 |
| `privacy.yml` · posture (closure, consent tabl · Run actions/checkout@v4 | 9 | 0 | 0.0 | 0.0 | 0.3 |
| `release.yml` · build-local-artifacts (aarch64 · Upload artifacts | 7 | 0 | 0.0 | 0.1 | 0.3 |
| `release.yml` · build-local-artifacts (aarch64 · Fetch local artifacts | 17 | 0 | 0.0 | 0.0 | 0.3 |
| `release.yml` · build-local-artifacts (x86_64- · Post Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.0 | 0.3 |
| `release.yml` · build-local-artifacts (x86_64- · Fetch local artifacts | 17 | 0 | 0.0 | 0.0 | 0.3 |
| `release.yml` · plan · Set up job | 18 | 0 | 0.0 | 0.0 | 0.3 |
| `ci.yml` · beta (allowed to fail) · Set up job | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (macos-latest · msr · Post Run actions/checkout@v4 | 12 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (macos-latest · msr · install the pinned toolchain | 12 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (macos-latest · sta · Post Run actions/checkout@v4 | 11 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (ubuntu-latest · ms · Set up job | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (ubuntu-latest · ms · install the pinned toolchain | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (ubuntu-latest · st · Set up job | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (ubuntu-latest · st · install the pinned toolchain | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (windows-latest · m · Set up job | 12 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (windows-latest · s · Set up job | 12 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · build+test (windows-latest · s · install the pinned toolchain | 12 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · check-gate (the CI gate, on it · Post Run Swatinem/rust-cache@v2 | 13 | 0 | 0.0 | 0.1 | 0.2 |
| `ci.yml` · check-gate (the CI gate, on it · Run actions/upload-artifact@v4 | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · check-gate (the CI gate, on it · Set up job | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · out-of-tree-plugin (the exampl · Post Run Swatinem/rust-cache@v2 | 26 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · out-of-tree-plugin (the exampl · Set up job | 13 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · spike s1 reproduces (six gramm · Post Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · structural extractor core and  · Post Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.2 |
| `contracts.yml` · docs + workspace contracts · Set up job | 18 | 0 | 0.0 | 0.0 | 0.2 |
| `docs.yml` · build the html site · Run actions/checkout@v4 | 9 | 0 | 0.0 | 0.0 | 0.2 |
| `docs.yml` · build the html site · keep a preview for the pull request | 9 | 0 | 0.0 | 0.0 | 0.2 |
| `fuzz.yml` · smoke (hooks) · Run rui314/setup-mold@v1 | 15 | 0 | 0.0 | 0.0 | 0.2 |
| `fuzz.yml` · smoke (hooks) · Set up job | 15 | 0 | 0.0 | 0.0 | 0.2 |
| `fuzz.yml` · smoke (paths) · Run rui314/setup-mold@v1 | 15 | 0 | 0.0 | 0.0 | 0.2 |
| `perf.yml` · perf budgets (small class) · publish the trend as a job artefact | 19 | 0 | 0.0 | 0.0 | 0.2 |
| `privacy.yml` · default binary, egress refused · Set up job | 9 | 0 | 0.0 | 0.0 | 0.2 |
| `privacy.yml` · posture (closure, consent tabl · the static rules V01-V07 | 9 | 0 | 0.0 | 0.0 | 0.2 |
| `release.yml` · build-local-artifacts (aarch64 · Install dist | 7 | 0 | 0.0 | 0.0 | 0.2 |
| `release.yml` · build-local-artifacts (aarch64 · Post Run swatinem/rust-cache@v2 | 7 | 0 | 0.0 | 0.1 | 0.2 |
| `release.yml` · build-local-artifacts (x86_64- · Fetch local artifacts | 17 | 0 | 0.0 | 0.0 | 0.2 |
| `release.yml` · build-local-artifacts (x86_64- · Post Run swatinem/rust-cache@v2 | 17 | 0 | 0.0 | 0.0 | 0.2 |
| `sbom.yml` · the committed artefacts match  · Run actions/checkout@v4 | 8 | 0 | 0.0 | 0.0 | 0.2 |
| `ci.yml` · Guardrail 3 calibration reprod · Post Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · beta (allowed to fail) · Post Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · build+test (macos-latest · sta · install the pinned toolchain | 11 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · build+test (ubuntu-latest · ms · Post Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · build+test (ubuntu-latest · st · Post Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · build+test (ubuntu-latest · st · e7.7 beta loop: one channel, one vocabulary, | 13 | 3 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · build+test (windows-latest · m · toolchain | 12 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · build+test (windows-latest · s · toolchain | 12 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · cargo-deny (licences, advisori · Post Run actions/setup-python@v5 | 19 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · check-gate (the CI gate, on it · Post Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · clippy (strict) · Post Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · fmt (rustfmt) · Post Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · out-of-tree-plugin (the exampl · Post Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · out-of-tree-plugin (the exampl · run it the way a reviewer does — a delta doc | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · packaging (deb, rpm, and the i · Post Run actions/checkout@v4 | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · packaging (deb, rpm, and the i · Run dtolnay/rust-toolchain@stable | 13 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · packaging (deb, rpm, and the i · install the .deb the way a user would, and r | 13 | 5 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · release skeleton matches dist- · Post Run actions/checkout@v4 | 17 | 0 | 0.0 | 0.0 | 0.1 |
| `contracts.yml` · docs + workspace contracts · e5.9 demo miniature: the shipped pack is the | 18 | 0 | 0.0 | 0.0 | 0.1 |
| `contracts.yml` · docs + workspace contracts · glossary, plan references and link consisten | 18 | 0 | 0.0 | 0.0 | 0.1 |
| `contracts.yml` · docs + workspace contracts · m2 CI gate: the installer, the workflows and | 18 | 3 | 0.0 | 0.0 | 0.1 |
| `docs.yml` · build the html site · Set up job | 9 | 0 | 0.0 | 0.0 | 0.1 |
| `docs.yml` · build the html site · the docs builder's self-test (no build, mill | 9 | 0 | 0.0 | 0.0 | 0.1 |
| `dogfood.yml` · fathom check over the corpus · Run actions/checkout@v4 | 3 | 0 | 0.0 | 0.1 | 0.1 |
| `fuzz.yml` · smoke (lockfile) · Post Run actions/checkout@v4 | 15 | 0 | 0.0 | 0.0 | 0.1 |
| `fuzz.yml` · smoke (paths) · Post Run actions/checkout@v4 | 15 | 0 | 0.0 | 0.0 | 0.1 |
| `perf.yml` · perf budgets (small class) · Post Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.1 |
| `perf.yml` · perf budgets (small class) · the gate can fail: self-test (no build, no f | 19 | 0 | 0.0 | 0.0 | 0.1 |
| `privacy.yml` · default binary, egress refused · Post Run Swatinem/rust-cache@v2 | 9 | 0 | 0.0 | 0.0 | 0.1 |
| `privacy.yml` · default binary, egress refused · Run actions/upload-artifact@v4 | 9 | 0 | 0.0 | 0.0 | 0.1 |
| `privacy.yml` · default binary, egress refused · prove the egress trap traps | 9 | 0 | 0.0 | 0.0 | 0.1 |
| `privacy.yml` · posture (closure, consent tabl · Post Run actions/checkout@v4 | 9 | 0 | 0.0 | 0.0 | 0.1 |
| `privacy.yml` · posture (closure, consent tabl · Set up job | 9 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (aarch64 · Complete job | 7 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (aarch64 · Fetch local artifacts | 7 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (aarch64 · Post Run actions/checkout@v6 | 7 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (aarch64 · Post-build | 7 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (aarch64 · Install dependencies | 17 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (aarch64 · Post Run swatinem/rust-cache@v2 | 17 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (x86_64- · Complete job | 17 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (x86_64- · Install dependencies | 17 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (x86_64- · Post Run swatinem/rust-cache@v2 | 17 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · build-local-artifacts (x86_64- · Post Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.0 | 0.1 |
| `release.yml` · plan · Post Run actions/checkout@v6 | 18 | 0 | 0.0 | 0.0 | 0.1 |
| `sbom.yml` · the committed artefacts match  · Set up job | 8 | 0 | 0.0 | 0.0 | 0.1 |
| `ci.yml` · Guardrail 3 calibration reprod · Complete job | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · Guardrail 3 calibration reprod · Run actions/setup-python@v5 | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · beta (allowed to fail) · Complete job | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · beta (allowed to fail) · install beta | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (macos-latest · msr · toolchain | 12 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (macos-latest · sta · toolchain | 11 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (ubuntu-latest · ms · Complete job | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (ubuntu-latest · ms · toolchain | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (ubuntu-latest · st · Complete job | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (ubuntu-latest · st · toolchain | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (windows-latest · m · Complete job | 12 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · build+test (windows-latest · s · Complete job | 12 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · cargo-deny (licences, advisori · Complete job | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · cargo-deny (licences, advisori · Post Run actions/checkout@v4 | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · cargo-deny (licences, advisori · Run actions/setup-python@v5 | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · check-gate (the CI gate, on it · Complete job | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · check-gate (the CI gate, on it · the SARIF is a SARIF an uploader will take | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · clippy (strict) · Complete job | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · fmt (rustfmt) · Complete job | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · out-of-tree-plugin (the exampl · Complete job | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · out-of-tree-plugin (the exampl · and refuse a document that is not a delta do | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · packaging (deb, rpm, and the i · Complete job | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · packaging (deb, rpm, and the i · Run actions/setup-python@v5 | 13 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · packaging (deb, rpm, and the i · the packager's own rules (its negative contr | 10 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · release skeleton matches dist- · Complete job | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · release skeleton matches dist- · One version, seven places (P01-P08) | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · release skeleton matches dist- · Post Run actions/setup-python@v5 | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · release skeleton matches dist- · Run actions/setup-python@v5 | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · release skeleton matches dist- · The release path (R01-R17) | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · release skeleton matches dist- · the .deb/.rpm packager's negative controls | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · release skeleton matches dist- · the version bump's negative controls | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · spike s1 reproduces (six gramm · Complete job | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `ci.yml` · structural extractor core and  · Complete job | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `contracts.yml` · docs + workspace contracts · Complete job | 18 | 0 | 0.0 | 0.0 | 0.0 |
| `contracts.yml` · docs + workspace contracts · Post Run actions/checkout@v4 | 18 | 0 | 0.0 | 0.0 | 0.0 |
| `contracts.yml` · docs + workspace contracts · Run actions/setup-python@v5 | 18 | 0 | 0.0 | 0.0 | 0.0 |
| `contracts.yml` · docs + workspace contracts · e0.8 config: the config reference tables mat | 18 | 3 | 0.0 | 0.0 | 0.0 |
| `contracts.yml` · docs + workspace contracts · reco-j1o no shipped sentence claims a closed | 18 | 0 | 0.0 | 0.0 | 0.0 |
| `contracts.yml` · docs + workspace contracts · reco-yuk: the workflows are checked against  | 17 | 1 | 0.0 | 0.0 | 0.0 |
| `docs.yml` · build the html site · Complete job | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `docs.yml` · build the html site · Post Run actions/checkout@v4 | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `docs.yml` · build the html site · Post Run actions/setup-python@v5 | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `docs.yml` · build the html site · Run actions/setup-python@v5 | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `docs.yml` · build the html site · the generated pages are still the program's  | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `docs.yml` · build the html site · what got built | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `dogfood.yml` · fathom check over the corpus · Complete job | 3 | 0 | 0.0 | 0.0 | 0.0 |
| `dogfood.yml` · fathom check over the corpus · Post Run actions/checkout@v4 | 3 | 0 | 0.0 | 0.0 | 0.0 |
| `dogfood.yml` · fathom check over the corpus · Set up job | 3 | 0 | 0.0 | 0.0 | 0.0 |
| `dogfood.yml` · fathom check over the corpus · the artefacts | 3 | 0 | 0.0 | 0.0 | 0.0 |
| `dogfood.yml` · fathom check over the corpus · the corpus, as a sentence | 3 | 0 | 0.0 | 0.0 | 0.0 |
| `dogfood.yml` · fathom check over the corpus · the pins this job will gate | 3 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (hooks) · Complete job | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (hooks) · Post Run actions/checkout@v4 | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (hooks) · Seed the corpus cargo-fuzz will run | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (hooks) · The committed corpus is the generated one | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (lockfile) · Complete job | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (lockfile) · Seed the corpus cargo-fuzz will run | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (lockfile) · The committed corpus is the generated one | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (paths) · Complete job | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (paths) · Seed the corpus cargo-fuzz will run | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `fuzz.yml` · smoke (paths) · The committed corpus is the generated one | 15 | 0 | 0.0 | 0.0 | 0.0 |
| `perf.yml` · perf budgets (small class) · Complete job | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `perf.yml` · perf budgets (small class) · Run actions/setup-python@v5 | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `perf.yml` · perf budgets (small class) · budgets.json, BUDGETS.md and the ADR table a | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `perf.yml` · perf budgets (small class) · the trend, for the run summary | 19 | 0 | 0.0 | 0.0 | 0.0 |
| `privacy.yml` · default binary, egress refused · Complete job | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `privacy.yml` · default binary, egress refused · Post Run actions/checkout@v4 | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `privacy.yml` · default binary, egress refused · default binary links zero network-capable cr | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `privacy.yml` · posture (closure, consent tabl · Complete job | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `privacy.yml` · posture (closure, consent tabl · Post Run actions/setup-python@v5 | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `privacy.yml` · posture (closure, consent tabl · Run actions/setup-python@v5 | 9 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (aarch64 · Install dependencies | 7 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (aarch64 · enable windows longpaths | 7 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (aarch64 · Complete job | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (aarch64 · Post Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (aarch64 · Post-build | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (aarch64 · enable windows longpaths | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Install dependencies | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Post-build | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · enable windows longpaths | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Complete job | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Complete job | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Install dependencies | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Post Run actions/checkout@v6 | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Post-build | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · enable windows longpaths | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Complete job | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · Post-build | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · build-local-artifacts (x86_64- · enable windows longpaths | 17 | 0 | 0.0 | 0.0 | 0.0 |
| `release.yml` · plan · Complete job | 18 | 0 | 0.0 | 0.0 | 0.0 |
| `sbom.yml` · the committed artefacts match  · Complete job | 8 | 0 | 0.0 | 0.0 | 0.0 |
| `sbom.yml` · the committed artefacts match  · Post Run actions/checkout@v4 | 8 | 0 | 0.0 | 0.0 | 0.0 |

## `Swatinem/rust-cache`, by proxy

| workflow | restores | of which pulled bytes | saves run | saves skipped (job was red) |
|---|---|---|---|---|
| `ci.yml` | 233 | 196 | 131 | 102 |
| `release.yml` | 92 | 92 | 82 | 10 |
| `perf.yml` | 19 | 19 | 2 | 17 |
| `privacy.yml` | 9 | 9 | 9 | 0 |

