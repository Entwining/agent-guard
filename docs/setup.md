# Set up agent-guard

`agent-guard` is a macOS pre-tool hook for Claude Code, Codex, and Pi. It blocks supported tool calls that would scan broad filesystem roots or read protected App Data and credential material. An unscoped `rg`, `fd`, `ag`, `ack`, or `tree` run from the home directory is denied; pass an explicit project path.

## Prerequisites

- Apple Silicon macOS with a local Claude Code, Codex, or Pi session.
- Homebrew for installation. Go is a build dependency managed by the formula.
- Permission to edit the configuration for the runtime you choose. Codex's managed configuration uses the system `/etc/codex/requirements.toml` and may require an administrator.

The Homebrew executable is `/opt/homebrew/bin/agent-guard`. The examples below use that path; replace it if your Homebrew prefix differs.

## Install

Install from tagged GitHub source through the LoopHubs tap:

```sh
brew tap loophubs/tap
brew install loophubs/tap/agent-guard
/opt/homebrew/bin/agent-guard --version
```

For an existing Homebrew installation, use `brew update` followed by `brew upgrade loophubs/tap/agent-guard`.

## Register Claude Code

Merge the following [Claude Code `PreToolUse` hook](https://code.claude.com/docs/en/hooks) into your existing `~/.claude/settings.json`; retain other settings and hook entries. A hook exit code of `2` blocks the call.

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash|Read|Edit|Write|Grep",
      "hooks": [{ "type": "command", "command": "/opt/homebrew/bin/agent-guard --runtime claude", "timeout": 5 }]
    }]
  }
}
```

Start a new Claude Code session after saving. This matcher covers the named shell, file, and search tools; another tool needs its own matching handler if it should be checked.

## Register Codex

Use a [managed `PreToolUse` hook](https://learn.chatgpt.com/docs/hooks) in macOS's [system requirements file](https://learn.chatgpt.com/docs/enterprise/managed-configuration), `/etc/codex/requirements.toml`. Merge these tables with existing requirements rather than replacing the file.

```toml
[features]
hooks = true

[hooks]
managed_dir = "/opt/homebrew/bin"

