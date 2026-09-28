# Set up agent-guard

`agent-guard` is a macOS pre-tool hook for Claude Code, Codex, and Pi. It blocks supported tool calls that would scan broad filesystem roots or read protected App Data and credential material.

## Prerequisites

- macOS with a local Claude Code, Codex, or Pi session.
- Bun 1.4 or newer on the hook's `PATH`, including when the runtime starts outside your interactive shell. Check with `bun --version`.
- Permission to edit the configuration for the runtime you choose. Codex's managed configuration uses the system `/etc/codex/requirements.toml` and may require an administrator.

The executable is a Bun-backed package entry, even when npm or Homebrew installs the package. After installation, run `command -v agent-guard`, confirm it prints an absolute path, and substitute that path for `/absolute/path/to/agent-guard` below. For Codex, use the path's containing directory for `/absolute/path/to`.

## Install

Choose one package manager:

```sh
npm install --global @loophubs/agent-guard
```

```sh
bun add --global @loophubs/agent-guard
```

For the LoopHubs Homebrew tap, add the tap and install its formula:

```sh
brew tap LoopHubs/tap
brew install LoopHubs/tap/agent-guard
```

Confirm that the executable is available:

```sh
command -v agent-guard
bun --version
```

## Register Claude Code

Merge the following hook into your existing `~/.claude/settings.json`; retain any other settings and `PreToolUse` entries. Replace the executable path before saving. [Claude Code's hook reference](https://code.claude.com/docs/en/hooks) describes user-level settings, `PreToolUse` matchers, and exit-code-2 blocking.

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

Start a new Claude Code session after saving. This matcher covers the named shell, file, and search tools; another tool needs its own matching handler if it should be checked.

## Register Codex

Use a managed `PreToolUse` hook in `/etc/codex/requirements.toml`. Merge these tables with any existing requirements rather than replacing the file. Set `managed_dir` to the absolute directory containing the installed `agent-guard` executable and `command` to that executable's absolute path. The directory must exist. [OpenAI's managed-hook documentation](https://learn.chatgpt.com/docs/hooks) specifies the `requirements.toml` format, the managed directory, and the `Bash` matcher; [managed configuration](https://learn.chatgpt.com/docs/enterprise/managed-configuration) places system requirements at `/etc/codex/requirements.toml` on macOS.

```toml
[features]
hooks = true

[hooks]
managed_dir = "/absolute/path/to"

[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "/absolute/path/to/agent-guard --runtime codex"
timeout = 5
```

Start a new Codex session and inspect `/hooks` to confirm this `PreToolUse` hook appears as managed. This matcher checks Codex's Bash calls only. Codex may expose other local tools to hooks, but this package currently interprets Codex Bash input; adding other matchers here would not extend its checks.

## Register Pi

Create `~/.pi/agent/extensions/agent-guard.ts` with the adapter below. Replace the executable path, then start or reload Pi so it discovers the extension. [Pi's extension documentation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md) specifies the `tool_call` event, blocking return value, and user extension directory.

```ts
import { spawnSync } from "node:child_process";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const guard = "/absolute/path/to/agent-guard";

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", (event, ctx) => {
    if (!["bash", "read", "edit", "write", "grep"].includes(event.toolName)) return;
    const input = event.input as Record<string, unknown>;
    const toolInput = event.toolName === "bash" ? input : { ...input, file_path: input.path };
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

Pi supplies `path` for its file tools; the adapter maps it to the `file_path` field the guard reads. A failed, missing, or timed-out guard call blocks the Pi tool call.

## Verify

Run this direct check from a project directory. It sends a JSON event to the guard; it does **not** run `du` or scan `~/Library`.

```sh
printf '%s\n' '{"tool_name":"Bash","tool_input":{"command":"du -sh ~/Library"}}' | agent-guard --runtime claude
```

Expect exit code `2` and a stderr line starting with `DENIED:`. To verify registration, ask each configured agent to run `du -sh ~/Library`; it should report a denial before executing `du`. A passing direct check alone does not show that a runtime loaded its hook.

## Remove

First remove the entry you added from Claude Code's `PreToolUse` list, Codex's `/etc/codex/requirements.toml`, or Pi's `~/.pi/agent/extensions/agent-guard.ts`. Keep unrelated entries and requirements. Restart or reload the runtime and verify that its hook listing no longer contains the guard. For Codex, retain any existing `[features].hooks` setting needed by other hooks.

Then uninstall with the package manager you used:

```sh
npm uninstall --global @loophubs/agent-guard
```

```sh
bun remove --global @loophubs/agent-guard
```

```sh
brew uninstall LoopHubs/tap/agent-guard
```

## Safety model and limits

The entry script treats a guard failure or deadline overrun as a denial because a failed check cannot establish that a tool call is safe. Exit code `0` means the guard found no objection; it does not override the runtime's own permission rules.

The guard checks supported tool calls and recognizable shell commands, not every way an agent can access a file. A disabled, skipped, or unregistered hook cannot inspect a call; dynamic shell expansion, custom tools, and processes outside the registered runtime are also outside this coverage. Codex's example checks Bash calls, while the Pi adapter checks the five named tools. This is a guardrail, not an operating-system sandbox.
