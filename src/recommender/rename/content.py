"""Deterministic file bodies whose *similarity* is an authored quantity.

A rename-detection fixture is not about paths, it is about how much of a file survived. `git`'s
similarity index, and every content matcher `e2.4` might build, score a (before, after) pair of
**blobs** — so the fixture set needs bodies whose overlap it can set on purpose: a move that kept
95 % of the file, a move that kept a fifth of it, a file split into three, three files merged into
one. Random text would not do: it makes every case the same case, and the interesting zone — the
threshold, where git and a matcher disagree — would be an accident of the seed.

So a file is a list of **blocks** (one function-shaped chunk each) and every operation `s4` needs is
an operation on that list:

| operation | what it does | the case it authors |
|---|---|---|
| :func:`keep` | returns the body verbatim | a move with no edit |
| :func:`light_edit` | perturbs a fraction of blocks in place | a rename-with-rewrite that a reviewer still calls the same file |
| :func:`heavy_edit` | keeps a head block and replaces the rest | a move that *looks* like a rewrite |
| :func:`partition` | distributes blocks over N bodies | a split, one file into several |
| :func:`concat` | joins bodies under one header | a merge, several files into one |
| :func:`shared_scaffold` | the boilerplate many real files share | the decoy: two files that are near-identical and have nothing to do with each other |
| :func:`regenerated` | same path, unrecognisable content | a generated or vendored file whose bytes cannot carry a pairing |

Blocks are drawn from a seeded generator, so the same call always writes the same bytes; every
fixture in :mod:`recommender.rename.specs` is byte-stable across runs, machines and git versions,
which is what lets `--check` demand an exact reproduction of the committed report.
"""

from __future__ import annotations

import hashlib
import random

#: Lines per block. Small enough that "replace half the blocks" moves a similarity index a lot,
#: large enough that a block is more than one shingle.
BLOCK_LINES = 7


def _rng(*key: object) -> random.Random:
    """A seeded RNG keyed by string coordinates — no float rounding, no platform drift."""
    digest = hashlib.sha256("\x00".join(str(k) for k in key).encode()).digest()
    return random.Random(int.from_bytes(digest[:8], "big"))


def header(module: str) -> str:
    """The two lines every module carries. Deliberately identical across files: this is the part of
    real code that matches everything and proves nothing."""
    return (f'"""{module}: a module in the fixture corpus."\n'
            f"from __future__ import annotations\n")


def block(module: str, index: int, *, flavour: str = "plain") -> str:
    """One function-shaped chunk, byte-stable for its (module, index, flavour)."""
    rng = _rng(module, index, flavour)
    name = f"{module.split('/')[-1].removesuffix('.py')}_f{index}"
    lines = [f"def {name}(self, payload: bytes, limit: int = {rng.randrange(2, 60)}) -> dict:",
             f'    """{flavour.title()} step {index} of {module}."""']
    for row in range(BLOCK_LINES - 3):
        lines.append(f"    acc{row} = (payload[{row}] + {rng.randrange(100, 999)}) * {row + 1}")
        lines.append(f"    out['{name}_{row}'] = acc{row} % {rng.randrange(1000, 9999)}")
    lines.append("    return out")
    return "\n".join(lines)


def body(module: str, blocks: int = 8, *, flavour: str = "plain") -> str:
    """A whole file: the shared header plus `blocks` blocks."""
    parts = [header(module)]
    parts += [block(module, i, flavour=flavour) for i in range(blocks)]
    return "\n\n".join(parts) + "\n"


def perturb(text: str, fraction: float, *, key: str) -> str:
    """Rewrite a fraction of the blocks in place — same names, different constants.

    This is the shape of a real edit inside a moved file: the reviewer sees the same functions and a
    couple of changed lines, and so does every line-based similarity score. `fraction` is approximate
    by design; the report records the similarity git actually measured, so the fixture does not have
    to hit a number exactly, it only has to spread the cases out.
    """
    chunks = text.split("\n\n")
    rng = _rng("perturb", key, fraction)
    out = [chunks[0]]
    for index, chunk in enumerate(chunks[1:], start=1):
        if rng.random() < fraction:
            lines = []
            for line in chunk.split("\n"):
                if line.startswith("    acc") or line.startswith("    out["):
                    lines.append(line.rstrip() + f"  # revised {rng.randrange(10, 99)}")
                else:
                    lines.append(line)
            out.append("\n".join(lines))
        else:
            out.append(chunk)
    return "\n\n".join(out)


