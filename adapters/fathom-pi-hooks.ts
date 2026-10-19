/**
 * fathom-pi-hooks — the pi hook adapter: pi's session/turn/tool events, stamped into hooks v1.
 *
 * `e6.2` (`reco-7go`), the first of the three reference adapters in
 * [docs/harness/pi.md](../docs/harness/pi.md). It is a pi **extension**: pi loads it, calls its
 * handlers with pi's own event objects, and it appends hooks v1 records (ADR-005 §2) to
 * `.fathom/events.jsonl`. Nothing else in Fathom is involved — this file has no Fathom dependency,
 * so it is copied to a machine that has pi and a repository and nothing else.
 *
 * Install (one line, and it survives a pi upgrade because there is nothing to upgrade):
 *
 *     mkdir -p .pi/extensions && cp adapters/fathom-pi-hooks.ts .pi/extensions/
 *     pi            # or: pi -e adapters/fathom-pi-hooks.ts
 *
 * What it claims, and how far that claim reaches: **[observed]**. Every event name and field below
 * was read out of the installed pi package (`dist/core/extensions/types.d.ts`) and then *run* —
 * `docs/harness/verification.md` §1 records a live session's output, produced by this file, being
 * read back by `fathom hooks --check`. That is the difference between this adapter and the two
 * whose vendors are not installed here: pi is the harness this repository is developed in.
 *
 * The mapping, and the two judgement calls in it:
 *
 * | pi event | `source` | `fathom_event` | why |
 * |---|---|---|---|
 * | `session_start` | `session_start` | `session_start` | pi's name is already Fathom's canonical one |
 * | `turn_start` | `turn_start` | `turn_start` | pi's turn is the ADR's turn: one prompt, its tool calls, its reply |
 * | `tool_execution_start` | `tool_execution_start` | `tool_call` | §2.1 |
 * | `tool_execution_end` | `tool_execution_end` | `tool_result` | §2.1, with `ok`/`is_error`/`duration_ms` |
 * | `tool_execution_end` on `write`/`edit` | `file_edit` | `file_edit` | **derived**, and stamped `derived` |
 * | `turn_end` | `turn_end` | `turn_end` | with `outcome` and a digest of the assistant's reply |
 * | `session_shutdown` | `session_shutdown` | `session_end` | **not `agent_settled`**: a run settles many times a session; only shutdown ends a session |
 * | `model_select`, `session_compact`, `thinking_level_select` | as pi spells them | *(none)* | v1 has no event for these: counted, not dropped (§2) |
 *
 * Two things it deliberately does not do:
 *
 * * **It never copies a file body, a diff or a command's output.** pi hands the handler the whole
 *   tool result, which for `read` is a file's contents. Those are digested (`sha256` + `chars`),
 *   never stored — ADR-005 §4.3 and §6.5, and `docs/harness/verification.md` §1.4 proves it with a
 *   canary rather than with a comment.
 * * **It never throws into the agent.** Every handler swallows its own failure: an extension whose
 *   `tool_call` handler throws *blocks the tool*, so an observability adapter that cannot write a
 *   line must be the quietest thing in the process.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { createHash } from "node:crypto";
import {
	appendFileSync,
	existsSync,
	mkdirSync,
	openSync,
	readFileSync,
	readSync,
	rmSync,
	statSync,
	writeFileSync,
	writeSync,
	fsyncSync,
	closeSync,
} from "node:fs";
import { dirname, join, relative, isAbsolute } from "node:path";
import { spawnSync } from "node:child_process";

const SCHEMA_VERSION = 1;
const MAX_LINE_BYTES = 4 * 1024 * 1024; // ADR-005 §3.6, the same number as the Rust reader's
const LOCK_WAIT_MS = 2000;
const LOCK_STALE_MS = 5000;
const MAX_STRING = 2048;
const MAX_ITEMS = 32;
const MAX_DEPTH = 6;

/** pi's own name for this adapter, published in the `harness` field. */
const HARNESS = "pi";
const WIRE_PROTOCOL = "pi-hooks/1";

