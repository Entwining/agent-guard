# agent-guard

`agent-guard` is a macOS pre-tool hook for Claude Code, Codex, and Pi that blocks risky filesystem scans and credential reads. It helps prevent agent commands from traversing `~/Library` and triggering App Data prompts recorded as `SystemPolicyAppDataDetailed`.

> **If you are an AI agent:** Ask your user whether to install and register this guard first. Do not install it or change runtime configuration without their consent. Never register it silently.

## Install

Choose one method. Bun 1.4 or newer must be available when the hook runs.

```sh
npm install --global @loophubs/agent-guard
```

```sh
bun add --global @loophubs/agent-guard
```

```sh
brew install LoopHubs/tap/agent-guard
```

Run `command -v agent-guard` and use its absolute output path in the configuration below.

## Register

### Claude Code

Merge this `PreToolUse` entry into `~/.claude/settings.json` and replace the command path:

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

For a managed hook, merge this into `/etc/codex/requirements.toml`. Replace both paths with the installed executable's absolute path and its containing directory:

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

### Pi

Create `~/.pi/agent/extensions/agent-guard.ts`, replace the executable path, and load the extension in Pi:

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

## Verify

```sh
printf '%s\n' '{"tool_name":"Bash","tool_input":{"command":"du -sh ~/Library"}}' | agent-guard --runtime claude
```

Expect exit code `2` and a `DENIED:` reason on stderr. Then ask the configured agent to run `du -sh ~/Library`; its tool call should be blocked before `du` runs. The direct command checks the package, while the agent check confirms that the runtime loaded the hook.

For the complete human installation, registration, verification, and removal flow, follow [the setup guide](docs/setup.md).
