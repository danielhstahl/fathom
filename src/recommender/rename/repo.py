"""A scripted, byte-stable `git` repository writer — the histories `s4` measures are authored.

`s4`'s cases are not a diff shape, they are a *history* shape: a move that crossed a merge commit, a
move a rebase introduced, a file that arrived as three files. Producing those means running real
merges and real rebases, because the thing under test — git's own rename detection — is a property of
git's diff machinery and cannot be simulated without quietly testing a stand-in. So this module drives
`git` itself, and the same script always produces the same commit graph:

* one epoch, one identity per author, `GIT_*_DATE` bumped by one minute per authored step;
* `core.autocrlf=false`, no signing, and no config read from the machine
  (`GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_SYSTEM=/dev/null`), so the objects do not depend on who
  ran the bench or on what their `~/.gitconfig` says about `diff.renameLimit`;
* merge and rebase conflict resolutions spelled out by the spec rather than left to a strategy
  option's taste, wherever the spec cares what the resolved bytes are.

The writer returns `{label: oid}`, and the ground-truth files name commits by **label**. That is why
the fixtures survive a `git` upgrade that changes object ids: the pairs under test are pairs of
*paths*, and a label is how a human points at a commit without copying forty hex digits.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

EPOCH = 1_760_000_000  # one instant for every fixture; every step is an offset from it

#: The environment every `git` call here runs under, before identity and dates are added. Nothing is
#: read from the machine: no `~/.gitconfig`, no `/etc/gitconfig`, no ambient `GIT_*`. A fixture that
#: depended on who ran it would be a fixture that measures the operator's dotfiles, and `git diff`
#: has plenty of knobs a stray `[diff]` section could reach (`renameLimit`, `detectLimit`, ` renames`),
#: so reads go through :func:`neutral_env` rather than inheriting the runner's own.
BASE_ENV = {
    "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    "HOME": "/nonexistent",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_SYSTEM": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_MERGE_AUTOEDIT": "no",
    "GIT_TERMINAL_PROMPT": "0",
    "LC_ALL": "C",
    "TZ": "UTC",
}

IDENTITY = {
    "sam": ("sam@example.invalid", "Sam Human"),
    "pi-agent": ("pi-agent@example.invalid", "Pi Agent"),
    "codex-agent": ("codex-agent@example.invalid", "Codex Agent"),
    "deps-bot": ("deps-bot@example.invalid", "Deps Bot"),
}


class Op:
    """One tree edit, applied to the working tree just before the next commit."""

    def __init__(self, kind: str, *args: object) -> None:
        self.kind = kind
        self.args = args


class Step:
    """One authored action: a commit, a branch, a checkout, a merge or a rebase."""

    def __init__(self, kind: str, *, label: str | None = None, branch: str = "main",
                 ops: list[Op] | None = None, message: str = "", author: str = "sam",
                 **extra: object) -> None:
        self.kind = kind
        self.label = label
        self.branch = branch
        self.ops = ops or []
        self.message = message
        self.author = author
        self.extra = extra


class Script:
    """A readable authoring script.

    ```python
    s = Script("clean_moves")
    s.write("src/core/store.py", body(...))
    s.commit("c0", message="scaffold")
    s.move("src/core/store.py", "src/store/mod.py")
    s.commit("c1", author="pi-agent")
    ```

    `move` is a filesystem move plus `git add -A`, which is exactly how git sees a move: one deletion
    and one addition. Whether that pair *reads* as a rename is the question, so the writer must not
    answer it.
    """

    def __init__(self, name: str) -> None:
        self.name = name
        self.steps: list[Step] = []
        self._ops: list[Op] = []

    # -- tree edits, accumulated until the next commit ------------------------------------------

    def write(self, path: str, text: str | bytes) -> "Script":
        self._ops.append(Op("write", path, text))
        return self

    def remove(self, path: str) -> "Script":
        self._ops.append(Op("delete", path))
        return self

    def move(self, src: str, dst: str) -> "Script":
        self._ops.append(Op("move", src, dst))
        return self

    def copy(self, src: str, dst: str) -> "Script":
        self._ops.append(Op("copy", src, dst))
        return self

    # -- history --------------------------------------------------------------------------------

    def commit(self, label: str, *, branch: str = "main", message: str = "",
               author: str = "sam", trailer: str | None = None) -> "Script":
        text = message or f"{self.name}: {label}"
        if trailer:
            text = f"{text}\n\n{trailer}"
        self.steps.append(Step("commit", label=label, branch=branch, ops=self._ops,
                               message=text, author=author))
        self._ops = []
        return self

    def branch(self, name: str, *, at: str) -> "Script":
        self.steps.append(Step("branch", branch=name, at=at))
        return self

    def checkout(self, name: str) -> "Script":
        self.steps.append(Step("checkout", branch=name))
        return self

    def merge(self, label: str, into: str, from_ref: str, *, message: str = "",
              resolve: dict[str, str] | None = None, option: str | None = None) -> "Script":
        """`git merge --no-ff`, with an authored resolution for anything it cannot take quietly."""
        self.steps.append(Step("merge", label=label, branch=into,
                               message=message or f"Merge {from_ref} into {into}",
                               into=into, from_ref=from_ref, resolve=resolve or {},
                               option=option))
        return self

    def rebase(self, label: str, branch: str, onto: str, *,
               resolve: dict[str, str] | None = None) -> "Script":
        self.steps.append(Step("rebase", label=label, branch=branch, onto=onto,
                               resolve=resolve or {}))
        return self

    def squash(self, label: str, branch: str, onto: str) -> "Script":
        """Rewrite `branch` onto `onto` as **one** commit — what `rebase -i` and "squash these" does.

        Authored as a reset-plus-commit rather than an interactive rebase: an interactive rebase needs
        an editor and a todo list, and neither changes the history this fixture is about. The result is
        one commit on top of `onto` whose tree is the branch tip's, which is the property that makes a
        squash a squash.
        """
        self.steps.append(Step("squash", label=label, branch=branch, onto=onto))
        return self

    def evil_merge(self, label: str, into: str, from_ref: str, *, ops: list[tuple] | None = None,
                   message: str = "") -> "Script":
        """A merge commit whose tree is neither parent's — the evil merge, and a real one.

        A merge that only reconciles two sides is rare in agent-driven histories; what actually happens
        is that the conflict resolution *is* a reorganisation, and the move exists only in the merge
        commit. That is the case `s4`'s "renames that cross a merge commit" is about, and it is only
        constructible with `commit-tree`: no checkout produces it.
        """
        self.steps.append(Step("evil_merge", label=label, branch=into, into=into, from_ref=from_ref,
                               ops=[Op(*op) for op in (ops or [])],
                               message=message or f"Merge {from_ref} (with a reorganisation)"))
        return self


def _git(repo: Path, args: list[str], env: dict[str, str], check: bool = True
        ) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=repo, env=env, check=check, capture_output=True)


def neutral_env() -> dict[str, str]:
    """The environment for a read-only `git` call: no machine config, no ambient `GIT_*`, no identity.

    `detect.py` runs `git diff` against a built fixture, and that call is part of the measurement, so
    it gets the same isolation the writer gives the commits themselves.
    """
    return dict(BASE_ENV)


def _env(when: int, who: str) -> dict[str, str]:
    name, email = IDENTITY[who]
    stamp = f"{EPOCH + when * 60} +0000"
    return {
        **neutral_env(),
        "GIT_AUTHOR_NAME": name,
        "GIT_AUTHOR_EMAIL": email,
        "GIT_COMMITTER_NAME": name,
        "GIT_COMMITTER_EMAIL": email,
        "GIT_AUTHOR_DATE": stamp,
        "GIT_COMMITTER_DATE": stamp,
    }


def _apply(repo: Path, ops: list[Op]) -> None:
    for op in ops:
        if op.kind == "write":
            target = repo / str(op.args[0])
            target.parent.mkdir(parents=True, exist_ok=True)
            data = op.args[1]
            target.write_bytes(data.encode() if isinstance(data, str) else bytes(data))
        elif op.kind == "delete":
            target = repo / str(op.args[0])
            if target.exists():
                target.unlink()
        elif op.kind == "move":
            src, dst = repo / str(op.args[0]), repo / str(op.args[1])
            if not src.exists():
                raise FileNotFoundError(f"cannot move {op.args[0]}: not in the working tree")
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(src), str(dst))
        elif op.kind == "copy":
            source, target = repo / str(op.args[0]), repo / str(op.args[1])
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(source.read_bytes())
        else:  # pragma: no cover - a spec typo should fail loudly, not silently
            raise ValueError(f"unknown op {op.kind!r}")


def build(dest: Path, script: Script) -> dict[str, str]:
    """Write `script` into a fresh repository at `dest`; return `{label: oid}`."""
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True, exist_ok=True)
    _git(dest, ["init", "-q", "--initial-branch=main"], _env(0, "sam"))
    _git(dest, ["config", "core.autocrlf", "false"], _env(0, "sam"))

    labels: dict[str, str] = {}
    clock = 0
    current = "main"

    def rev(revspec: str) -> str:
        return _git(dest, ["rev-parse", revspec], _env(clock, "sam")).stdout.decode().strip()

    for step in script.steps:
        clock += 1
        env = _env(clock, step.author)

        if step.kind == "branch":
            at = str(step.extra["at"])
            _git(dest, ["branch", "-f", step.branch, labels.get(at, at)], env)
            continue

        if step.kind == "checkout":
            _git(dest, ["checkout", "-q", step.branch], env)
            current = step.branch
            continue

        if step.kind == "commit":
            if current != step.branch:
                _git(dest, ["checkout", "-q", step.branch], env)
                current = step.branch
            _apply(dest, step.ops)
            _git(dest, ["add", "-A"], env)
            _git(dest, ["commit", "-q", "--allow-empty", "-m", step.message], env)
            labels[str(step.label)] = rev(step.branch)
            continue

        if step.kind == "merge":
            into = str(step.extra["into"])
            from_ref = str(step.extra["from_ref"])
            if current != into:
                _git(dest, ["checkout", "-q", into], env)
                current = into
            args = ["merge", "--no-ff", "-q"]
            if step.extra.get("option"):
                args.append(f"-X{step.extra['option']}")
            args += ["-m", step.message, labels.get(from_ref, from_ref)]
            _git(dest, args, env, check=False)
            conflicted = _git(dest, ["diff", "--name-only", "--diff-filter=U"], env
                              ).stdout.decode().split()
            resolve = step.extra.get("resolve") or {}
            for path in conflicted:
                (dest / path).write_text(str(resolve[path]))
                _git(dest, ["add", "--", path], env)
            unhandled = sorted(set(conflicted) - set(resolve))
            if unhandled:
                raise RuntimeError(f"merge of {from_ref} conflicted on {unhandled}, which the "
                                   f"spec did not author a resolution for")
            if _git(dest, ["status", "--porcelain"], env).stdout.decode().strip():
                _git(dest, ["commit", "-q", "--no-edit", "-m", step.message], env)
            labels[str(step.label)] = rev("HEAD")
            continue

        if step.kind == "evil_merge":
            into, from_ref = str(step.extra["into"]), str(step.extra["from_ref"])
            if current != into:
                _git(dest, ["checkout", "-q", into], env)
                current = into
            _apply(dest, step.ops)
            _git(dest, ["add", "-A"], env)
            tree = _git(dest, ["write-tree"], env).stdout.decode().strip()
            commit = _git(dest, ["commit-tree", tree, "-p", labels.get(into, into), "-p",
                                 labels.get(from_ref, from_ref), "-m", step.message],
                          env).stdout.decode().strip()
            _git(dest, ["reset", "-q", "--hard", commit], env)
            labels[str(step.label)] = commit
            continue

        if step.kind == "squash":
            branch, onto = step.branch, str(step.extra["onto"])
            tip = labels[branch] if branch in labels else _git(dest, ["rev-parse", branch], env
                                                              ).stdout.decode().strip()
            _git(dest, ["checkout", "-q", onto], env)
            current = onto
            _git(dest, ["merge", "--squash", "-q", tip], env, check=False)
            if _git(dest, ["status", "--porcelain"], env).stdout.decode().strip():
                _git(dest, ["add", "-A"], env)
                _git(dest, ["commit", "-q", "-m", f"{branch}: squashed into one commit"], env)
            _git(dest, ["branch", "-f", branch, "HEAD"], env)
            labels[str(step.label)] = rev("HEAD")
            continue

        if step.kind == "rebase":
            branch, onto = step.branch, str(step.extra["onto"])
            _git(dest, ["checkout", "-q", branch], env)
            result = _git(dest, ["rebase", labels.get(onto, onto)], env, check=False)
            if result.returncode != 0:
                conflicted = _git(dest, ["diff", "--name-only", "--diff-filter=U"], env
                                  ).stdout.decode().split()
                resolve = step.extra.get("resolve") or {}
                if not conflicted or set(conflicted) - set(resolve):
                    _git(dest, ["rebase", "--abort"], env, check=False)
                    raise RuntimeError(
                        f"rebase of {branch} onto {onto} failed on {conflicted}, which the spec did "
                        f"not author a resolution for: {result.stderr.decode()[:300]}")
                for path in conflicted:
                    (dest / path).write_text(str(resolve[path]))
                    _git(dest, ["add", "--", path], env)
                _git(dest, ["-c", "core.editor=true", "rebase", "--continue"], env)
            labels[str(step.label)] = rev("HEAD")
            current = branch
            continue

        raise ValueError(f"unknown step {step.kind}")
    return labels


# -- reading a built repository back out ---------------------------------------------------------

def log(repo: Path, revspec: str = "HEAD") -> list[dict]:
    """Commits reachable from `revspec`, oldest first — the shape the ingest walk sees."""
    raw = _git(repo, ["log", "--reverse", "--format=%x00%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%B",
                      revspec], _env(0, "sam")).stdout.decode()
    out: list[dict] = []
    for chunk in raw.split("\x00"):
        chunk = chunk.strip("\n")
        if not chunk:
            continue
        sha, parents, name, email, when, message = chunk.split("\x1f", 5)
        out.append({"sha": sha, "parents": parents.split() if parents else [],
                    "author": name, "email": email, "when": int(when), "message": message})
    return out


def ls_tree(repo: Path, revspec: str) -> dict[str, str]:
    """`{path: blob oid}` at `revspec`."""
    listing = _git(repo, ["ls-tree", "-r", "--format=%(objectname) %(path)", revspec],
                   _env(0, "sam")).stdout.decode()
    out: dict[str, str] = {}
    for line in listing.splitlines():
        if line:
            oid, path = line.split(" ", 1)
            out[path] = oid
    return out


def blob(repo: Path, oid: str) -> bytes:
    return _git(repo, ["cat-file", "blob", oid], _env(0, "sam")).stdout


def blobs(repo: Path, oids: list[str]) -> dict[str, bytes]:
    """Read many blobs in one `cat-file --batch` pass — the shape `e1.4`'s cache reads in."""
    if not oids:
        return {}
    uniq = list(dict.fromkeys(oids))
    proc = subprocess.run(["git", "cat-file", "--batch"], cwd=repo,
                          input=("\n".join(uniq) + "\n").encode(), capture_output=True,
                          env=_env(0, "sam"), check=True).stdout
    out: dict[str, bytes] = {}
    index = 0
    for oid in uniq:
        end = proc.index(b"\n", index)
        header = proc[index:end].decode()
        parts = header.split()
        if len(parts) < 3 or parts[1] != "blob":
            raise RuntimeError(f"cat-file did not return a blob for {oid}: {header!r}")
        size = int(parts[2])
        out[oid] = proc[end + 1:end + 1 + size]
        index = end + 1 + size + 1
    return out


def changed(repo: Path, base: str, head: str) -> tuple[list[str], list[str]]:
    """`(removed, added)` paths between two revisions with rename detection forced **off**.

    This is the candidate set every matcher in `detect.py` receives: the honest raw pair of trees, the
    same two rows `e1.2`'s spine would store with `rename.threshold` high enough never to match. A
    matcher may widen it (`copies_survive` needs sources that were never deleted) but never narrow it.
    """
    raw = _git(repo, ["diff", "--no-renames", "--name-status", base, head],
               _env(0, "sam")).stdout.decode()
    removed: list[str] = []
    added: list[str] = []
    for line in raw.splitlines():
        if not line:
            continue
        status, path = line.split("\t", 1)
        kind = status[0]
        if kind == "D":
            removed.append(path)
        elif kind == "A":
            added.append(path)
        elif kind in ("R", "C"):  # impossible with --no-renames, and that is the point
            old, new = path.split("\t")
            removed.append(old)
            added.append(new)
    return removed, added
