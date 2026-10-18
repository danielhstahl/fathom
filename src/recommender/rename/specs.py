"""The fixture set: twelve authored histories, each one a case `s4` has to price.

Each spec is a :class:`~recommender.rename.repo.Script` plus the *intent* behind it. The intent is not
the ground truth — the ground truth is the hand-authored JSON in `truth/`, which says what a reviewer
would agree with and which no detector here computes. What lives here is the authoring note: why the
history is shaped this way, and which failure it exists to catch.

| scenario | the case | what it can cost a detector |
|---|---|---|
| `clean_moves` | the floor: bulk moves, bytes unchanged | a detector that fails here is not worth pricing |
| `rename_rewrite` | five moves with graded rewrites, a quarter replaced to nothing left | recall falls as the rewrite deepens; that is `e2.4`'s whole thesis |
| `split_one_to_many` | one module becomes three | 1:1 pairing silently drops two thirds of the truth |
| `merge_many_to_one` | three modules become one | the mirror case, equally 1:1-hostile |
| `copies_survive` | a file is copied and the original stays | no deletion pairs with the addition, so a rename detector sees nothing |
| `cross_merge` | a clean move on a branch, read at the merge commit | whether the walk's merge policy lets a reviewer see the pair at all |
| `conflict_merge` | a clean move on one side, a rewrite on the other, resolved into a third thing | the same pair is trivial on one window and hopeless on the next |
| `evil_merge` | a move that exists only inside the merge commit | a spine that skips merges never sees it |
| `split_episode_move` | the deletion and the addition in two commits of one turn | no commit-pair window contains both halves |
| `rebase_rewrites` | a move introduced by a rebase, read before and after | a pairing that changes when the branch is rewritten is a pairing no reviewer can rely on |
| `rebase_squash` | a squash rebase fusing a clean move with a later rewrite | the pair falls off git's cliff when the history is compressed |
| `rebase_over_delete` | a move rebased onto a history that deleted the source | no window-local method can recover it; the fixture prices the ceiling |
| `scaffold_decoys` | near-identical scaffolded files, one real move | the wrong pairing a content matcher is most confident about |
| `generated_and_binary` | a lockfile swap, a byte-identical binary move, a regenerated module | no content evidence at all in two of the three |
| `reformat_then_delayed` | a reformat-only turn, then a delayed move inside a noisy turn | a fabricated pairing out of a formatter run |
| `bulk_refactor` | 60-file reorganisation: moves, a split, new files, reformat noise | scale: cost, and precision when one wrong pair in fifty is still a lie |
"""

from __future__ import annotations

from .content import (binary_blob, body, concat, heavy_edit, light_edit, partition, regenerated,
                      shared_scaffold)
from .repo import Script


def clean_moves() -> tuple[Script, str]:
    """The floor: six paths move with unchanged bytes, then a turn that moves one and edits another."""
    s = Script("clean_moves")
    for path, blocks in [("src/core/store.py", 6), ("src/api/routes.py", 8),
                         ("src/queue/worker.py", 7), ("src/queue/sched.py", 5),
                         ("tests/test_api.py", 6), ("README.md", 4)]:
        s.write(path, body(path, blocks))
    s.write(".gitignore", "output/\n")
    s.commit("c0", message="scaffold the tree")

    s.move("src/core/store.py", "src/store/mod.py")
    s.move("src/api/routes.py", "src/http/handlers.py")
    s.move("src/queue/sched.py", "src/queue/scheduler.py")
    s.move("tests/test_api.py", "tests/test_store.py")
    s.write("src/core/cache.py", body("src/core/cache.py", 4))
    s.commit("c1", message="move four modules to their new homes", author="pi-agent")

    s.move("src/http/handlers.py", "src/http/api.py")
    s.write("src/queue/worker.py", light_edit(body("src/queue/worker.py", 7), key="c2"))
    s.commit("c2", message="move the handlers, tune the worker", author="pi-agent")
    return s, (
        "Four byte-identical moves in one turn, then a turn that moves one file while editing "
        "another. Every detector must be perfect here; this is the case that prices the floor, and a "
        "detector that loses pairs here has a bug rather than a trade-off.")


