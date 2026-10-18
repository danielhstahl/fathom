# The perf budget commands (`e0.11`, `reco-pef`): ADR-004 §11's budgets as runnable gates.
#
# Only the perf gate has a make target, and it has one because the ticket's acceptance criterion is
# literally "make bench reproduces the numbers locally in under 5 minutes". Everything else in this
# repository is invoked the way `ci.yml` and `contracts.yml` invoke it — `cargo`, `python3 tools/…`,
# `pytest` — and this file deliberately does not grow a parallel build system for those.
#
# The gate itself is `tools/perf_gate.py`; the numbers are `benchmarks/budgets.json`; the blessed
# baseline is `benchmarks/baselines/perf-gate.json`; and `benchmarks/BUDGETS.md` is the page that
# lists every threshold with what asserts it. The numbers below are the `small` size class on the
# deterministic fixture corpus, which is the class a CI runner can afford to assert on.
#
# These targets need POSIX (`/proc`, a pty, `sleep`). The Windows CI legs build and test Rust; the
# budget gate is a linux job, and says so rather than half-running elsewhere.

BIN := target/release/fathom
GATE := python3 tools/perf_gate.py

.PHONY: bench bench-check bench-bless bench-selftest bench-criterion bench-fast

## bench: build the release binary, measure everything, print the verdicts (~30 s)
bench: $(BIN)
	@$(GATE)

## bench-check: as CI runs it — also the 15 % regression gate and the bless-the-baseline rule
bench-check: $(BIN)
	@$(GATE) --check

## bench-bless: rewrite the baseline from this run. The ratchet, done as a deliberate commit.
bench-bless: $(BIN)
	@$(GATE) --bless

## bench-selftest: prove the gate can go red, with no build and no fixture (milliseconds)
bench-selftest:
	@$(GATE) --self-test

## bench-criterion: just the criterion benches (the store-side micro-budgets)
bench-criterion:
	cargo bench -q -p fathom-store --features sqlite-bundled

## bench-fast: the same measurements, fewer samples; for iterating, not for blessing
bench-fast: $(BIN)
	@$(GATE) --samples 1 --skip-benches

$(BIN):
	cargo build --release -p fathom-cli
