# Set up agent-guard

`agent-guard` is a macOS pre-tool hook for Claude Code, Codex, and Pi. It blocks supported tool calls that would scan broad filesystem roots or read protected App Data and credential material. An unscoped `rg`, `fd`, `ag`, `ack`, or `tree` run from the home directory is denied; pass an explicit project path.

Hidden recursive content searches in shell commands are refused when an operand is or may be a directory. A named public file, including a resolved link to one, does not traverse hidden files and remains permitted with recursive flags. The guard checks lexical protection and resolves links before using file metadata to distinguish that file from a directory; metadata failures remain failed checks.

## Prerequisites

- Apple Silicon macOS with a local Claude Code, Codex, or Pi session.
- Homebrew for installation. The source package, development tools and acceptance harnesses build with Rust.
- Permission to edit the configuration for the runtime you choose. Codex's managed configuration uses the system `/etc/codex/requirements.toml` and may require an administrator.

The Homebrew executable is `/opt/homebrew/bin/agent-guard`. The examples below use that path; replace it if your Homebrew prefix differs.

## Install

Install from tagged GitHub source through the LoopHubs tap:

> **AI agents:** Read this guide before installation or registration. Use existing authorization for the exact action and target; otherwise obtain it before installing or changing runtime configuration.

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

Developers can run the full [installed acceptance tool](../cmd/agent-guard-verify/main.rs) with `cargo run --bin agent-guard-verify --` from a checkout of the release being evaluated, passing the absolute installed executable, for example `/opt/homebrew/bin/agent-guard`. The tool is not part of the runtime package.

## Remove

First remove the entry you added from Claude Code's `PreToolUse` list, Codex's `/etc/codex/requirements.toml`, or Pi's `~/.pi/agent/extensions/agent-guard.ts`. Keep unrelated entries and requirements. Restart or reload the runtime and verify that its hook listing no longer contains the guard. For Codex, retain any existing `[features].hooks` setting needed by other hooks.

Then uninstall:

```sh
brew uninstall loophubs/tap/agent-guard
```

## Safety model and limits

The guard denies a call when its own check fails or times out because it cannot establish that the call is safe. Exit code `0` means the guard found no objection; it does not override the runtime's own permission rules.

Inspection-budget and deadline refusals ask for smaller calls with explicit public targets. Bounded or cyclic aliases require an ordinary absolute path. Invalid events require complete UTF-8 JSON with documented tool fields and an absolute working directory. If a single public-file check still fails, the checker owner can run `agent-guard --version` and repair the reported fault; none of these steps authorizes the blocked operation.

The serialized event is limited to 262,144 bytes, and shell substitutions/groups to 64 simultaneous nested delimiters. Excess input or nesting, a relative working directory, and invalid UTF-8 produce completed refusals: native checker status `2`, native runner status `3`, and hook status `2`. The reason asks the caller to shorten or split the request, supply an absolute working directory, or encode UTF-8. Malformed events, filesystem probe faults and other operational errors remain failed checks; the hook blocks them too. The checker deadline is 2.5 seconds, the runner deadline 2.8 seconds, and the entry deadline 3 seconds, below the example consumer timeout of 5 seconds. Cancellation must complete and reap children.

The byte cap was measured on release builds using public literal arguments, data-heavy Git pathspec substitutions and repeated statements from 64 KiB through 8 MiB. The substitution shape took 0.318 seconds at 256 KiB, 0.631 at 512 KiB and 1.286 at 1 MiB. 256 KiB was the largest measured warm size below 0.5 seconds; its serialized envelope was 262,286 bytes, so the cap is 262,144. The first cold literal launch took 0.988 seconds. These finite measurements describe those shapes on that host, not a universal latency guarantee. Depth bounds simultaneous nesting, not statement count or list position.

The guard is a bounded preflight check. It decides from the targets it infers under modelled command semantics. The limits below fall into three families; operating system read restrictions must cover the reads they leave out.