def rename_rewrite() -> tuple[Script, str]:
    """Five moves whose content is rewritten by a graded amount — the fixture `e2.4` names."""
    s = Script("rename_rewrite")
    base = {f"src/mod{i}.py": body(f"src/mod{i}.py", 9) for i in range(5)}
    for path, text in base.items():
        s.write(path, text)
    s.commit("c0", message="five modules")

    # graded by intent: a light touch, then nearly half replaced, then most of it, then all but the
    # header, then nothing but the header.
    s.write("src/moved/mod0.py", light_edit(base["src/mod0.py"], key="r0", fraction=0.2))
    s.write("src/moved/mod1.py", light_edit(base["src/mod1.py"], key="r1", fraction=0.45))
    s.write("src/moved/mod2.py", light_edit(base["src/mod2.py"], key="r2", fraction=0.8))
    s.write("src/moved/mod3.py", heavy_edit(base["src/mod3.py"], key="r3", keep_blocks=1))
    s.write("src/moved/mod4.py", heavy_edit(base["src/mod4.py"], key="r4", keep_blocks=0))
    for path in base:
        s.remove(path)
    s.commit("c1", message="move every module and rewrite it on the way", author="pi-agent")
    return s, (
        "Five renames, graded by how much of the file survived: a light edit, a medium edit, most of "
        "it replaced, all but the header replaced, and nothing but a shared header left. By author "
        "intent all five are renames. The last two are the ones no content algorithm can honestly "
        "call a rename — a reviewer staring at the two blobs cannot tell them from an unrelated new "
        "file either — which is what makes this the fixture that decides fact versus guess.")


def split_one_to_many() -> tuple[Script, str]:
    """One module becomes three, then another becomes three again with new content added."""
    s = Script("split_one_to_many")
    report = body("src/report.py", 12)
    feeds = body("src/legacy/feeds.py", 9)
    s.write("src/report.py", report)
    s.write("src/legacy/feeds.py", feeds)
    s.commit("c0", message="two fat modules")

    for name, part in zip(("text", "json", "csv"), partition(report, 3)):
        s.write(f"src/report/{name}.py", part)
    s.remove("src/report.py")
    s.commit("c1", message="split the report module three ways", author="pi-agent")

    parts = partition(feeds, 3)
    parts[0] = parts[0] + "\n\ndef new_ingest(source: str) -> None:\n" \
                          '    """Content that was never in the old file."""\n' \
                          "    return None\n"
    for name, part in zip(("rss", "atom", "jsonfeed"), parts):
        s.write(f"src/feeds/{name}.py", part)
    s.remove("src/legacy/feeds.py")
    s.commit("c2", message="split the feeds module and grow one part", author="pi-agent")
    return s, (
        "One file becomes three, twice; the second split also adds code that never existed in the "
        "source. A 1:1 pairing can report at most one of the three destinations and silently drops "
        "the rest, so this is the fixture that forces the representation question `e2.4`'s second "
        "acceptance criterion is about.")


def merge_many_to_one() -> tuple[Script, str]:
    """Three modules become one."""
    s = Script("merge_many_to_one")
    names = ("clock", "calendar", "filesystem")
    parts = {name: body(f"src/legacy/{name}.py", 5) for name in names}
    for name, text in parts.items():
        s.write(f"src/legacy/{name}.py", text)
    s.write("src/util/time.py", body("src/util/time.py", 3))
    s.commit("c0", message="three legacy modules and a util")

    s.write("src/util/time.py", concat([parts[n] for n in names], module="src/util/time.py"))
    for name in names:
        s.remove(f"src/legacy/{name}.py")
    s.commit("c1", message="fold the legacy modules into util", author="pi-agent")
    return s, (
        "Three deletions and one *modified* destination: `util/time.py` already existed and absorbed all "
        "three. Three "
        "pairs are true at once; a detector that emits one is wrong twice and missing once, and a "
        "reviewer told 'clock became util/time' will stop trusting the pane for the rest of the "
        "review.")


def copies_survive() -> tuple[Script, str]:
    """Copies where the original stays: a rename detector cannot see these at all."""
    s = Script("copies_survive")
    client = body("src/client.py", 8)
    s.write("src/client.py", client)
    s.write("src/admin.py", shared_scaffold("src/admin.py", variant=1))
    s.commit("c0", message="the client and an unrelated admin module")

    s.copy("src/client.py", "src/client_v2.py")
    s.commit("c1", message="vendor a v2 copy of the client", author="pi-agent")

    s.write("src/client_v3.py", light_edit(client, key="copy-edit", fraction=0.3))
    s.write("src/admin_v2.py", shared_scaffold("src/admin_v2.py", variant=2))
    s.commit("c2", message="copy the client again with edits; add a second admin module",
             author="pi-agent")
    return s, (
        "Two copies, one byte-identical and one edited, where the source file is never deleted. A "
        "rename detector cannot see a copy at all — no deletion pairs with the addition — and a "
        "content matcher widened to the whole tree to find them is exactly as likely to pair "
        "`admin.py` with `admin_v2.py`, which is a scaffold twin and not a copy. The copy case is "
        "where a wider candidate set is the point and the danger at once.")


