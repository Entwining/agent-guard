# agent-guard

`agent-guard` is a macOS pre-tool hook for Claude Code, Codex, and Pi that blocks risky filesystem scans and credential reads. It helps avoid agent commands that traverse `~/Library` and trigger macOS App Data prompts recorded as `SystemPolicyAppDataDetailed`.

## Install

Install globally with npm or Bun (Bun 1.4 or newer must be available at runtime):

```sh
npm install --global @loophubs/agent-guard
# or
bun add --global @loophubs/agent-guard
command -v agent-guard
```

Use the absolute path printed by the last command in the examples below.

## Register a runtime

### Claude Code

Add this `PreToolUse` hook to `~/.claude/settings.json`, replacing the command path. It covers Claude Code's Bash and file-reading tools; [Claude Code's hook settings](https://code.claude.com/docs/en/hooks#hook-locations) describe the user-level location.

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash|Read|Edit|Write|Grep",
      "hooks": [{ "type": "command", "command": "/absolute/path/to/agent-guard --runtime claude", "timeout": 5 }]
    }]
  }
}
```

### Codex

Add this to `~/.codex/config.toml`, replacing the command path. [OpenAI's hook documentation](https://learn.chatgpt.com/docs/hooks) specifies `PreToolUse`, the Bash matcher, and `/hooks` trust review; open `/hooks` in Codex and trust the new hook before testing it.

```toml
[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "/absolute/path/to/agent-guard --runtime codex"
timeout = 5
```

### Pi

Create `~/.pi/agent/extensions/agent-guard.ts` with this adapter, replacing the command path. [Pi's extension API](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md) loads files in that directory and lets `tool_call` block a tool before execution.

```ts
import { spawnSync } from "node:child_process";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const guard = "/absolute/path/to/agent-guard";

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", (event, ctx) => {
    if (!["bash", "read", "edit", "write", "grep"].includes(event.toolName)) return;
    const input = event.input as Record<string, unknown>;
    const toolInput = event.toolName === "bash" ? input : {
      ...input,
      file_path: input.path,
    };
    const result = spawnSync(guard, ["--runtime", "pi"], {
      cwd: ctx.cwd,
      input: JSON.stringify({ tool_name: event.toolName, tool_input: toolInput, cwd: ctx.cwd }),
      encoding: "utf8",
      timeout: 4500,
    });
    if (result.status === 0) return;
    return { block: true, reason: result.stderr?.trim() || result.error?.message || "agent-guard failed" };
  });
}
```

## Verify it blocks

From a project directory, ask the configured agent to run `du -sh ~/Library`. It should report the guard's denial **without running `du`**. You can check the package entry directly without reading `~/Library`:

```sh
printf '%s\n' '{"tool_name":"Bash","tool_input":{"command":"du -sh ~/Library"}}' | agent-guard --runtime claude
```

The direct check should exit 2 and print a `DENIED:` reason on stderr. A direct check confirms the package works; the agent check confirms that runtime loaded its hook.

## Limits

The guard checks supported tool calls and recognizable shell commands; it does not sandbox the agent or cover every custom tool, dynamic shell expansion, or process outside a registered hook. Codex's example covers Bash calls, while the Pi adapter covers Bash, read, edit, write, and grep; other Pi tools need their own adapter handling. A broken or timed-out package entry denies a checked call, but a hook disabled or skipped by its runtime cannot inspect that call.