[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "/opt/homebrew/bin/agent-guard --runtime codex"
timeout = 5
```

Start a new Codex session and inspect `/hooks` to confirm this `PreToolUse` hook appears as managed. This registration and the verified Codex integration cover Bash calls only. Before adding another matcher, verify that the runtime's tool name and event payload match a supported guard input.

## Register Pi

Create the [Pi `tool_call` extension](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md) below at `~/.pi/agent/extensions/agent-guard.ts`, then start or reload Pi so it discovers the extension. Returning `block: true` prevents the call.

```ts
import { spawnSync } from "node:child_process";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const guard = "/opt/homebrew/bin/agent-guard";

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

Check the installed version and an allowed synthetic event:

```sh
/opt/homebrew/bin/agent-guard --version
printf '%s\n' '{"tool_name":"Bash","tool_input":{"command":"ls"}}' | /opt/homebrew/bin/agent-guard --runtime codex
```

The event is checked, not executed. Require exit code `0` and no output for this event. This smoke check does not show that a runtime loaded its hook. Inspect the runtime's hook listing and confirm registration separately.

Developers can run the full [installed acceptance tool](../cmd/agent-guard-verify) with Go from a checkout of the release being evaluated, passing the absolute installed executable, for example `/opt/homebrew/bin/agent-guard`. The tool is not part of the runtime package.

## Remove

First remove the entry you added from Claude Code's `PreToolUse` list, Codex's `/etc/codex/requirements.toml`, or Pi's `~/.pi/agent/extensions/agent-guard.ts`. Keep unrelated entries and requirements. Restart or reload the runtime and verify that its hook listing no longer contains the guard. For Codex, retain any existing `[features].hooks` setting needed by other hooks.

Then uninstall:

```sh
brew uninstall loophubs/tap/agent-guard
```

## Safety model and limits

The guard denies a call when its own check fails or times out because it cannot establish that the call is safe. Exit code `0` means the guard found no objection; it does not override the runtime's own permission rules.

The guard is a bounded preflight check. It decides from the targets it infers under modelled command semantics. The limits below fall into three families; operating system read restrictions must cover the reads they leave out.

**Observation coverage** is which calls reach the guard. A disabled, skipped, or unregistered hook cannot inspect a call, and a runtime that treats a hook launch failure or its own hook timeout as non-blocking lets the call proceed unchecked; custom tools and processes outside the registered runtime are outside this coverage. `Glob` is not covered by the matchers or guard input handlers. Codex's example checks Bash calls, while the Pi adapter checks the five named tools and blocks the call itself when the guard fails, is missing or times out.

**Execution semantics** is what a command does when it runs, beyond what its text names:

- The guard treats a program it does not know as reading every path it is handed, so `aws s3 cp .env s3://bucket/x`, `open .env`, and `python3 script.py .env` are denied. Pass a credential file through the program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`, which the guard allows for the clients its program table models. A file the client uses itself, such as `ssh -i` or `docker run --env-file`, is allowed by design; the guard does not control what the client does with its contents.
- A read whose target the program picks while it runs cannot be decided before execution: an interpreter opening a file itself, `git diff`, `git log -p`, or `git show` without a path operand, and a walk that reaches credential files it does not treat as hidden (such as `*.pem` under `rg`, `fd -x`, `tar`, or `cp -r`). An interpreter that chooses to print process environment values, such as Python code that prints `os.environ`, is also outside the static dump-command list.
- Command text the guard does not follow is also not covered: process substitution as input (`xargs cat < <(echo …)`), names another command prints into `xargs` (`ls *.pem | xargs cat`), wrappers the guard does not list such as `xcrun`, a value glued to a short option of a program the guard does not know (`tool -f.env`), a path built by command substitution or held in a variable (including a `for` loop variable), and shell state such as `cd -`, `~-`, `readonly`, `set -P`, zsh's `CHASE_LINKS`, or `env -C` with a redirection.
- The table models only some file options of curl, wget, docker, ssh, scp, sftp, and git, and treats the value of any other option as a name it does not judge: docker's build context and its `--build-context`, `--cache-from`, `--cache-to`, `--output`, `--ssh`, `--metadata-file`, and `--security-opt` values, the words `docker compose run` and `compose exec` pass to the container command, the other file settings of `ssh -o`, and `git clone --reference`, `--template`, and `--separate-git-dir` or `git worktree add`. A `-` that a client reads as standard input (`curl -K -`, `curl -T -`, `wget -i -`) is judged as a path when the working directory is sensitive.
- A command that prints a secret it is allowed to read, such as `gcloud auth print-access-token`, has no path to check, so the guard lists the subcommands it knows and cannot list them all.

**State and resource identity** is which file a path names when the command runs, compared with when the guard checked it. For a shell operand or file redirect with an unresolved expansion, the guard may call `readlink` on the fixed path prefix; it judges the uncertain suffix lexically without passing that suffix to `readlink` or `stat`. Unresolved working directories and iterator roots are outside this probe guarantee. A file moved or linked by an earlier command and then read, child links that `rg -L` follows, and a wildcard the shell expands to a link (`da*/x` where `data-link` leads elsewhere) are not resolved.

The hook installs no operating system read policy. In a local 2x2 comparison recorded by the [read-enforcement experiment](../experiments/README.md), every run wrapped by `sandbox-exec` exited 71; a control in the sandboxed Codex environment failed with `sandbox_apply: Operation not permitted` before `/usr/bin/true` started. These are execution environment failures, not enforced read denials or proof that Seatbelt is unavailable on macOS 27. Nesting is the likely explanation, not a verified kernel denial record. [Claude Code](https://code.claude.com/docs/en/sandboxing) and [Codex's Seatbelt implementation](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt.rs) still use Seatbelt. The guard alone denied the direct operand and inline literal forms but allowed paths chosen inside an external script or runtime configuration; those reads remain outside a command text preflight check.

[Apple's container access rules](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox) tie TCC access to another app's container to the recognized client identity and the user's grant; an allowed app may access other app containers until it exits. The [read-enforcement experiment](../experiments/README.md) observed no prompt and did not establish whether the shell, hook or child was the responsible client. [App Sandbox](https://developer.apple.com/documentation/xcode/configuring-the-macos-app-sandbox) belongs to the owning app's entitlements, while [Endpoint Security](https://developer.apple.com/documentation/BundleResources/Entitlements/com.apple.developer.endpoint-security.client) requires its client entitlement.

macOS 27 [AppSettings privacy defaults](https://developer.apple.com/documentation/devicemanagement/appsettings) configure consent defaults, not arbitrary file read restrictions, and do not replace the administrator App Data policy described below.

## Administrator App Data policy

The [App Data profile generator](../cmd/agent-guard-profile) creates an unsigned policy for an explicitly selected client and prints the exact inspection, MDM deployment, and test steps. Run this optional tool with Go from a checkout of the release being evaluated:

```sh
go run ./cmd/agent-guard-profile --instructions
```

Follow the printed prerequisite, attribution, deployment, and test steps before applying a profile. They use [Apple's deployment requirements](https://support.apple.com/guide/deployment/privacy-preferences-policy-control-payload-dep38df53c2a/web) and the reviewed [PPPC schema](https://github.com/apple/device-management/blob/09f249a06e7e3289930bf6d05f38fb562f748ebf/mdm/profiles/com.apple.TCC.configuration-profile-policy.yaml). A successful installed-package check or plist validation does not prove OS enforcement.

## Development checks

Use the Go version and parser source pinned in `go.mod`. The parser pin preserves the supported shell syntax. Development tools, tests and runtime harnesses use Go.

Keep build outputs, module and build caches, and evidence outside every checkout:

```sh
out=/absolute/path/outside/checkouts/agent-guard-evidence
export GOMODCACHE="$out/modcache" GOCACHE="$out/buildcache"
make check
make build OUT="$out/package"
go run ./cmd/agent-guard-verify "$out/package/bin/agent-guard"
```

`make check` runs `native/check`: goimports formatting and import grouping, go vet, Staticcheck, and Go race tests across the implementation, tools and harnesses. Both development tools are pinned in `go.mod` and run with `go tool`. goimports runs in `-format-only` mode, which applies gofmt formatting without adding or removing imports; `native/check` prints the fix command for any file it lists. Set `GO` to an absolute executable path when it is absent from `PATH`. Plain `go test ./...` includes all 3,795 fixture cases and checks exact public exit codes, denial text and Claude advice; it requires no exporter or environment opt-in.

The installed verifier requires the assembled `bin/agent-guard`, adjacent `agent-guard-native` and `VERSION`; it resolves the executable paths, records both hashes and checks that both executables report the package version. Require all 33 protocol cases to pass. It does not prove hook loading or all descendant cleanup.

The [synthetic runtime and lifecycle harnesses](../tests/harness/README.md) check the assembled entry and adjacent binary. Runtime verdicts, direct protocol checks and instrumented lifecycle checks are separate evidence. A runtime failure before hooks is not a guard denial. Check registration and runtime acceptance separately before replacing an installation.

## Release

This project remains in `0.x`; do not prepare `1.0.0` under the current policy. Use patch for internal, fix, dependency and documentation changes that leave user-visible behavior unchanged, and minor only for a real contract change such as a widened or narrowed scope of what the guard blocks or allows. The [release workflow](../.agents/skills/release/SKILL.md) owns authorization, version selection, preparation folding and tag safety.

`VERSION` is the single version owner. From a validated default-branch checkout, update it and write `docs/releases/<version>.md`, then create an annotated `v<version>` tag and push it only after the release gate clears.

Pushing the tag starts `publish.yml`, which checks the version and notes, runs the checks, creates the GitHub Release from the notes, then dispatches `agent-guard-release` to the tap. The formula builds that GitHub tag and records its commit revision. The tap uses Homebrew livecheck's `github_latest` strategy and `bump-formula-pr --write-only` for later updates, so a tag alone cannot trigger an update before the Release passes its checks. Verify the Release, tap revision and a built installation before declaring a release complete.

If a job fails, inspect the Release and tap state before retrying; an existing Release is reused and only its tap notification is repeated.