def cross_merge() -> tuple[Script, str]:
    """A move made on a topic branch, read at the merge commit."""
    s = Script("cross_merge")
    s.write("src/core/engine.py", body("src/core/engine.py", 9))
    s.write("src/core/config.py", body("src/core/config.py", 5))
    s.write("docs/notes.md", "# notes\n\nan unrelated page\n" * 4)
    s.commit("c0", message="main: the tree before the branch")

    s.branch("topic", at="c0").checkout("topic")
    s.write("src/engine/__init__.py", "from .core import *  # noqa\n")
    s.move("src/core/engine.py", "src/engine/core.py")
    s.commit("t1", branch="topic", message="topic: move engine into its own package",
             author="pi-agent")

    s.checkout("main")
    s.write("docs/notes.md", "# notes\n\nan unrelated page\n" * 4
            + "\nMain added this while the branch was open.\n")
    s.write("src/core/config.py", light_edit(body("src/core/config.py", 5), key="main-edit",
                                             fraction=0.3))
    s.commit("c1", message="main: notes and a config tune")

    s.merge("m1", "main", "topic")
    return s, (
        "The move happens on a topic branch and reaches main inside a merge commit, and the merge is "
        "clean. Read against the first parent git calls it a rename at R100, so the interesting "
        "question is not the matcher at all: it is the walk. Under the shipped `MergePolicy::Skip` a "
        "merge contributes no file rows, and a first-parent walk never visits the topic commit, so "
        "whether a reviewer ever sees this move is a decision `e1.2` already made.")


def conflict_merge() -> tuple[Script, str]:
    """A rename/delete conflict resolved by hand: the merge carries a move whose evidence is its own."""
    s = Script("conflict_merge")
    engine = body("src/engine.py", 10)
    s.write("src/engine.py", engine)
    s.write("src/main.py", body("src/main.py", 4))
    s.commit("c0", message="the tree before both sides moved")

    s.branch("move", at="c0").checkout("move")
    s.move("src/engine.py", "src/engine/v2.py")
    s.commit("t1", branch="move", message="move the engine, touching nothing", author="pi-agent")

    s.checkout("main")
    s.write("src/engine.py", heavy_edit(engine, key="main-rewrite", keep_blocks=2))
    s.commit("c1", message="rewrite the engine in place")

    # The authored resolution: the reviewer keeps the branch's new path and main's rewrite, but
    # finishes the job — the merged file is neither parent's bytes. `evil_merge`'s op list is the
    # resolution, which is the only way to make a merge commit carry exactly what a human wrote.
    s.evil_merge("m1", "main", "move", ops=[
        ("delete", "src/engine.py"),
        ("write", "src/engine/v2.py", heavy_edit(engine, key="resolution", keep_blocks=4)),
    ])
    return s, (
        "One branch moves the file with no edit at all — R100 on the branch — while main rewrites it "
        "where it stands, and the reviewer resolving the conflict keeps the new path and finishes the "
        "rewrite. Reading the merge against its first parent, the deleted blob is main's rewrite and "
        "the added blob is the resolution, and they match only loosely. The same pair is trivial on "
        "one window and nearly hopeless on the next, which is the finding about *window choice* "
        "`e2.4` has to ship a decision for.")