**Observation coverage** is which calls reach the guard. A disabled, skipped, or unregistered hook cannot inspect a call, and a runtime that treats a hook launch failure or its own hook timeout as non-blocking lets the call proceed unchecked; custom tools and processes outside the registered runtime are outside this coverage. `Glob` is not covered by the matchers or guard input handlers. Codex's example checks Bash calls, while the Pi adapter checks the five named tools and blocks the call itself when the guard fails, is missing or times out. Tool paths are checked as the consumer resolves them: Claude Code reads a Grep `file_path` as its `path` and a Write `path` as its `file_path` when the named field is absent, and trims surrounding whitespace, and Pi replaces Unicode spaces, drops a leading `@` and decodes lowercase `file://` URLs. Any other `file:` spelling in a tool path, including every one Claude Code receives, is a relative name, as it is for those tools. Codex's hook input omits the `workdir` an `exec_command` call runs in ([openai/codex#33986](https://github.com/openai/codex/issues/33986)), so relative paths in Codex Bash calls are checked against the session `cwd`.

**Execution semantics** is what a command does when it runs, beyond what its text names:

- The guard treats a program it does not know as reading every path it is handed, so `aws s3 cp .env s3://bucket/x`, `open .env`, and `python3 script.py .env` are denied. Pass a credential file through the program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`, which the guard allows for the clients its program table models. A file the client uses itself, such as `ssh -i` or `docker run --env-file`, is allowed by design; the guard does not control what the client does with its contents.
- A read whose target the program picks while it runs cannot be decided before execution: an interpreter opening a file itself, `git diff`, `git log -p`, or `git show` without a path operand, and a walk that reaches credential files it does not treat as hidden (such as `*.pem` under `rg`, `fd -x`, `tar`, or `cp -r`). An interpreter that chooses to print process environment values, such as Python code that prints `os.environ`, is also outside the static dump-command list.
- A wildcard without credential-identifying fixed text, such as `cat *`, `cat s*`, or a Grep glob `**/*.ts`, is not intersected with the credential catalog, so it can expand to a protected file. Ordinary source globs such as `**/*.{ts,tsx,js}` remain permitted under a public project root. Explicit sensitive patterns such as `*.pem`, `.env*`, `**/.aws/credentials` and `**/.ssh/id_*` are still denied; the guard does not list directories to determine wildcard expansions. Without listing, the stored spelling is unknown, so a bracket class matches either ASCII case of a protected name: `cat ~/.[!s]sh/id_rsa` is denied as if the directory could be spelled `.Ssh`.
- Positive Grep tool globs and shell search globs (`rg -g`, `--glob`, `--iglob`, `grep --include`) without a path separator match basenames at any depth under the search root. Globs containing a separator keep their complete relative path; a glob with a `..` component selects nothing, and negative globs do not add read targets. Without `--hidden`, ripgrep still enters each hidden file or directory a positive glob matches. The guard therefore checks hidden names when any glob's last component can match a dot name: `rg -g '*.npmrc' .` and `rg -g .aws -g credentials .` are denied, while `rg -g credentials .` is permitted because ripgrep does not enter `.aws` for it. A wildcard-leading glob counts for every glob in the command, so `rg -g '*.ts' -g credentials .` is denied as well. The Grep tools and `grep --include` always reach hidden names, so a Grep glob `credentials` targets nested `.aws/credentials` files. Claude Code's Grep splits its `glob` at whitespace, then at commas in any piece without both `{` and `}`, and passes each piece as its own `--glob`, so each positive piece is checked as a filter and `!*.jsonl, .env` still reads `.env`; Pi's Grep passes the whole value as one `--glob`.
- The Claude Code and Pi Grep tools run ripgrep with `--hidden`, so a Grep call under a public root also searches hidden files its ignore rules do not exclude, such as an untracked `.env` outside a Git repository. A shell search glob whose last component matches any name, such as `rg -g '*' .` or `rg -g 'src/**' .`, does the same. The guard checks the root and glob, not the hidden descendants of a public root; a shell `rg --hidden` over a directory is refused.
- Literal bindings, function arguments, bounded loops and recognized pipeline/process-substitution producers are modelled. Their stdout keeps branch alternatives separate from sequential output; `printf` replaces its input, while `cat` and `tee` forward modelled input. Known shell code and read targets remain checked alongside unknown output. Materialized consumer input exceeding 262,144 bytes receives an inspection-budget refusal, as does a word expansion with more than 16,384 fields or any expansion while a modelled value exceeds 262,144 bytes, so repeated doubling such as `a=$a$a` or `set -- "$@" "$@"` is refused within a few statements. Arbitrary runtime output (including file-mediated names such as `echo .env > l; xargs cat < l` and `cat $(cat l)`) and embedded program languages remain unmodelled. Generic option handling does not give every client-specific command string an execution model.
- Some long commands, mainly loops and pipelines with many keywords or array elements, exceed the inspection budget. The guard fails closed: it refuses the call and advises splitting it into smaller commands with explicit public paths. Conditional field accumulation over a finite literal list keeps each alternative without enumerating subsets. Contiguous or unbounded literal accumulators such as `s=public; while true; do s="$s/public"; done; cat "$s"` still reach the budget; repeated unknown read fragments are widened while their fixed path prefixes and suffixes stay in the protection check. A path or glob whose brace or extglob alternatives need more than 1,024 spellings, counting partial expansions (a product of nine two-member groups), is refused the same way rather than checked in part. In the 73,446-call real-usage replay, 29 calls that Go 0.6.0 permits are refused for this reason. This replay count does not bound all possible inputs.
- Command text still outside the model includes filenames printed into `xargs` by an unmodelled producer (`ls *.pem | xargs cat`), a value glued to a short option of an unknown program (`tool -f.env`), and `readonly` bindings (`readonly f=.env; cat $f`). Directory-history expansion (`OLDPWD=~/.ssh; cat ~-/x`) and the global physical-directory mode selected by `set -P` (`set -P; cat link/x`) are not tracked. A runtime-unknown previous directory (`OLDPWD=$d; cd -; cat x`) also remains unresolved; a known literal `OLDPWD` is followed by `cd -`.
- Docker build contexts (`docker build ~/.ssh`) and the values of `--build-context`, `--cache-from`, `--cache-to`, `--output`, `--ssh`, `--metadata-file`, and `--security-opt` are treated as names without file-access targets. Examples include `docker build --build-context x=~/.ssh .`, `docker build --ssh default=~/.ssh/id_rsa .`, and `docker build --metadata-file ~/Library/Containers/harness/x .`. Container command words in `docker compose run app cat .env` and `docker compose exec app cat .env` are not analysed as shell commands.
- Some client file settings remain unmodelled: the library path in `ssh -o SecurityKeyProvider=~/Library/Containers/harness/provider.dylib host`, the values of `git clone --reference ~/.ssh/repo url`, `git clone --template ~/.ssh url`, and `git clone --separate-git-dir ~/.ssh/repo url`, and the target of `git worktree add ~/.ssh/x`. Modelled stdin consumers include `curl -K -`, `curl -T -`, and `wget -i -`; protected input redirections and sensitive working directories are checked.
- A command that prints a secret it is allowed to read, such as `gcloud auth print-access-token`, has no path to check, so the guard lists the subcommands it knows and cannot list them all.

**State and resource identity** is which file a path names when the command runs, compared with when the guard checked it. Protected names are compared after the case folding the default APFS volume applies, so `~/.ßh` and `~/Library/Containerſ` name `~/.ssh` and `~/Library/Containers`. App Data traversal checks lexical protection before each probe and uses readlink alone. SSH identity comparisons additionally use stat/inode metadata after lexical checks; private-key spellings are decided without stat. For an unresolved shell operand or redirect, the fixed prefix may be resolved while the uncertain suffix is judged lexically. Working directories and iterator roots with an unresolved leading expansion, such as `cd "$d/app-link"; cat public` or `find "$d/app-link" -type f`, can lack a fixed path to probe. Moving or hard-linking protected credential, environment or SSH content to an unprotected name is refused, including `mv .env public-moved; cat public-moved`. Earlier commands still do not update the preflight filesystem model; a later call that reads a symbolic link resolves its target. Claude Code and Pi tool paths remove each `..` together with the name before it, then follow links, as those tools do, so `link/../x` names the `x` beside `link`; in a shell operand the kernel follows `link` first and `..` leaves its target. The working directory in the hook input, and a `..` that climbs above a relative tool path into it, keep kernel resolution. Child links followed by a recursive program (`rg -L CANARY .`, including a search glob through such a link, `rg -L -g 'link/x' CANARY .`) and a wildcard that expands to a link (`cat da*/x` when `data-link` leads elsewhere) are not exhaustively resolved; a literal link operand can still be resolved and denied.

The following retained boundaries were checked as event data with a synthetic HOME and public fixture files. The examples describe limits; they are not instructions to access real protected material.

macOS `/.nofollow` and `/.resolve/<device>` path prefixes are checked using the remaining absolute path, including when a symlink names one of these aliases. Inode-addressed `/.vol` paths are refused without probing them; name the file by its ordinary path and recheck the call.

The relocation check applies to `mv` and hard `ln` when the source is protected and the destination name is unprotected. Removal, renames between protected names, symbolic-link creation and copy-destination writes retain their original roles. Copy sources are still reads, so copying protected contents is denied. SSH write protection covers copy destinations and redirections into a private `.ssh` scope; moving or hard-linking a file onto such a name keeps the rename role and is permitted. Metadata maintenance (`chmod`, `chown`, `touch`, `stat`) and size or digest reads (`wc`, `file`, `shasum`, `md5`, `cksum`) remain permitted on credential paths because their output does not disclose file contents; App Data protection and existing SSH write protection apply independently. This distinction does not widen the guard's existing content-editing or client-use roles.

Private `.ssh` material is protected at any location, including aliases of the home SSH directory. Listing or reading the whole directory is refused because that scope includes private material. Exact public files (`config`, `config.*`, `*.pub`, `allowed_signers`, `known_hosts*`) remain permitted. For the home SSH directory and its aliases, a directory with one of those names is still a private search scope.

`/dev/fd/N` path operands belong to the executing process, not the checker. They remain unknown inherited inputs with limited preflight, without probing the checker's descriptors. Numeric redirection duplicates and moves retain modelled pipeline/process-substitution input routing and shared read positions. File redirections are checked independently, so a protected redirection is still denied.

| Boundary | Reproducer and observed scope |
| --- | --- |
| Dynamic item sources | `cat $(echo .env)`; `for f in $(printf '%s\n' .env); do cat "$f"; done`; `env $(echo cat .env)`; `bash -c "$(echo cat .env)"` remain uncovered dynamic-output forms. |
| Alias and sourced stdin | `alias c='cat .env'; c`; `source /dev/stdin <<< 'cat .env'` remain uncovered. |
| Filesystem mutations before reads | `ln -s .env public-link; cat public-link` does not update the preflight identity model. |
| Shell option changes | `shopt -s dotglob; cat *` remains uncovered; `setopt globdots; cat *` and `setopt CHASE_LINKS; cat link/x` are refused as unsupported executor syntax. |
| Embedded program languages | `sed 'r .env' public`; `sed 'e cat .env' public`; `awk 'BEGIN { getline < ".env" }'`; `awk 'BEGIN { system("cat .env") }'`; `vim -c 'read .env'`; `tmux new 'cat .env'`; `watch 'cat .env'` remain uncovered. |
| Client-defined command strings | `GIT_SSH_COMMAND='cat .env' git fetch`; `git -c core.pager='cat .env' log`; `git -c alias.x='!cat .env' x`; `ssh -o ProxyCommand='cat .env' host` remain uncovered execution contexts. |
| OS service reads | `defaults export com.example.app -` has no inferred protected read target. |
| Dynamic eval | `eval "val=\$$v"` is an unsupported dynamic rewrite; runtime-unknown command output is a separate unresolved-code state. |
| Executor divergence | `a=(public)#` retains Bash/Zsh divergence refusal. |
| Inline mentions | `python3 -c "print('~/.ssh/id_rsa')"` is refused although the spelling is data. |
| Redacted workdir | A replayed Codex transcript call (`shell_command`, `shell`) whose only working directory is `workdir: "__REDACTED__"` receives the explained relative-cwd refusal. Codex hooks deliver shell calls as `Bash` with the session `cwd`, so hook input does not carry these tool names. |
| Fresh temporary trees | `d=$(mktemp -d); cp public "$d/file"` is permitted with limited preflight because `mktemp` is an unmodelled program; the check does not establish its runtime-generated destination. |

The hook installs no operating system read policy. In a local 2x2 comparison recorded by the [read-enforcement experiment](../experiments/README.md), every run wrapped by `sandbox-exec` exited 71; a control in the sandboxed Codex environment failed with `sandbox_apply: Operation not permitted` before `/usr/bin/true` started. These are execution environment failures, not enforced read denials or proof that Seatbelt is unavailable on macOS 27. Nesting is the likely explanation, not a verified kernel denial record. [Claude Code](https://code.claude.com/docs/en/sandboxing) and [Codex's Seatbelt implementation](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt.rs) still use Seatbelt. The guard alone denied the direct operand and inline literal forms but allowed paths chosen inside an external script or runtime configuration; those reads remain outside a command text preflight check.

[Apple's container access rules](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox) tie TCC access to another app's container to the recognized client identity and the user's grant; an allowed app may access other app containers until it exits. The [read-enforcement experiment](../experiments/README.md) observed no prompt and did not establish whether the shell, hook or child was the responsible client. [App Sandbox](https://developer.apple.com/documentation/xcode/configuring-the-macos-app-sandbox) belongs to the owning app's entitlements, while [Endpoint Security](https://developer.apple.com/documentation/BundleResources/Entitlements/com.apple.developer.endpoint-security.client) requires its client entitlement.

macOS 27 [AppSettings privacy defaults](https://developer.apple.com/documentation/devicemanagement/appsettings) configure consent defaults, not arbitrary file read restrictions, and do not replace the administrator App Data policy described below.

## Administrator App Data policy

The [App Data profile generator](../cmd/agent-guard-profile/main.rs) creates an unsigned policy for an explicitly selected client and prints the exact inspection, MDM deployment, and test steps. Run this optional tool with Cargo from a checkout of the release being evaluated:

```sh
cargo run --bin agent-guard-profile -- --instructions
```

Follow the printed prerequisite, attribution, deployment, and test steps before applying a profile. They use [Apple's deployment requirements](https://support.apple.com/guide/deployment/privacy-preferences-policy-control-payload-dep38df53c2a/web) and the reviewed [PPPC schema](https://github.com/apple/device-management/blob/09f249a06e7e3289930bf6d05f38fb562f748ebf/mdm/profiles/com.apple.TCC.configuration-profile-policy.yaml). A successful installed-package check or plist validation does not prove OS enforcement.

## Development checks

Use the Rust toolchain pinned in `rust-toolchain.toml` (1.98.1) for the production runner, development tools and runtime harnesses. Cargo's exact parser pins preserve policy semantics, and `Cargo.lock` binds dependency resolution.

Install cargo-deny 0.20.2 before running the checks:

```sh
cargo install --locked --version 0.20.2 cargo-deny
```

Keep Cargo's executable directory on `PATH` so `cargo deny --version` reports `cargo-deny 0.20.2`. `cargo deny --locked check` fetches the RustSec advisory database and requires network access; advisory, license, ban and source checks remain enabled. CI installs the same version from the official arm64 macOS release archive and verifies its pinned SHA-256 before extraction.

Keep build outputs and evidence outside every checkout:

```sh
out=/absolute/path/outside/checkouts/agent-guard-evidence
export CARGO_TARGET_DIR="$out/cargo-target"
make check
make build OUT="$out/package"
cargo run --bin agent-guard-verify -- "$out/package/bin/agent-guard"
```

`make check` runs `make rust-check`: rustfmt, Clippy across all targets, locked Rust tests and cargo-deny. Set `CARGO` to an absolute executable path when absent from `PATH`. Rust tests check frozen contract fixtures for exact verdicts, public exit codes, denial text and advice.

The installed verifier requires the assembled `bin/agent-guard`, adjacent `agent-guard-native` and `VERSION`; it resolves the executable paths, records both hashes and checks that both executables report the package version. Require all 33 protocol cases to pass. It does not prove hook loading or all descendant cleanup.

The [synthetic runtime and lifecycle harnesses](../tests/harness/README.md) check the assembled entry and adjacent binary. Runtime verdicts, direct protocol checks and instrumented lifecycle checks are separate evidence. A runtime failure before hooks is not a guard denial. Check registration and runtime acceptance separately before replacing an installation.

## Release

This project remains in `0.x`; do not prepare `1.0.0` under the current policy. Use patch for internal, fix, dependency and documentation changes that leave user-visible behavior unchanged, and minor only for a real contract change such as a widened or narrowed scope of what the guard blocks or allows. The [release workflow](../.agents/skills/release/SKILL.md) owns authorization, version selection, preparation folding and tag safety.

`VERSION` is the single version owner. From a validated default-branch checkout, update it and write `docs/releases/<version>.md`, then create an annotated `v<version>` tag and push it only after the release gate clears.

Pushing the tag starts `publish.yml`, which checks the version and notes, runs the checks, creates the GitHub Release from the notes, then dispatches `agent-guard-release` to the tap. The formula builds that GitHub tag and records its commit revision. The tap uses Homebrew livecheck's `github_latest` strategy and `bump-formula-pr --write-only` for later updates, so a tag alone cannot trigger an update before the Release passes its checks. Verify the Release, tap revision and a built installation before declaring a release complete.

If a job fails, inspect the Release and tap state before retrying; an existing Release is reused and only its tap notification is repeated.