/** Keys whose value is *content*: a digest proves the bytes were seen without storing them (§4.3). */
const CONTENT_KEYS = new Set([
	"content",
	"text",
	"body",
	"diff",
	"patch",
	"stdout",
	"stderr",
	"output",
	"oldString",
	"newString",
	"old_string",
	"new_string",
	"file_text",
	"contents",
	"data",
	"messages",
]);
const SECRET_KEY = /(secret|token|password|passwd|api[-_]?key|authorization|cookie|credential|private[-_]?key)$/i;
const SECRET_VALUE =
	/(-----BEGIN [A-Z ]*PRIVATE KEY-----|AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[baprs]-[A-Za-z0-9-]{10,})/;

/** pi tools that edit files: their result is also a `file_edit` (§2.1's derivation). */
const EDIT_TOOLS = new Set(["write", "edit"]);
const PATH_KEYS = ["path", "file_path", "notebook_path"];

// --------------------------------------------------------------------------- the write side

interface HookRecord {
	[key: string]: unknown;
}

let eventsFile: string | undefined;
let sessionPointer: string | undefined;

/** ADR-005 §3.1's order, minus `fathom.toml`: see the note in `adapters/fathom-hook.py`. */
function eventsPath(cwd: string): string {
	for (const name of ["FATHOM_EVENTS_FILE", "RECO_EVENTS_FILE"]) {
		const value = process.env[name];
		if (value) return value;
	}
	for (const name of ["FATHOM_EVENTS_DIR", "RECO_EVENTS_DIR"]) {
		const value = process.env[name];
		if (value) return join(value, "events.jsonl");
	}
	return join(cwd, ".fathom", "events.jsonl");
}