def evil_merge() -> tuple[Script, str]:
    """The move exists only in the merge commit: neither parent contains it."""
    s = Script("evil_merge")
    s.write("src/pkg/service.py", body("src/pkg/service.py", 8))
    s.write("src/pkg/util.py", body("src/pkg/util.py", 5))
    s.write("docs/arch.md", "# architecture\n\nthe package layout, as documented\n" * 3)
    s.commit("c0", message="the layout both branches share")

    s.branch("feature", at="c0").checkout("feature")
    s.write("src/pkg/service.py", light_edit(body("src/pkg/service.py", 8), key="feat", fraction=0.3))
    s.commit("t1", branch="feature", message="feature: touch the service", author="pi-agent")

    s.checkout("main")
    s.write("src/pkg/util.py", light_edit(body("src/pkg/util.py", 5), key="main-util", fraction=0.2))
    s.commit("c1", message="main: touch the util")

    s.merge("m_plain", "main", "feature")
    # The second merge does the reorganisation inside the merge itself.
    s.branch("feature2", at="c1").checkout("feature2")
    s.write("src/pkg/service.py", light_edit(body("src/pkg/service.py", 8), key="feat2",
                                             fraction=0.25))
    s.commit("t2", branch="feature2", message="feature2: touch the service again", author="pi-agent")
    s.checkout("main")
    s.write("docs/arch.md", "# architecture\n\nthe package layout, as documented\n" * 3
            + "\nA note added on main.\n")
    s.commit("c2", message="main: document the layout")
    s.evil_merge("m_evil", "main", "feature2", ops=[
        ("move", "src/pkg/service.py", "src/services/service.py"),
        ("write", "docs/arch.md", "# architecture\n\nthe services package, as reorganised\n" * 3),
    ])
    return s, (
        "Two merges of the same shape, one clean and one evil. The evil one moves a file *inside the "
        "merge commit*: neither parent contains the destination path. That is what a conflict "
        "resolution that turns into a reorganisation looks like, and it is invisible to a spine that "
        "skips merges, which is the shipped default.")


def split_episode_move() -> tuple[Script, str]:
    """The deletion and the addition land in two different commits of one episode."""
    s = Script("split_episode_move")
    s.write("src/legacy/importer.py", body("src/legacy/importer.py", 9))
    s.write("src/legacy/format.py", body("src/legacy/format.py", 6))
    s.commit("c0", message="the legacy importer")

    s.remove("src/legacy/importer.py")
    s.commit("t1", message="drop the importer", author="pi-agent",
             trailer="Agent-Turn: 1")
    s.write("src/ingest/importer.py", body("src/legacy/importer.py", 9))
    s.commit("t2", message="put it back where it belongs", author="pi-agent",
             trailer="Agent-Turn: 1")
    return s, (
        "One turn, two commits: the old path dies in the first and the new one is born in the second. "
        "No commit-pair window contains both halves, so per-commit detection cannot produce this pair "
        "at any threshold, while the episode window sees it at R100. It is the cleanest evidence in "
        "the set that the *unit* of rename detection is a decision, not a detail.")


def rebase_rewrites() -> tuple[Script, str]:
    """A move introduced by a rebase, readable before and after the rewrite."""
    s = Script("rebase_rewrites")
    handlers = body("src/api/handlers.py", 8)
    s.write("src/api/handlers.py", handlers)
    s.write("src/api/models.py", body("src/api/models.py", 6))
    s.commit("c0", message="the api as it was")

    s.branch("reorder", at="c0").checkout("reorder")
    s.write("src/http/__init__.py", "from .handlers import *  # noqa\n")
    s.move("src/api/handlers.py", "src/http/handlers.py")
    s.commit("t1", branch="reorder", message="move handlers to the http package",
             author="pi-agent")

    s.checkout("main")
    s.write("src/api/models.py", light_edit(body("src/api/models.py", 6), key="main-models",
                                            fraction=0.5))
    s.write("src/api/auth.py", body("src/api/auth.py", 5))
    s.commit("c1", message="main grows an auth module")

    s.checkout("reorder")
    s.rebase("r1", "reorder", "main")
    return s, (
        "The move exists at `t1` and, after `git rebase main`, at a different commit under a "
        "different SHA with different parents. A reviewer's ledger and Fathom's episode identity both "
        "have to survive that; here the question is narrower and sharper — does the *pairing* come "
        "out the same on both sides of the rewrite? If it does not, the same episode changes its "
        "answer when the branch moves, which is worse than a missing pair.")


def rebase_squash() -> tuple[Script, str]:
    """A squash rebase fuses a clean move and a later rewrite into one heavy-rewrite move."""
    s = Script("rebase_squash")
    report = body("src/report.py", 10)
    s.write("src/report.py", report)
    s.write("src/render.py", body("src/render.py", 5))
    s.commit("c0", message="the flat layout")

    s.branch("tidy", at="c0").checkout("tidy")
    s.move("src/report.py", "src/output/report.py")
    s.commit("t1", branch="tidy", message="move the report into output/", author="pi-agent")
    s.write("src/output/report.py", heavy_edit(report, key="squash-edit", keep_blocks=1))
    s.commit("t2", branch="tidy", message="rewrite the report for the new layout", author="pi-agent")

    s.checkout("main")
    s.write("src/render.py", light_edit(body("src/render.py", 5), key="main-render", fraction=0.3))
    s.commit("c1", message="main tunes the renderer")

    s.checkout("tidy")
    s.squash("r1", "tidy", "main")
    return s, (
        "Pre-squash the reviewer saw two commits: a move at R100 and an edit to the moved file. "
        "Squashed — which is what `rebase -i` and every 'squash before merge' button produces — the "
        "one commit's window holds a move with most of the file replaced. The pairing is the same "
        "pair, and it falls off git's cliff between the two histories. A ledger that recorded it as a "
        "fact before the squash has to be able to be wrong after it.")


