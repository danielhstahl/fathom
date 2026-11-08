"""The one spelling of the command line this fixture answers to, used in every remedy printed.

ADR-002 §4.3 designs a `fathom config <subverb>` surface, and this package is that design made
executable while the Rust verb stays unbuilt: the shipped grammar in
`crates/fathom-cli/src/verbs.rs` has no `config` variant, so a remedy printed as
`fathom config doc --write` is an instruction the reader cannot execute — the failure moves from
the message to the person. `reco-c81` is exactly that: CI failed on a stale generated table, the
remedy named a verb with no binary behind it, and the reader had to guess the real command.

So the runnable form is spelled once, here:

    PYTHONPATH=src python3 -m recommender.config …

`PYTHONPATH=src` is load-bearing, not ceremony: `recommender` is read out of `src/` and is not
installed anywhere, which is why every config step in `.github/workflows/contracts.yml` carries
the prefix. `docs/concepts/config.md` §11 spells its command list the same way, for the same
reason: a command a page tells you to run has to run. The short `python3 -m recommender.config …`
form stays in prose that *names* the surface rather than handing it to you.
"""

from __future__ import annotations

#: The invocation, exactly as a shell needs it when started from the repository root.
RUN = "PYTHONPATH=src python3 -m recommender.config"


def command(*argv: str) -> str:
    """The whole command line for `argv`, for a remedy that has to paste.

    `command("doc", "--write")` -> `PYTHONPATH=src python3 -m recommender.config doc --write`.
    """
    return " ".join((RUN, *argv))
