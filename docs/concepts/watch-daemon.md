# The watch daemon: one writer, and everyone else a client

- **Status**: shipped (`e6.3`, **reco-kvk**). Designed in
  [ADR-004 §7](../adr/0004-storage.md#7-concurrency-and-the-single-writer); the budget it keeps is
  [ADR-004 §11](../adr/0004-storage.md) and the stream it tails is
  [ADR-005 §6](../adr/0005-harness-contract.md).
- **In code**: the lease is `crates/fathom-store/src/lease.rs`; the loop, the queue, the budget and the
  endpoint are `crates/fathom-ingest/src/daemon/`; the verbs, the worker and the status report are
  `crates/fathom-cli/src/daemon.rs`. The tests that prove the four acceptance criteria are
  `crates/fathom-ingest/tests/daemon.rs` (the loop, with a scripted worker) and
  `crates/fathom-cli/tests/daemon.rs` (a real binary, a real socket, a real repository).
- **Read this if**: you are running Fathom beside a coding agent and several things are asking for the index
  at once, or you are the person who has to say why the daemon was using a core at 3 a.m.

Three TUIs are open, a CLI just ran `fathom index`, and the harness has appended nine events. That is four
things that want the repository indexed, and before this page there were four processes ready to each do the
whole job. The daemon is the answer to that: **one process per repository** takes the writer lease, watches,
tails and indexes, and everybody else is a client that asks it for a pass and reads what it reports.

The rule underneath is ADR-004 §7.1's:

> **Who writes is a fact about a lock, not a fact about which binary you ran.**

## 1. Starting one, and being told there is one already

```console
$ fathom daemon start                 # detached; the parent waits until the socket answers
daemon: running (pid 412, epoch 1, endpoint .fathom/daemon.sock, cpu budget 25%)
$ fathom daemon start                 # a second one, in another terminal
cannot take the writer lease: pid 412 (fathom 0.1.0-alpha.1) holds it, started 4 minutes ago — \
this platform releases a dead holder's lease with the kernel, so nothing is stale
```

The second command is the interesting one. It did not start a daemon, did not fail mysteriously, and did not
guess that the first one had died: it read `.fathom/daemon.lock`, checked the holder against four independent
probes, and named it. Those probes are, in order:

| Probe | What it rules out |
|---|---|
| the `flock(2)` on the lease file is still held | everything about a process that exited on any OS |
| the pid in the record still exists | an exit that left the file behind, on a platform where that is the only evidence |
| the record's `boot_id` is this boot | a pid that has been recycled by a *new* process after a reboot — the case ADR-004 §7.1 says a pid check alone cannot see |
| the record's process start time still matches that pid | a pid recycled *within* one boot, which the boot-id probe cannot see |
| the heartbeat is younger than `lease.stale_after_s` | a process alive but wedged, which is the last case left and is deliberately the least trusted signal |

A lease that fails any probe is **broken with a sentence**, not silently: the notice says whose claim it was,
what made it stale, and what the new epoch is. Epochs only ever go up, which is what makes a zombie writer's
rows recognisable ([ADR-004 §6.3](../adr/0004-storage.md)).

`fathom daemon start --foreground` is the same daemon in the current process, for systemd, a container `init`
and a CI step: stdout stays attached, and SIGTERM leaves by the same path `fathom daemon stop` uses. Without
`--foreground` the parent re-executes itself detached, waits for the socket to answer, and prints the health
document if you asked with `--json` — so the daemon's lease is its own and not a hand-me-down.

## 2. Where the socket is

Endpoint resolution is one function (`fathom_ingest::daemon::socket::resolve`), used identically by the daemon
and by every client, because two verbs disagreeing about where the daemon is would be a bug report nobody can
reproduce.

| Order | Endpoint | Why |
|---|---|---|
| 1 | `--socket PATH` | the supervisor's or the test's choice, honoured exactly |
| 2 | `FATHOM_DAEMON_SOCKET` | the same, for a whole environment |
| 3 | `<repo>/.fathom/daemon.sock` | the repository-local default: it travels with the repository, so two checkouts do not share a daemon |
| 4 | `$XDG_RUNTIME_DIR/fathom-<digest>.sock`, or `$TMPDIR` under it | the fallback when the repository path will not fit in `sockaddr_un` (108 bytes on Linux, 104 on macOS) |
| — | `\\.\pipe\fathom-<digest>` | Windows: a pipe name is always short, so there is nothing to fall back from. **Binding a named pipe needs FFI, which this workspace forbids; on Windows the daemon runs with the lease and the heartbeat and no endpoint**, and says so in `daemon.socket.reason` rather than pretending |

A socket file whose listener died is *stale*, and staleness is decided by asking it — `listen(2)` on the path
tells you whether anyone is home, which is exactly the question and does not need a heuristic. Stale is
replaced; live is `AddrInUse`, which is the truth.

## 3. The protocol, which is one line each way

`fathom.daemon/1`. One request per connection: connect, write one JSON line, read one line, done. Versioned
first field, and a daemon that does not speak your version refuses you with `protocol_mismatch` rather than
answering in a shape you will misread.

| Request | Does |
|---|---|
| `{"method":"hello"}` | capabilities, protocol version, build — read by `fathom doctor` |
| `{"method":"status"}` | the health document below |
| `{"method":"index","origin":"fathom tui","wait_ms":20000}` | do a pass; `wait_ms: 0` means "queue it and answer now", anything else means "hold this connection until a pass covers it" |
| `{"method":"stop","reason":"..."}` | finish the pass, checkpoint, release, exit 0 |

`origin` is not decoration. The queue coalesces **per asker**, so three TUIs asking for the same thing are one
request, `fathom status` can say who has been asking, and a flood of distinct askers hits the depth bound and
is *refused with a reason* instead of growing memory without limit.

```console
$ fathom status --json | jq '.daemon.queue, .daemon.passes.requests_by_origin'
{"depth":0,"bound":64,"pushed":6,"coalesced":2,"shed":0,"served":6,"high_water":2,"oldest_ms":null}
{"fathom index":2,"fathom tui":3,"fathom watch":1}
```

That pair of documents is how the ticket's first criterion is asserted rather than asserted-to: `pushed` counts
every request, `coalesced` counts the ones that folded into an identical one already waiting, and
`passes.count` did not move once per client. The counter test is
`three_tuis_and_a_cli_make_one_indexing_pass` in `crates/fathom-ingest/tests/daemon.rs`, and it fails if the
queue is asked to coalesce per *method* instead of per asker — which is the shape that looks identical in a
demo and makes the depth bound, the high-water mark and the shed counter into decoration.

A client that asked to be waited for is parked, and parked means the connection is kept open and answered by
whichever pass covers it — including a pass that started before the ask. Parking is why `fathom index` can
defer to a daemon and still print what the pass did:

```console
$ fathom index --json | jq '{contract, daemon}'
{"contract":"fathom.cli.index.deferred/1","daemon":{"settled":true,"commits":1,"units":4,"coalesced":false}}
```

`fathom watch --json` says the same thing in one field: `indexer` is `"this process"` or `"daemon"`, because
two runs whose counters look identical are not the same event when one of them held the writer lease and the
other asked for someone else to.

It is a **different contract** from `fathom.cli.index/1` on purpose: the timings, the cache table and the
segmentation in a run report belong to a run this process did not perform, and inventing them for somebody
else's pass is the kind of near-miss number this project treats as a bug.

## 4. What it costs, and how it knows

One thread, deliberately: two indexing threads double peak CPU without finishing a pass sooner.

```console
$ fathom status | sed -n '1,6p'
daemon: running — pid 412, writer epoch 1, up 6 m, protocol fathom.daemon/1
cpu: 3.1% of one core measured over 1 s (budget 25.0%, throttled 0 window(s), slept 0 ms)
requests: 6 asked, 2 coalesced, 6 served, 0 shed (depth 0 of 64, high water 2)
harness stream: offset 4100 of 9200, 5100 bytes unread, 5 read(s), 0 rotation(s)
back-pressure: cap 1048576 bytes/pass, 0 saturated pass(es)
index passes: 5 run, 6 client(s) served, 0 refused; by origin fathom index=2, fathom tui=3
```

The budget is **measured and enforced**, not configured and hoped for:

- the measurement is user+system CPU from `/proc/self/stat` over a rolling window, because wall-clock busy
  time includes time this process was descheduled and would let the *kernel's* decisions be reported as
  Fathom's;
- where that clock is not available the number is still printed, from busy wall time, with
  `cpu.source: "estimate"` and a note in the human output saying it is an upper bound;
- over the share, the loop owes time and pays it with sleep before taking more work — so `cpu.slept_ms` is
  the visible price of the throttling, and `cpu.over_windows` counts the windows that triggered it;
- a client heard recently shrinks the work slice to `yield_slice` (50 units, against 2 000 when nothing is
  waiting), because the ticket's criterion is that *the TUI stays responsive*, not that the daemon stays under
  budget while a human watches a spinner.

The harness stream is capped per pass too (`hook-bytes`, default 1 MiB), and — the part that matters — the cap
stops at a **whole line boundary**, because a torn line is not a readable event and re-reading a torn tail is
a parse error waiting to happen. What the cap cannot read in one pass is reported as `hooks.lag_bytes`: back
pressure that is *visible*, in the same shape as the queue's `shed`, rather than an event stream quietly
dropping lines (ADR-005 §6.5).

## 5. Leaving

There is one exit path, and everything else is a way into it:

| Event | Path |
|---|---|
| `fathom daemon stop` | a `stop` request over the socket |
| Ctrl-C | `SIGINT` |
| `kill`, systemd, container stop | `SIGTERM` |
| `--deadline`, `--max-cycles` | the supervisor's own limits |
| a worker error that cannot be recovered | the fatal path, which still runs the same finish |

The finish is: let the current pass end, checkpoint the store, blank the lease record, remove the socket. A
kill -9 skips all of that, and the *next* start says so out loud — breaks the stale claim, bumps the epoch,
replaces the dead socket — which is what "a crash leaves the store consistent and a restart resumes" means
operationally: the WAL is already durable at each commit boundary, and the wreckage left behind is
self-describing rather than mysterious.

## 6. `fathom status`, and the four sources of an answer

`fathom status --json` is a published contract
([`schemas/status.schema.json`](../../schemas/status.schema.json)) and it carries, as the ticket asked, pid,
uptime, the hook offset, the queue depth and the cache statistics. What the page has not said yet is that a
third of the time there is no daemon to ask, and the verb still has to answer. It does so in a stated order,
and it prints which one it used:

```console
$ fathom status --json | jq '.daemon.source, .daemon.running, .exit_code'
"none" false 0
```

| `daemon.source` | What it means |
|---|---|
| `daemon` | a live process answered on its socket; the document is its self-report |
| `lease` | nothing answered, but the writer lease is live or recent: a writer is running without IPC, or died and left a claim |
| `heartbeat` | neither, but `fathom watch` left its heartbeat file — a dead process's last note, and labelled as one |
| `none` | nothing is watching, and the report says so with the endpoint it would have used |

Every demotion between those is a finding with a remedy, so `source: "lease"` and `source: "heartbeat"` are not
a silent substitution of a dead process's words for a live one's. A stale lease is exit `12`
(`INDEX_DEGRADED`), because a writer that vanished mid-pass is exactly what that code is for; an absent daemon
is exit `0`, because a repository that is merely idle is not a broken one. `daemon.running` is the field a
script should branch on — one code cannot mean both "fine" and "nobody is home".

## 7. Three places the implementation is not what ADR-004 §7 literally says

The ADR is the design; these are the three points where building it found a better answer, written down rather
than left as a mystery for the next reader of §7.

1. **Releasing blanks the file; it does not unlink it.** §7.1's step 4 says "release the lock and delete
   `.fathom/daemon.lock`". Deleting has a race a blank does not: a client that opened the file a moment before
   the unlink can then `flock` an inode no reader can see, and write a claim nobody will ever read. So
   `release` truncates, and `Lease::state` reads an empty file as **free** — the same fact a missing file
   states. A half-written record is not empty and stays `unreadable`, because that is the case where guessing
   "free" could seat two writers in one repository.
2. **The queue coalesces per asker, not per method.** The daemon has one kind of request, so a queue keyed on
   the method would hold exactly one entry forever and the depth bound, the high-water mark and the shed
   counter would all be decoration. Keyed on `origin`, three TUIs are still one request, a TUI that polls does
   not grow the queue, and the bound finally means "distinct askers".
3. **A broken lease is a notice and a log line, not a `store_event` row.** §7.1 asks for a `store_event` line;
   the table does not exist in the schema yet, so there is nothing to insert. Until it does the takeover is
   printed at start-up and carried in `daemon.notices`, which is the difference between *unstated* and *not
   yet wired*, and the two should not be confused.

## 8. Deliberately not here

- **A Windows named pipe.** Resolving one is easy; binding one is `CreateNamedPipeW`, which is FFI, and this
  workspace forbids `unsafe_code` rather than carrying an exception per crate. Windows gets the lease, the
  heartbeat and no endpoint, and says so. The clean fix is `mio` or `tokio` in `fathom-store`, which is a
  dependency decision bigger than this ticket.
- **An authentication layer.** The socket is a local file with the repository's own permissions; a peer that
  can open it can already read the repository and write to its working tree, so authenticating it would be
  theatre. What the endpoint *does* do is refuse anything that is not a well-formed request from a known
  protocol version, because the interesting failure is a confused client, not an attacker.
- **A `store_event` row when a stale lease is broken.** ADR-004 §7.1 asks for a `store_event` line and a log
  line. The log line and the health notice are here; the table does not exist yet, so there is nothing to
  insert into. Until it does, the takeover is printed and carried in `daemon.notices`.
- **Distributing the indexing.** The daemon is one thread on one machine. The bounded LRU of materialised
  revisions the architecture panes need (spike s3) belongs beside these budgets when the TUI grows into it;
  nothing here pretends to be a build farm.