function sleep(ms: number): void {
	// Synchronous on purpose: the handlers are awaited by pi and a timer would outlive the process.
	Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/**
 * Run `body` holding the append lock, which is what makes "`seq` is allocated by the file" (§3.3)
 * true when two sessions write at once. A `mkdir` lock, because Node has no `flock(2)` and every
 * platform pi runs on has atomic `mkdir`; a lock older than `LOCK_STALE_MS` belongs to a writer
 * that died holding it and is taken over.
 */
function withLock(dir: string, body: () => void): void {
	const lock = join(dir, ".events.lock");
	const deadline = Date.now() + LOCK_WAIT_MS;
	for (;;) {
		try {
			mkdirSync(lock);
			break;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
			try {
				if (Date.now() - statSync(lock).mtimeMs > LOCK_STALE_MS) rmSync(lock, { recursive: true });
			} catch {
				/* it went away; retry */
			}
			if (Date.now() > deadline) throw new Error(`fathom: another writer holds ${lock}`);
			sleep(20);
		}
	}
	try {
		body();
	} finally {
		try {
			rmSync(lock, { recursive: true });
		} catch {
			/* already gone */
		}
	}
}

/** The last `seq` in the file: the tail first, the whole file only if the tail could not answer. */
function lastSeq(path: string): number {
	let size: number;
	try {
		size = statSync(path).size;
	} catch {
		return 0;
	}
	if (!size) return 0;
	const window = Math.min(size, 65536);
	const buffer = Buffer.alloc(window);
	const fd = openSync(path, "r");
	try {
		readSync(fd, buffer, 0, window, size - window);
	} finally {
		closeSync(fd);
	}
	let text = buffer.toString("utf8");
	if (window < size) text = text.slice(text.indexOf("\n") + 1);
	let max = 0;
	for (const line of text.split("\n")) {
		const match = /"seq"\s*:\s*(\d+)/.exec(line);
		if (match) max = Math.max(max, Number.parseInt(match[1], 10));
	}
	if (max > 0 || size <= 65536) return max;
	// The tail held no sequence number at all (a freshly truncated file, a run of damaged lines), so
	// ask the whole file before concluding the stream starts at 1.
	const whole = readFileSync(path, "utf8");
	for (const line of whole.split("\n")) {
		const match = /"seq"\s*:\s*(\d+)/.exec(line);
		if (match) max = Math.max(max, Number.parseInt(match[1], 10));
	}
	return max;
}

/**
 * Close a half-written line before appending, so two writers never share one.
 *
 * A writer that died mid-line leaves the file without a trailing newline, and the next append would
 * extend that half line into one unreadable record. Appending the missing `\n` repairs nothing that
 * is on disk (ADR-005 §3.6's "the file is never rewritten to remove damage" is about not deleting
 * evidence): the damaged line becomes a complete line the reader counts in `bad_lines` and moves
 * past. All three adapters do this, because one writer that forgets it gives the defect to everyone.
 */
function terminateTornTail(path: string): void {
	let size: number;
	try {
		size = statSync(path).size;
	} catch {
		return;
	}
	if (!size) return;
	const probe = Buffer.alloc(1);
	const fd = openSync(path, "r");
	try {
		readSync(fd, probe, 0, 1, size - 1);
	} finally {
		closeSync(fd);
	}
	if (probe[0] !== 0x0a) appendFileSync(path, "\n");
}

/** Append records with a `seq` each, allocated under the lock (§3.3, §3.5). */
function append(path: string, records: HookRecord[]): void {
	if (!records.length) return;
	mkdirSync(dirname(path), { recursive: true });
	withLock(dirname(path), () => {
		terminateTornTail(path);
		let seq = lastSeq(path);
		const lines: string[] = [];
		for (const record of records) {
			seq += 1;
			const line = JSON.stringify(Object.fromEntries(Object.entries({ ...record, seq, event_id: `e${seq}` }).sort()));
			if (Buffer.byteLength(line, "utf8") > MAX_LINE_BYTES) {
				seq -= 1; // refused whole, and it does not burn a sequence number (§3.3)
				continue;
			}
			lines.push(line);
		}
		if (!lines.length) return;
		// "a" is O_APPEND: the kernel seeks to the end for every write, which is what keeps a
		// line from interleaving with another writer's (ADR-005 §3.5). fsync is the durability
		// half — an evidence trail a power cut can reorder is not evidence.
		const fd = openSync(path, "a");
		try {
			writeSync(fd, Buffer.from(`${lines.join("\n")}\n`, "utf8"));
			fsyncSync(fd);
		} finally {
			closeSync(fd);
		}
	});
}

// --------------------------------------------------------------------------- redaction

function sha256(value: unknown): string {
	return createHash("sha256").update(stable(value), "utf8").digest("hex");
}

function stable(value: unknown): string {
	if (value === null || typeof value !== "object") return JSON.stringify(value) ?? "null";
	if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
	return `{${Object.keys(value as object)
		.sort()
		.map((key) => `${JSON.stringify(key)}:${stable((value as Record<string, unknown>)[key])}`)
		.join(",")}}`;
}

/** Bound and redact a value before it is stored (ADR-005 §4.3, §6 rule 4, §6 rule 5). */
function scrub(value: unknown, depth = 0): unknown {
	if (depth >= MAX_DEPTH) return "[TRUNCATED depth]";
	if (typeof value === "string") {
		if (SECRET_VALUE.test(value)) return "[REDACTED:value-shape]";
		return value.length > MAX_STRING ? `${value.slice(0, MAX_STRING)}…[truncated ${value.length - MAX_STRING} chars]` : value;
	}
	if (value === null || typeof value === "boolean" || typeof value === "number" || value === undefined) return value ?? null;
	if (Array.isArray(value)) {
		const out = value.slice(0, MAX_ITEMS).map((item) => scrub(item, depth + 1));
		if (value.length > MAX_ITEMS) out.push(`[${value.length - MAX_ITEMS} more]`);
		return out;
	}
	const out: Record<string, unknown> = {};
	for (const [key, item] of Object.entries(value as Record<string, unknown>).slice(0, MAX_ITEMS)) {
		if (SECRET_KEY.test(key)) out[key] = "[REDACTED:key]";
		else if (CONTENT_KEYS.has(key) && typeof item === "string") {
			out[`${key}_sha256`] = createHash("sha256").update(item, "utf8").digest("hex");
			out[`${key}_chars`] = item.length;
		} else out[key] = scrub(item, depth + 1);
	}
	return out;
}

function now(): string {
	return new Date().toISOString();
}

function gitBranch(cwd: string): string | undefined {
	const result = spawnSync("git", ["-C", cwd, "rev-parse", "--abbrev-ref", "HEAD"], { encoding: "utf8" });
	const branch = result.status === 0 ? result.stdout.trim() : "";
	return branch && branch !== "HEAD" ? branch : undefined;
}

/** Contain the way the reader contains, but do not drop: an escaping path must reach `path_escape`. */
function relativeTo(cwd: string, path: string): string {
	if (!path || !isAbsolute(path)) return path;
	const rel = relative(cwd, path);
	return rel && !rel.startsWith("..") ? rel : path;
}

// --------------------------------------------------------------------------- the extension

export default function (pi: ExtensionAPI): void {
	let cwd = process.cwd();
	let sessionId = "";
	let branch: string | undefined;
	/** The prompt digest of the run in flight, attached to the next `turn_start` (§2.1). */
	let pendingPrompt: { sha256: string; chars: number } | undefined;
	let model: string | undefined;

	const base = (source: string, fathomEvent?: string): HookRecord => {
		const record: HookRecord = {
			cwd,
			harness: HARNESS,
			schema_version: SCHEMA_VERSION,
			source,
			session_id: sessionId,
			ts: now(),
		};
		if (fathomEvent) record.fathom_event = fathomEvent;
		if (branch) record.git_branch = branch;
		return record;
	};

	/** The one line the trailer recipe reads: which turn the agent is in right now. */
	function noteTurn(session: string, turnIndex: number): void {
		if (!sessionPointer) return;
		try {
			mkdirSync(dirname(sessionPointer), { recursive: true });
			writeFileSync(
				sessionPointer,
				JSON.stringify(
					{ session_id: session, turn_index: turnIndex, turn_id: `${session}:${turnIndex}`, harness: HARNESS, updated: now() },
					undefined,
					1,
				) + "\n",
			);
		} catch {
			/* the trailer recipe degrades to `fathom hooks --trailer`, which is the documented fallback */
		}
	}

	function write(records: HookRecord[]): void {
		try {
			append(eventsFile ?? eventsPath(cwd), records);
		} catch (error) {
			// Quiet by design: this runs inside the agent's own process, and an observability
			// adapter that shouts is an adapter the user uninstalls mid-task.
			process.stderr.write(`fathom-pi-hooks: wrote nothing: ${(error as Error).message}\n`);
		}
	}

	pi.on("session_start", (event, ctx) => {
		cwd = ctx.cwd;
		eventsFile = eventsPath(cwd);
		sessionPointer = join(cwd, ".fathom", "harness", "pi-turn.json");
		sessionId = ctx.sessionManager.getSessionId() || process.env.PI_SESSION_ID || "";
		model = ctx.model ? `${ctx.model.provider}/${ctx.model.id}` : undefined;
		branch = gitBranch(cwd);
		write([
			{
				...base("session_start", "session_start"),
				payload: scrub({
					flavour: event.reason,
					session_file: ctx.sessionManager.getSessionFile(),
					model,
					harness_version: process.env.PI_VERSION ?? "unknown",
					wire_protocol: WIRE_PROTOCOL,
				}),
			},
		]);
		return undefined;
	});

	pi.on("before_agent_start", (event) => {
		// pi's run boundary, not a turn boundary: the prompt belongs to the first `turn_start`
		// that follows, which is where §2.1 says a prompt digest is carried.
		const prompt = typeof event.prompt === "string" ? event.prompt : "";
		pendingPrompt = prompt ? { sha256: sha256(prompt), chars: prompt.length } : undefined;
		return undefined;
	});

	pi.on("turn_start", (event) => {
		const payload: Record<string, unknown> = { vendor_turn_index: event.turnIndex };
		if (pendingPrompt) {
			payload.prompt_sha256 = pendingPrompt.sha256;
			payload.prompt_chars = pendingPrompt.chars;
			pendingPrompt = undefined;
		}
		// pi's own turnIndex is 0-based and per-process; the turn_id the pipeline uses is
		// allocated by the reader at ingest (ADR-005 §3.3), so it is not written here.
		write([{ ...base("turn_start", "turn_start"), payload: scrub(payload) }]);
		noteTurn(sessionId, event.turnIndex + 1);
		return undefined;
	});

	/**
	 * What a call was made *with*, remembered from its start event. `tool_execution_end` carries
	 * `result` but not `args` — pi's own type says so, and a mapping written from memory would have
	 * assumed symmetry and quietly lost every `file_edit` derivation (it did, on the first live run).
	 */
	const calls = new Map<string, { tool: string; path?: string; command?: string }>();

	pi.on("tool_execution_start", (event) => {
		if (event.parentToolCallId) return undefined; // a nested call; the calling tool reports it
		const input = (event.args ?? {}) as Record<string, unknown>;
		const path = PATH_KEYS.map((key) => input[key]).find((value) => typeof value === "string") as
			| string
			| undefined;
		const command = typeof input.command === "string" ? input.command : undefined;
		if (calls.size > 4096) calls.clear(); // bounded: a long session must not grow without limit
		calls.set(event.toolCallId, { tool: event.toolName, path, command });
		const record: HookRecord = {
			...base("tool_execution_start", "tool_call"),
			payload: scrub({ tool_name: event.toolName, tool_use_id: event.toolCallId, tool_input: input }),
			tool_input_sha256: sha256(input),
		};
		if (path) record.path = relativeTo(cwd, path);
		write([record]);
		return undefined;
	});

	const toolRecords = (
		event: { toolName: string; toolCallId: string; result: unknown; durationMs?: number },
		failure: boolean,
	): HookRecord[] => {
		const called = calls.get(event.toolCallId) ?? { tool: event.toolName };
		const result = event.result;
		const output = typeof result === "string" ? { text: result } : (result ?? {});
		const outputDigest = sha256(output);
		const produced: HookRecord[] = [
			{
				...base("tool_execution_end", failure ? "error" : "tool_result"),
				payload: scrub({
					tool_name: event.toolName,
					tool_use_id: event.toolCallId,
					ok: !failure,
					is_error: failure,
					duration_ms: event.durationMs,
					output_sha256: outputDigest,
					// A shell command is the one argument worth its text: `test_run` is derived
					// from it at ingest (ADR-005 §2.2) and a digest answers nothing.
					command: called.command,
				}),
			},
		];
		const path = called.path;
		if (path) produced[0].path = relativeTo(cwd, path);
		if (!failure && EDIT_TOOLS.has(event.toolName) && path) {
			const details = (result as { details?: Record<string, unknown> } | null)?.details ?? {};
			produced.push({
				...base("file_edit", "file_edit"),
				path: relativeTo(cwd, path),
				payload: scrub({
					derived: true,
					derived_by: "tool_execution_end on an editing tool (ADR-005 §2.1)",
					tool: event.toolName,
					change: details.created === true ? "add" : details.deleted === true ? "delete" : "update",
					lines_added: details.added,
					lines_removed: details.removed,
					content_sha256_after: outputDigest,
				}),
			});
		}
		return produced;
	};

	pi.on("tool_execution_end", (event) => {
		if (event.parentToolCallId) return undefined;
		write(toolRecords(event, event.isError === true));
		calls.delete(event.toolCallId);
		return undefined;
	});

	pi.on("turn_end", (event) => {
		const message = (event.message ?? {}) as Record<string, unknown>;
		write([
			{
				...base("turn_end", "turn_end"),
				payload: scrub({
					vendor_turn_index: event.turnIndex,
					outcome: (event as Record<string, unknown>).outcome ?? "completed",
					tool_result_count: Array.isArray(event.toolResults) ? event.toolResults.length : 0,
					last_assistant_message_sha256: sha256(message),
				}),
			},
		]);
		return undefined;
	});

	// Counted, not dropped (§2): v1 has no Fathom event for these, so they arrive with no
	// `fathom_event` and show up in `fathom hooks`' `unprojectable.by_type`.
	for (const name of ["model_select", "thinking_level_select", "session_compact"] as const) {
		pi.on(name, (event) => {
			const { type, ...rest } = event as unknown as Record<string, unknown>;
			write([{ ...base(String(name)), payload: scrub(rest) }]);
			return undefined;
		});
	}

	pi.on("session_shutdown", (event) => {
		write([{ ...base("session_shutdown", "session_end"), payload: scrub({ reason: event.reason }) }]);
		return undefined;
	});

	pi.registerCommand("fathom-trailer", {
		description: "Print the Agent-Session/Agent-Turn trailer pair for the current turn (e6.2)",
		handler: async (_args, ctx) => {
			const turnFile = join(cwd, ".fathom", "harness", "pi-turn.json");
			if (!existsSync(turnFile)) {
				ctx.ui.notify("fathom: no turn recorded yet this session", "warn");
				return;
			}
			const turn = JSON.parse(readFileSync(turnFile, "utf8")) as { session_id: string; turn_id: string };
			ctx.ui.notify(`Agent-Session: ${turn.session_id}\nAgent-Turn: ${turn.turn_id}`, "info");
		},
	});
}
