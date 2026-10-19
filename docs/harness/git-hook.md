# Recipe: the git `post-commit` fallback — the harness you can always install

- **Audience**: a repository with no agent in it, or an agent that will not run hooks, or a CI job
  that commits. This is the adapter that makes "no harness configured" a coverage hole you can see
  instead of one you assume.
- **Adapter**: [`adapters/fathom-post-commit`](../../adapters/fathom-post-commit) — one file, `/bin/sh`,
  POSIX. No Python, no Node, no JSON library, no network, nothing to build. It works on dash, bash,
  busybox ash and git bash on Windows.

## 1. Install it

```bash
cp adapters/fathom-post-commit .git/hooks/post-commit
chmod +x .git/hooks/post-commit
```

That is the whole install. If a `post-commit` hook already exists, do not overwrite it — add this line
to the end of it instead:

```sh
"$(git rev-parse --show-toplevel)/.git/hooks/fathom-post-commit" || true
```

and put the adapter at that path. (The Python reference installer
`src/recommender/hooks/generic_jsonl.py:install_git_hook` refuses to clobber a hook that is not its
own, which is the behaviour worth copying by hand when you are the one editing someone's hook.)

## 2. Commit, then look

```bash
git commit -m "anything"
fathom hooks --check
```

Each path the commit touched becomes one `file_edit` event, with the counts git gave it:

```
.fathom/events.jsonl — offset 1540 of 1540 bytes (0 bytes, 0 lines behind)
  events read: 3   last seq 3 (file at 3)   types: file_edit 3
```

and `fathom index` then has harness evidence for a history that had none. What the record claims is
bounded by what a commit can confess:

```json
{"cwd": "/srv/app", "fathom_event": "file_edit", "git_branch": "main", "harness": "git_post_commit",
 "payload": {"author_email": "dev@example.com", "author_name": "A Dev", "binary": false,
             "change": "update", "commit": "752450465da1…", "lines_added": 12, "lines_removed": 3,
             "provenance": "asserted", "subject": "anything"},
 "path": "src/main.rs", "schema_version": 1, "seq": 3, "session_id": "git:752450465da1…",
 "source": "file_edit", "ts": "2026-10-19T09:30:02.117Z", "event_id": "e3"}
```

`"provenance": "asserted"` is the important field. ADR-001's amendment grades asserted evidence
instead of hiding it: git says a path changed, so this is a fact about a commit and **not** evidence
that an agent was in a turn. There is no `turn_id` and no `turn_start`, so the reader reports the turn
as `inferred` — accurately, because the transport never stated where the turn began:

```
  note: 3 turn(s) were inferred from turn-scoped events with no turn_start: the adapter never said
  where a turn began
```

## 3. Why it cannot cost you a commit

The acceptance criterion is "never blocks a commit on failure", and it is structural:

* the script's first statement is `trap 'exit 0' EXIT`, so its exit status is 0 whatever happens
  inside, including a signal;
* no command's status is examined, `main`'s stdout and stderr go to `.fathom/post-commit.log` (or to
  `/dev/null` when that is not writable), and its stdin is closed rather than read;
* the lock on the events file is waited on **bounded** (≈2 s with a stale-lock takeover) and giving up
  is a logged, non-event — a stuck writer costs a commit's worth of events and nothing else;
* work is bounded: two git plumbing calls, at most 500 paths per commit, a 1 MiB log;
* it never calls the network, never prompts, and never runs anything the repository told it to run.

The three failure modes `tests/test_adapter_contract.py` actually exercises: `.fathom` chmod'd to
`r-x`, `events.jsonl` chmod'd to read-only, and `PATH` pointing at a directory with no `git` in it. A
commit in each of those still exits 0.

## 4. What a `post-commit` hook cannot know, and what that costs

| It cannot know | So the record says | Cost |
| --- | --- | --- |
| which turn, or which prompt | no `turn_id`; `session_id` is `git:<commit>` | turns come out `inferred`, and attribution's time-window rule has nothing to work with |
| whether the change was one agent turn or four humans | `provenance: "asserted"` | episode boundaries still come from ADR-003's git-side rungs, at their own confidence |
| anything before the hook was installed | — | backfill with `python3 -m recommender.harness …` or the reference `replay_git_history`, which marks every backfilled record asserted too |
| a merge commit's content | merges are skipped, with a line in the log | correct, not missing: a merge is a graph boundary, and segmentation's rung 4 names it better than a hook can |

Paths that would damage the stream are skipped and named in `.fathom/post-commit.log` — a path with a
newline in it (the one input a newline-framed parser cannot read, so the hook rejoins the split record
and then refuses it), and the stream's own `.fathom/` directory, because a recorder that records the
file it writes makes every commit about the recorder.

## 5. Uninstall

```bash
rm .git/hooks/post-commit
```

The events already written stay; they are asserted evidence about real commits, and the reader is
content with a stream that stopped.