def rebase_over_delete() -> tuple[Script, str]:
    """A move rebased onto a history that deleted the source: the pair has no evidence left."""
    s = Script("rebase_over_delete")
    s.write("src/legacy/queue.py", body("src/legacy/queue.py", 7))
    s.write("src/legacy/README.md", "# legacy\n\nslated for removal\n" * 3)
    s.commit("c0", message="the legacy queue")

    s.branch("moveit", at="c0").checkout("moveit")
    s.move("src/legacy/queue.py", "src/queue/legacy.py")
    s.commit("t1", branch="moveit", message="move the legacy queue under src/queue",
             author="pi-agent")

    s.checkout("main")
    s.remove("src/legacy/queue.py")
    s.commit("c1", message="delete the legacy queue, it is dead")

    s.checkout("moveit")
    s.rebase("r1", "moveit", "main", resolve={"src/queue/legacy.py": body("src/legacy/queue.py", 7)})
    return s, (
        "The branch moves a file main has deleted. The rebased commit contains an addition and no "
        "deletion at all: no window-local method — git's, a matcher's, or a human's reading of the "
        "diff — can recover the pair from the commit, because the source is not in it. The truth file "
        "marks this pair unrecoverable, and the report states the ceiling rather than scoring a "
        "method on a question it cannot ask.")


def scaffold_decoys() -> tuple[Script, str]:
    """A real move with weak evidence, next to scaffold twins with strong-looking evidence."""
    s = Script("scaffold_decoys")
    invoices = body("src/handlers/invoices.py", 8)
    s.write("src/handlers/orders.py", shared_scaffold("src/handlers/orders.py", variant=1))
    s.write("src/handlers/invoices.py", invoices)
    s.write("src/handlers/shipments.py", shared_scaffold("src/handlers/shipments.py", variant=2))
    s.commit("c0", message="three handlers, two of them scaffolded from one template")

    # The true move: invoices -> billing, rewritten on the way, so its own evidence is weak.
    s.write("src/handlers/billing.py", heavy_edit(invoices, key="scaffold-move", keep_blocks=1))
    s.remove("src/handlers/invoices.py")
    # And a genuinely new scaffolded module in the same turn: it resembles the deleted one a lot and
    # has nothing to do with it.
    s.write("src/handlers/quotes.py", shared_scaffold("src/handlers/quotes.py", variant=3))
    s.remove("src/handlers/orders.py")
    s.commit("c1", message="move the invoice handler to billing; replace the orders handler",
             author="pi-agent")
    return s, (
        "One turn, two deletions, two additions. The true pair (`invoices.py` to `billing.py`) was "
        "rewritten on the way and carries weak evidence; the false pair (`orders.py` to `quotes.py`, "
        "and `orders.py` to `billing.py`) is scaffold overlap, which is large, stable, and has nothing "
        "to do with provenance. This fixture is not about recall: it is the manufactured ranking "
        "inversion the ticket asks about, where a wrong pairing outscores a right one.")


def generated_and_binary() -> tuple[Script, str]:
    """The cases where content evidence is missing on both ends."""
    s = Script("generated_and_binary")
    s.write("Cargo.lock", "\n".join(f'[[package]]\nname = "dep{i}"\nversion = "1.{i}.0"\n'
                                    for i in range(40)))
    s.write("assets/logo.bin", binary_blob(key="logo"))
    s.write("gen/models.py", body("gen/models.py", 6))
    s.write("src/parsers.py", body("src/parsers.py", 6))
    s.commit("c0", message="a lockfile, a binary, generated code and a parser")

    s.write("pnpm-lock.yaml", "\n".join(f"dep{i}:\n  version: 2.{i}.0\n" for i in range(55)))
    s.remove("Cargo.lock")
    s.move("assets/logo.bin", "assets/brand/logo.bin")
    s.write("gen/entities.py", regenerated(key="entities", blocks=7))
    s.remove("gen/models.py")
    s.commit("c1", message="swap the lockfile, move the asset, regenerate the models",
             author="deps-bot")
    return s, (
        "Three moves, two of which have no usable content evidence: a lockfile replaced by a "
        "different tool's lockfile at a different path, a byte-identical binary move (perfect content "
        "evidence, and the only case hashing alone solves), and a generated module regenerated at a "
        "new path with recognisably unrelated bytes. A spike that reports one number across these "
        "three is reporting nothing.")


