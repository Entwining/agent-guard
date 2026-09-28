// Entry of the agent guard, run by bin/agent-guard:
//   bun guard.ts --runtime claude|codex|pi --cwd HOOK_DIR < event.json
// The event is a PreToolUse event in Claude's shape; Codex sends only Bash
// events, with the cwd possibly inside tool_input. HOOK_DIR, the hook's own
// working directory, stands in when the event names none. Exit 0 means no
// objection and exit 2 denies with the reason on stderr. The wrapper turns
// every other outcome into a denial.
import { homedir } from "node:os";
import { buildRequest, evaluate, suggestions } from "./core.ts";
import type { Runtime, Tool } from "./record.ts";

const [flag, runtime, cwdFlag, hookCwd] = process.argv.slice(2) as [string, Runtime, string, string];
if (flag !== "--runtime" || !["claude", "codex", "pi"].includes(runtime) || cwdFlag !== "--cwd" || !hookCwd) {
  console.error("usage: agent-guard --runtime claude|codex|pi < event.json");
  process.exit(2);
}

// A missing tool_input throws, which the wrapper turns into a denial. A
// tool_input without the field a tool needs names nothing to check and passes.
const event: { tool_name?: unknown; cwd?: unknown; tool_input: Record<string, unknown> } = JSON.parse(await Bun.stdin.text());
const input = event.tool_input;
const string = (value: unknown) => (typeof value === "string" ? value : undefined);
const cwd = string(event.cwd) ?? string(input.cwd) ?? hookCwd;

const tools: Record<string, [Tool, string | undefined]> = {
  bash: ["bash", string(input.command)],
  read: ["read", string(input.file_path)],
  edit: ["edit", string(input.file_path)],
  write: ["write", string(input.file_path)],
  grep: ["grep", string(input.path) ?? ""],
};
const [tool, value] = tools[(string(event.tool_name) ?? "Bash").toLowerCase()] ?? [];
const request = tool && value !== undefined ? buildRequest(runtime, tool, cwd, value, string(input.glob) ?? "", homedir()) : undefined;
const reason = request && evaluate(request);
if (reason) {
  console.error(runtime === "claude" ? `DENIED: ${reason} Do NOT bypass this restriction or retry the same blocked command.` : reason);
  process.exitCode = 2;
} else if (request) {
  const advice = suggestions(request);
  if (advice.length) console.log(JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: advice.join("\n") } }));
}
