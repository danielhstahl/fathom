/**
 * fathom-envelope-driver — make `adapters/fathom-hook.py` testable against a live harness.
 *
 * `e6.2` (`reco-7go`). The second reference adapter, `fathom-hook.py`, speaks the envelope a hook
 * command receives on **stdin**: one JSON object per hook event, with `hook_event_name`,
 * `session_id`, `cwd`, `tool_name`, `tool_input`, `tool_response`. That is the shape Claude Code,
 * Gemini CLI and their imitators use, and none of them are installed in the container this was
 * developed in — so the adapter's field spellings are [external] (ADR-005 §1.2), and the honest way
 * to exercise the *transport* is to let the harness that **is** installed stand in for the vendor.
 *
 * This file is that stand-in. It is deliberately small and deliberately not clever: it registers pi
 * handlers, re-spells each event into the envelope with the key names the vendor protocol uses, and
 * spawns `fathom-hook.py` and writes the object to its stdin — which is precisely the boundary a
 * real vendor would cross, subprocess and all.
 *
 *     pi -e adapters/fathom-envelope-driver.ts
 *     FATHOM_HOOK_ADAPTER=/path/to/fathom-hook.py pi -e adapters/fathom-envelope-driver.ts
 *
 * What it proves, stated exactly: that a live agent's session, turns, tool calls and file edits
 * survive the path *vendor envelope → stdin → `fathom-hook.py` → `events.jsonl` → `fathom hooks` →
 * `fathom index` → the TUI*. What it does not prove is that Claude Code spells `file_path` the way
 * this file does; §9's capture is still the only thing that promotes those names, and
 * `tests/traces/README.md` says which of the shipped traces came from a live session and which from
 * a documented shape.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";

function adapterPath(): string {
	if (process.env.FATHOM_HOOK_ADAPTER) return process.env.FATHOM_HOOK_ADAPTER;
	// Loaded by pi from wherever the file sits; `__dirname` is not defined in an ESM-transformed
	// extension, and the two candidate roots are the two places this repo actually keeps it.
	for (const candidate of [join(dirname(__filename), "fathom-hook.py"), "adapters/fathom-hook.py"]) {
		if (existsSync(candidate)) return candidate;
	}
	return "fathom-hook.py";
}

/** Send one envelope to the adapter's stdin. Errors go to stderr and stop there: a stand-in that
 *  blocks the agent is not a stand-in, and `fathom-hook.py` never exits non-zero either. */
function send(harness: string, envelope: Record<string, unknown>): void {
	const adapter = adapterPath();
	const result = spawnSync("python3", [adapter, "--harness", harness, "--cwd", String(envelope.cwd ?? process.cwd())], {
		input: JSON.stringify(envelope),
		encoding: "utf8",
	});
	if (result.status !== 0) {
		process.stderr.write(`fathom-envelope-driver: ${adapter} exited ${result.status}: ${result.stderr}\n`);
	}
	if (process.env.FATHOM_ENVELOPE_TRACE) {
		// The recorder for the shipped trace: the envelope as it was sent, one per line, which is
		// byte-for-byte what `--jsonl` replays in tests/test_adapter_contract.py.
		const { appendFileSync } = require("node:fs") as typeof import("node:fs");
		appendFileSync(process.env.FATHOM_ENVELOPE_TRACE, JSON.stringify(envelope) + "\n");
	}
}

export default function (pi: ExtensionAPI): void {
	let cwd = process.cwd();
	let sessionId = "";
	const harness = process.env.FATHOM_HARNESS ?? "claude_code";
	/** pi's `tool_execution_end` has no `args`; the envelope has `tool_input`, so remember it. */
	const inputs = new Map<string, { name: string; args: unknown }>();

	const envelope = (name: string, extra: Record<string, unknown> = {}): Record<string, unknown> => ({
		hook_event_name: name,
		session_id: sessionId,
		transcript_path: process.env.PI_SESSION_FILE ?? "",
		cwd,
		...extra,
	});

	pi.on("session_start", (event, ctx) => {
		cwd = ctx.cwd;
		sessionId = ctx.sessionManager.getSessionId() || "";
		send(harness, envelope("SessionStart", { source: event.reason, model: ctx.model?.id }));
		return undefined;
	});

	pi.on("before_agent_start", (event) => {
		send(harness, envelope("UserPromptSubmit", { prompt: event.prompt }));
		return undefined;
	});

	pi.on("tool_execution_start", (event) => {
		if (event.parentToolCallId) return undefined;
		inputs.set(event.toolCallId, { name: event.toolName, args: event.args });
		send(harness, envelope("PreToolUse", { tool_name: event.toolName, tool_input: event.args, tool_use_id: event.toolCallId }));
		return undefined;
	});

	pi.on("tool_execution_end", (event) => {
		if (event.parentToolCallId) return undefined;
		const called = inputs.get(event.toolCallId) ?? { name: event.toolName, args: {} };
		inputs.delete(event.toolCallId);
		// The envelope's `tool_response` is what the vendor hands the hook, and the adapter digests
		// it rather than storing it; a stand-in that withheld the body would leave the adapter's
		// redaction path untested, which is the one thing on this page that could still be a lie.
		send(harness, envelope(event.isError ? "PostToolUseFailure" : "PostToolUse", {
			tool_name: called.name,
			tool_input: called.args,
			tool_use_id: event.toolCallId,
			tool_response: event.result,
			duration_ms: event.durationMs,
		}));
		return undefined;
	});

	pi.on("turn_end", (event) => {
		send(harness, envelope("Stop", { turn_index: event.turnIndex }));
		return undefined;
	});

	pi.on("session_shutdown", (event) => {
		send(harness, envelope("SessionEnd", { reason: event.reason }));
		return undefined;
	});
}