def reformat_then_delayed() -> tuple[Script, str]:
    """A reformat-only turn, then a turn that mixes a delayed move with noise."""
    s = Script("reformat_then_delayed")
    handlers = body("src/http/handlers.py", 7)
    router = body("src/http/router.py", 6)
    store = body("src/core/store.py", 6)
    s.write("src/http/handlers.py", handlers)
    s.write("src/http/router.py", router)
    s.write("src/core/store.py", store)
    s.commit("c0", message="the tree before the formatter")

    s.write("src/http/handlers.py", handlers.replace("def ", "def  ").replace("    acc", "        acc"))
    s.write("src/http/router.py", router.replace("    out", "        out"))
    s.commit("c1", message="run the formatter", author="pi-agent")

    s.write("src/store/mod.py", light_edit(store, key="delayed", fraction=0.2))
    s.remove("src/core/store.py")
    s.write("src/http/router.py", router.replace("    acc", "      acc"))
    s.commit("c2", message="move store, reformat the router again", author="pi-agent")
    return s, (
        "Two turns of noise and one delayed move. The formatter pass is a change with no move in it "
        "at all, and the second turn is half noise and half a real move — the case where a rename "
        "detector and a noise detector disagree about who owns the file. Any pairing asserted on the "
        "reformat turn is fabricated out of whitespace.")


def bulk_refactor() -> tuple[Script, str]:
    """A 60-file reorganisation at a size a real refactor reaches."""
    s = Script("bulk_refactor")
    modules = [f"src/{group}/m{i}.py" for group in ("alpha", "beta", "gamma", "delta")
               for i in range(12)]
    for path in modules:
        s.write(path, body(path, 5))
    s.write("README.md", "# bulk\n\na repository the size of a real one\n")
    s.commit("c0", message="sixty modules in four groups")

    for path in modules:
        name = path.rsplit("/", 1)[-1]
        group = path.split("/")[1]
        if name == "m4.py":
            s.write(f"src/{group}/archive/{name}",
                    light_edit(body(path, 5), key=f"bulk-{path}", fraction=0.35))
            s.remove(path)
        elif name in ("m0.py", "m1.py", "m2.py", "m3.py"):
            s.move(path, f"src/{group}/archived_{name}")

    split_source = body("src/alpha/m9.py", 12)
    s.write("src/alpha/m9.py", split_source)
    for letter, part in zip(("a", "b", "c"), partition(split_source, 3)):
        s.write(f"src/split/{letter}.py", part)
    s.remove("src/alpha/m9.py")

    for index in range(4):
        s.write(f"src/new/service{index}.py", body(f"src/new/service{index}.py", 4))
    for path in [p for p in modules if p.endswith("m11.py")]:
        s.write(path, body(path, 5).replace("    out", "      out"))
    s.commit("c1", message="reorganise four packages into archives, split one module, add services",
             author="pi-agent")
    return s, (
        "Sixty files in one turn: twelve clean moves, four moves-with-rewrite, one module overwritten "
        "by a three-way split of a file that also moved, four unrelated new modules and four "
        "reformatted files. This is the size at which a quadratic candidate set starts to cost "
        "something, and at which one wrong pair in fifty is still a reviewer being lied to, so it "
        "prices both the cost bound and the precision bar.")


SPECS = {
    "clean_moves": clean_moves,
    "rename_rewrite": rename_rewrite,
    "split_one_to_many": split_one_to_many,
    "merge_many_to_one": merge_many_to_one,
    "copies_survive": copies_survive,
    "cross_merge": cross_merge,
    "conflict_merge": conflict_merge,
    "evil_merge": evil_merge,
    "split_episode_move": split_episode_move,
    "rebase_rewrites": rebase_rewrites,
    "rebase_squash": rebase_squash,
    "rebase_over_delete": rebase_over_delete,
    "scaffold_decoys": scaffold_decoys,
    "generated_and_binary": generated_and_binary,
    "reformat_then_delayed": reformat_then_delayed,
    "bulk_refactor": bulk_refactor,
}
