// Entry of the agent guard, run by bin/agent-guard:
//   bun guard.ts --runtime claude|codex|pi --cwd HOOK_DIR < event.json
// The event is a PreToolUse event in Claude's shape; Codex sends only Bash
// events, with the cwd possibly inside tool_input. HOOK_DIR, the hook's own
// working directory, stands in when the event names none. Exit 0 means no
// objection and exit 2 denies with the reason on stderr. The wrapper turns
// every other outcome into a denial.
import { realpathSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";
import { parseArgs } from "node:util";

import { buildRequest, evaluate, suggestions } from "./core";
import type { Runtime, Tool } from "./record";

const usage = "usage: agent-guard --runtime claude|codex|pi < event.json";
let flags: { runtime?: string; cwd?: string };
try {
  flags = parseArgs({ options: { runtime: { type: "string" }, cwd: { type: "string" } } as const }).values;
} catch {
  console.error(usage);
  process.exit(2);
}
const runtime = flags.runtime as Runtime | undefined;
const hookCwd = flags.cwd;
if (!runtime || !["claude", "codex", "pi"].includes(runtime) || !hookCwd) {
  console.error(usage);
  process.exit(2);
}

// A missing tool_input, or a known tool whose field is not a string, throws,
// which the wrapper turns into a denial: a runtime that renames or retypes a
// field must not silently stop being guarded. Tools the guard does not know pass.
const event: { tool_name?: unknown; cwd?: unknown; tool_input: Record<string, unknown> } = JSON.parse(await Bun.stdin.text());
const input = event.tool_input;
const string = (value: unknown) => (typeof value === "string" ? value : undefined);
const cwd = string(event.cwd) ?? string(input["cwd"]) ?? hookCwd;

const fields: Record<string, [Tool, string]> = {
  bash: ["bash", "command"],
  read: ["read", "file_path"],
  edit: ["edit", "file_path"],
  write: ["write", "file_path"],
  grep: ["grep", "path"],
};
const [tool, field] = fields[(string(event.tool_name) ?? "Bash").toLowerCase()] ?? [];
// Grep searches the working directory when it names no path.
const value = field && (tool === "grep" && input[field] === undefined ? "" : string(input[field]));
if (field && value === undefined) throw new Error(`tool_input.${field} is not a string`);
// The link walk reports physical paths, so a home directory spelled through a link (`/tmp`) is compared by the spelling the walk reports.
// The home directory is not inside App Data, so resolving it cannot search a protected tree.
const request = tool && value !== undefined ? buildRequest(runtime, tool, cwd, value, string(input["glob"]) ?? "", realpathSync(resolve(homedir()))) : undefined;
const reason = request && evaluate(request);
if (reason) {
  console.error(runtime === "claude" ? `DENIED: ${reason} Do NOT bypass this restriction or retry the same blocked command.` : reason);
  process.exitCode = 2;
} else if (request) {
  const advice = suggestions(request);
  if (advice.length) console.log(JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: advice.join("\n") } }));
}