def keep(text: str) -> str:
    return text


def light_edit(text: str, *, key: str = "light", fraction: float = 0.25) -> str:
    """A move that also changed: a reviewer calls it the same file, and so should the detector."""
    return perturb(text, fraction, key=key)


def heavy_edit(text: str, *, key: str = "heavy", keep_blocks: int = 1) -> str:
    """A move that rewrote almost everything.

    Only the header and `keep_blocks` survive, and the survivors are perturbed too, so a similarity
    score sees a shared preamble and nothing else. This is the case `e2.4`'s description calls a
    rename-with-rewrite git misses — and the case where "show it as a fact" is least defensible,
    because the reviewer holding the diff cannot tell it from an unrelated new file either.
    """
    chunks = text.split("\n\n")
    out = [perturb(chunks[0], 0.0, key=key)]
    for i in range(min(keep_blocks, len(chunks) - 1)):
        out.append(perturb(chunks[1 + i], 0.6, key=f"{key}:{i}"))
    for i in range(4):
        out.append(f"def new_part_{i}_{key.replace('-', '_')}(self) -> None:\n"
                   f"    \"\"\"Authored new content for a rewritten move.\"\"\"\n"
                   f"    self.value = {i * 7 + 3}\n    return None")
    return "\n\n".join(out) + "\n"


def partition(text: str, parts: int) -> list[str]:
    """Split one body into `parts` bodies — a real split: every block survives, in a different file.

    Each part keeps the shared header, because a real split keeps the imports with the code that
    needs them, which is exactly the boilerplate that makes a split look like a copy to a matcher.
    """
    chunks = text.split("\n\n")
    head, blocks = chunks[0], chunks[1:]
    out = []
    for part in range(parts):
        mine = [b for i, b in enumerate(blocks) if i % parts == part]
        out.append("\n\n".join([head] + mine) + "\n")
    return out


def concat(bodies: list[str], *, module: str) -> str | None:
    """Merge several bodies into one file — the reverse operation, and just as common."""
    if not bodies:
        return None
    merged = [header(module)]
    for text in bodies:
        merged.append(text.split("\n\n", 1)[1] if "\n\n" in text else text)
    return "\n\n".join(merged) + "\n"


def shared_scaffold(module: str, *, variant: int, blocks: int = 8, shared: int = 5) -> str:
    """Files that share a scaffold and differ in their guts: a generator's output, a controller and its
    twin, the fifth HTTP handler that exists only because the framework wants one.

    The scaffold text is keyed by the *template*, not by the file — which is the whole point: real
    scaffolded files share their first fifty lines verbatim. A content matcher's favourite false pair,
    because the shared part is a large fraction of each file and has nothing to do with provenance.
    """
    stem = module.rsplit("/", 1)[-1].removesuffix(".py")
    common = "\n\n".join([header(f"scaffold/{stem}")[:0] or header("scaffold/template")]
                         + [block("scaffold/template", i) for i in range(shared)])
    mine = "\n\n".join(block(f"variant::{variant}", i) for i in range(max(1, blocks - shared)))
    return f"{common}\n\n{mine}\n"


def regenerated(*, key: str, blocks: int = 10) -> str:
    """What a lockfile or a code generator writes when it regenerates: a path survives, its bytes do
    not. No content algorithm can pair this with its own history, and pretending otherwise is how a
    spike manufactures a recall number nobody can earn."""
    return body(f"regenerated::{key}", blocks, flavour="regen")


def binary_blob(*, key: str, size: int = 512) -> bytes:
    """Deterministic non-text bytes, for the one case git pairs at 100 % by hash alone."""
    out = bytearray()
    counter = 0
    while len(out) < size:
        out += bytes.fromhex(hashlib.sha256(f"{key}:{counter}".encode()).hexdigest())[:16]
        counter += 1
    return bytes(out[:size])
