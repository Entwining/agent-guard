# Set up agent-guard

`agent-guard` is a macOS pre-tool hook for Claude Code, Codex, and Pi. It blocks supported tool calls that would scan broad filesystem roots or read protected App Data and credential material.

## Prerequisites

- macOS with a local Claude Code, Codex, or Pi session.
- Bun 1.4 or newer on the hook's `PATH`, including when the runtime starts outside your interactive shell. Check with `bun --version`.
- Permission to edit the configuration for the runtime you choose. Codex's managed configuration uses the system `/etc/codex/requirements.toml` and may require an administrator.

Bun is required at runtime even if you install with npm or Homebrew. After installation, run `command -v agent-guard`, confirm it prints an absolute path, and substitute that path for `/absolute/path/to/agent-guard` below. For Codex, use the path's containing directory for `/absolute/path/to`.

The package denies unscoped `rg` and `fd` searches from the home directory itself. No `~/.ignore` file or other dotfiles setup is required for that protection; give an explicit project path to search from home.

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
brew tap loophubs/tap
brew install loophubs/tap/agent-guard
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

A failed, missing, or timed-out guard check blocks the Pi tool call.

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
brew uninstall loophubs/tap/agent-guard
```

## Release

From a clean checkout of the default branch, choose the next `patch`, `minor`, or `major` version. A change to what the guard denies or allows, whether stricter or looser, is a `minor` release; a fix that leaves both unchanged is a `patch`. A commit's type prefix does not decide the level: a `fix:` commit that widens denials still needs `minor`, and a dependency update that Renovate prefixes with `fix` stays a `patch` while parsing behavior is unchanged. Configure npm trusted publishing for this repository and `publish.yml` with direct `npm publish` allowed.

For a patch release (substitute `minor` or `major` when appropriate):

```sh
npm version patch --no-git-tag-version
version=$(node -p 'require("./package.json").version')
mkdir -p docs/releases
```

Write `docs/releases/<version>.md` for this version only, using the value of `version` in its filename. Name what the guard newly blocks or allows and what existing checks became stricter or looser; include dependency changes only if useful. Review the file before committing it with the version bump. These files are individual Release bodies, not an accumulated `CHANGELOG.md`, and the npm package excludes `docs/`.

Commit the version and its notes together, then push the annotated tag:

```sh
git add package.json "docs/releases/$version.md"
git commit -m "release: prepare v$version"
git tag -a "v$version" -m "v$version"
git push --follow-tags
```

`--no-git-tag-version` leaves the version change uncommitted so the notes file can enter the same bump commit. `bun.lock` contains dependency versions but no root package version, so the version bump does not require a lockfile edit. Pushing the tag triggers the publish workflow, which checks it against `package.json` and requires a nonempty `docs/releases/<version>.md` before publishing to npm with OIDC. The Release job reads that file from the tagged commit and creates the GitHub Release only after publishing succeeds; its step summary then records the package version and Release URL.

For a transient job failure, use **Re-run failed jobs** in GitHub Actions for the same tag ref. If npm publishing succeeded and Release creation failed, rerun only the Release job; a successful npm publish cannot be repeated for the same package version. If the tagged commit lacks its notes file, re-running cannot add it: prepare a new version commit and tag with the notes included.

## Safety model and limits

The guard denies a call when its check fails or times out because it cannot establish that the call is safe. Exit code `0` means the guard found no objection; it does not override the runtime's own permission rules.

The guard is a bounded preflight check, not an operating-system sandbox. It decides from the targets it infers under modelled command semantics, and exit code `0` means only that it found no objection to those targets. The limits below fall into three families. Only the operating system's read restrictions on the credential stores cover what they leave out.

**Observation coverage** is which calls reach the guard. A disabled, skipped, or unregistered hook cannot inspect a call, and custom tools and processes outside the registered runtime are outside this coverage. `Glob` is not covered by the matchers or guard input handlers. Codex's example checks Bash calls, while the Pi adapter checks the five named tools.

**Execution semantics** is what a command does when it runs, beyond what its text names. The guard treats a program it does not know as reading every path it is handed, so `aws s3 cp .env s3://bucket/x`, `open .env`, and `python3 script.py .env` are denied; pass a credential file through the program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`, which the guard allows for the clients its program table models. The guard does not control what such a client does with the contents. A read whose target the program picks while it runs cannot be decided before execution: an interpreter opening a file itself, `git diff`, `git log -p`, or `git show` without a path operand, and a walk that reaches credential files it does not treat as hidden (such as `*.pem` under `rg`, `fd -x`, `tar`, or `cp -r`). Command text the guard does not follow is also not covered: process substitution as input (`xargs cat < <(echo …)`), names another command prints into `xargs` (`ls *.pem | xargs cat`), wrappers the guard does not list such as `xcrun`, a value glued to a short option of a program the guard does not know (`tool -f.env`), a path built by command substitution or held in a variable (including a `for` loop variable), and shell state such as `cd -`, `~-`, `readonly`, or `env -C` with a redirection. The table models only some file options of curl, wget, docker, ssh, scp, sftp, and git, and treats the value of any other option as a name it does not judge: docker's build context and its `--build-context`, `--cache-from`, `--cache-to`, `--output`, `--ssh`, `--metadata-file`, and `--security-opt` values, the words `docker compose run` and `compose exec` pass to the container command, the other file settings of `ssh -o`, and `git clone --reference`, `--template`, and `--separate-git-dir` or `git worktree add`. A `-` that a client reads as standard input (`curl -K -`, `curl -T -`, `wget -i -`) is judged as a path when the working directory is sensitive. A file the client uses itself, such as `ssh -i` or `docker run --env-file`, is allowed by design. A command that prints a secret it is allowed to read, such as `gcloud auth print-access-token`, has no path to check, so the guard lists the subcommands it knows and cannot list them all.

**State and resource identity** is which file a path names when the command runs, compared with when the guard checked it. A file moved or linked by an earlier command and then read, child links that `rg -L` follows, and a wildcard the shell expands to a link (`da*/x` where `data-link` leads elsewhere) are not resolved.
