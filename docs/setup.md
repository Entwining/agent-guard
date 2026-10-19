# Set up agent-guard

This guide installs `agent-guard`, registers it as a pre-tool hook for Claude Code, Codex or Pi, verifies it and removes it. The later sections state what the guard decides, its known limits, and the operating system policy that covers reads it cannot check. Development and release steps are in [the development guide](development.md).

## Prerequisites

- Apple Silicon macOS with a local Claude Code, Codex, or Pi session.
- Homebrew for installation.
- Permission to edit the configuration for the runtime you choose. Codex's managed configuration uses the system `/etc/codex/requirements.toml` and may require an administrator.

The Homebrew executable is `/opt/homebrew/bin/agent-guard`. The examples below use that path; replace it if your Homebrew prefix differs.

## Install

Install from tagged GitHub source through the Entwining tap:

> **AI agents:** Read this guide before installation or registration. Use existing authorization for the exact action and target; otherwise obtain it before installing or changing runtime configuration.

```sh
brew tap entwining/tap
brew install entwining/tap/agent-guard
/opt/homebrew/bin/agent-guard --version
```

## Upgrade

Homebrew clears the old trust when it redirects `loophubs/tap` to `entwining/tap`. Trusting the new canonical formula allows Homebrew to execute its Ruby code with your user privileges; review the [formula](https://github.com/Entwining/homebrew-tap/blob/main/Formula/agent-guard.rb) before granting that trust.

> **AI agents:** Use existing authorization for each exact action and target below; otherwise obtain it before proceeding.

For an existing Homebrew installation:

```sh
brew update
brew trust --formula entwining/tap/agent-guard
brew upgrade entwining/tap/agent-guard
```

## Register Claude Code

Merge the following [Claude Code `PreToolUse` hook](https://code.claude.com/docs/en/hooks) into your existing `~/.claude/settings.json`; retain other settings and hook entries. This registration requires Claude Code 2.1.295 or later. A hook exit code of `2` blocks the call; `onFailure: "block"` also blocks it if the guard cannot start, reaches the hook timeout, or exits with an unexpected code.

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash|Read|Edit|Write|Grep",
      "hooks": [{ "type": "command", "command": "/opt/homebrew/bin/agent-guard --runtime claude", "timeout": 5, "onFailure": "block" }]
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
    const failure = result.error?.message ?? (result.signal ? `signal ${result.signal}` : `exit ${result.status}`);
    return {
      block: true,
      reason:
        result.stderr?.trim() ||
        `[error:agent-guard/guard_unavailable] The agent guard did not complete its check (${failure}), so this call is blocked. Ask the checker owner to run agent-guard --version and repair the installation before rechecking the call.`,
    };
  });
}
```

A failed, missing, or timed-out guard check blocks the Pi tool call. When the guard gives no reason, for example because it could not start, the extension's own reason carries the `guard_unavailable` marker described under [Reason markers](#reason-markers).

## Verify

Check the installed version and an allowed synthetic event:

```sh
/opt/homebrew/bin/agent-guard --version
printf '%s\n' '{"tool_name":"Bash","tool_input":{"command":"ls"}}' | /opt/homebrew/bin/agent-guard --runtime codex
```

The event is checked, not executed. Require exit code `0` and no output for this event. This smoke check does not show that a runtime loaded its hook. Inspect the runtime's hook listing and confirm registration separately.

## Remove

First remove the entry you added from Claude Code's `PreToolUse` list, Codex's `/etc/codex/requirements.toml`, or Pi's `~/.pi/agent/extensions/agent-guard.ts`. Keep unrelated entries and requirements. Restart or reload the runtime and verify that its hook listing no longer contains the guard. For Codex, retain any existing `[features].hooks` setting needed by other hooks.

Then uninstall:

```sh
brew uninstall entwining/tap/agent-guard
```

## How the guard decides

The guard denies a call when its own check fails or times out because it cannot establish that the call is safe. Exit code `0` means the guard found no objection; it does not override the runtime's own permission rules.

Scans of broad filesystem roots are denied: an unscoped `rg`, `fd`, `ag`, `ack`, or `tree` run from the home directory is refused; pass an explicit project path. Hidden recursive content searches in shell commands are refused when an operand is or may be a directory, unless name filters bound the files the walk reads. `grep -r --include`, `rg --hidden -g`, and a `find` whose `-name` or `-path` test runs before its `-exec` action or before the names it prints into `xargs` read only files whose names end in the literal text after each filter's last wildcard, such as `*.ts` or `Makefile`. The guard compares those names, and each filter as written, with credential and environment file names at any depth, including inside hidden directories, so `--include='*.json'`, `--include='*.toml'` and `-name '.env*.local'` stay refused because they select files such as `auth.json`, `.cargo/credentials.toml` and `.env.local`. A walk stays unbounded when any positive filter has no literal text after its last wildcard (`*`, `s*`, `src/**`) or takes its value from the shell at run time (`--include="$pat"`), when it has only negative filters, `rg -t` types or `fd` options, or when a `find` expression tests names after its action, in only one branch of `-o`, under `!`, or with a primary the guard does not model. Output that `find` prints through `-exec` or a `-printf` format is unbounded. A consumer that walks the names it is handed walks below each one as it walks its own operands, so `find . -name '*.mjs' | xargs rg foo` is permitted while `xargs grep -r` over the same names, or `rg` over a credential directory a filter names, is refused. A named public file, including a resolved link to one, does not traverse hidden files and remains permitted with recursive flags. The guard checks lexical protection and resolves links before using file metadata to distinguish that file from a directory; metadata failures remain failed checks.

Private `.ssh` material is protected at any location, including aliases of the home SSH directory. Listing or reading the whole directory is refused because that scope includes private material. Exact public files (`config`, `config.*`, `*.pub`, `allowed_signers`, `known_hosts*`) remain permitted. For the home SSH directory and its aliases, a directory with one of those names is still a private search scope.

Moving or hard-linking a protected file to an unprotected name is refused: the relocation check applies to `mv` and hard `ln` when the source is protected and the destination name is unprotected. Removal, renames between protected names, symbolic-link creation and copy-destination writes retain their original roles. Copy sources are still reads, so copying protected contents is denied. SSH write protection covers copy destinations and redirections into a private `.ssh` scope; moving or hard-linking a file onto such a name keeps the rename role and is permitted. Metadata maintenance (`chmod`, `chown`, `touch`, `stat`) and size or digest reads (`wc`, `file`, `shasum`, `md5`, `cksum`) remain permitted on credential paths because their output does not disclose file contents; App Data protection and existing SSH write protection apply independently. This distinction does not widen the guard's existing content-editing or client-use roles.

Each refusal names what to do instead. Inspection-budget and deadline refusals ask for smaller calls with explicit public targets. Bounded or cyclic aliases require an ordinary absolute path. A malformed event sends the checker owner to update the guard for the runtime's hook event. If a single public-file check still fails, the checker owner can run `agent-guard --version` and repair the reported fault; none of these steps authorizes the blocked operation.

The serialized event is limited to 262,144 bytes, and shell substitutions/groups to 64 simultaneous nested delimiters. Excess input or nesting, a relative working directory, and invalid UTF-8 are refused with a reason that asks the caller to shorten or split the request, supply an absolute working directory, or encode UTF-8. Malformed events, filesystem probe faults and other operational errors remain failed checks; the hook blocks both kinds with exit status `2`. The entry deadline is 3 seconds, below the example consumer timeout of 5 seconds. A cancelled check stops and reaps its child processes before the hook returns.

## Reason markers

Every blocking reason produced by the guard or the documented Pi extension starts with a marker that names who blocked the call and why, so a session log or index can attribute the decision without matching its wording:

```text
[policy:agent-guard/app_data] DENIED: This reads a protected macOS app-data directory. …
```

Hook failures blocked by Claude Code's `onFailure` setting use Claude Code's own diagnostic, outside the guard's marker contract.

The marker is `[<class>:agent-guard/<id>]`, followed by one space and the reason. For Claude Code, a reason the checker gives after reading the event keeps its `DENIED:` prefix after the marker; every other reason, such as a usage or shell entry failure, follows the marker directly. The class is the disposition:

- `policy`: the call reaches protected material; the reason names a safe alternative.
- `coverage`: the guard cannot inspect the call completely enough to permit it; rewriting the call can make it checkable.
- `error`: the check itself failed or ran out of time; the call is blocked without a verdict.

The ID names the checked condition. A condition that can block in more than one class keeps one ID, so a class and ID pair identifies a reason. An ID stays the same when only the wording changes; a materially different condition or recovery receives a new ID, and a retired ID is never reused. Advice that does not block a call carries no marker.

| ID | Class | Condition |
| --- | --- | --- |
| `resource_change` | policy | Moving or hard-linking protected content to an unprotected name |
| `app_data` | policy | Reading a protected macOS app-data directory |
| `broad_scan` | policy | A scan rooted at the home directory or `~/Library` |
| `credential_file` | policy | Reading a credential or environment file |
| `credential_store` | policy | Searching a directory that holds a client's credential store |
| `inline_credential_file` | policy | Inline code naming a credential or environment file |
| `hidden_search` | policy | A recursive search that can reach hidden credential files |
| `variable_dump` | policy | Dumping environment or shell variables |
| `credential_variable` | policy | Printing a credential variable |
| `git_hosting_token` | policy | Printing a Git hosting token |
| `keychain` | policy | Extracting a password from the macOS Keychain |
| `stored_secret` | policy | Printing a stored secret or access token |
| `curl_trace` | policy | curl verbose or trace output that can print request headers |
| `credential_upload` | policy | Sending a credential file's contents |
| `ssh_private` | policy | Reading private material in an `.ssh` directory or its alias |
| `grep_ssh_private` | policy | A Grep tool search over private `.ssh` material |
| `input_byte_limit` | coverage | The event exceeds the input byte limit |
| `nesting_limit` | coverage | Shell nesting exceeds 64 levels |
| `absolute_cwd_required` | coverage | The event has no absolute working directory |
| `invalid_encoding` | coverage | The event is not valid UTF-8 |
| `unsupported_shell_syntax` | coverage | Shell syntax the guard cannot inspect |
| `executor_divergence` | coverage | Syntax or shell options that a shell the consumer may run would read differently from the guard's model |
| `identity_bound` | coverage | A path reached through bounded or cyclic aliases |
| `inode_alias` | coverage | An inode-addressed `/.vol` path |
| `inspection_budget` | coverage | A command that exceeds the inspection budget |
| `malformed_input` | error | An event that is not a complete JSON event with the documented tool fields |
| `probe_fault` | error | A filesystem metadata check failed |
| `guard_fault` | error | An internal checker fault |
| `deadline` | error | The check did not finish before its deadline |
| `usage` | error | The hook command line is not a supported invocation |
| `relative_home` | error | `HOME` is not an absolute path |
| `entry_unreadable` | error | The entry cannot resolve its own path |
| `runner_missing` | error | The package directory or its native runner is missing or not usable |
| `checker_incomplete` | error | The checker ended without a verdict |
| `reason_missing` | error | The checker blocked the call without a reason |
| `guard_unavailable` | error | The [Pi extension](#register-pi) received no reason from the guard |

## Known limits

The guard is a bounded preflight check: it decides from the targets it infers under modelled command semantics. Operating system read restrictions must cover the reads these limits leave out. The examples describe limits; they are not instructions to access real protected material.

### Calls that reach the guard

A disabled, skipped, or unregistered hook cannot inspect a call. The documented Claude Code registration and Pi extension block guard launch failures and timeouts; a runtime that treats those failures as non-blocking lets the call proceed unchecked. Custom tools and processes outside the registered runtime are not checked, and `Glob` is covered by neither the example matchers nor the guard's input handlers.

Tool paths are checked as the consumer resolves them: Claude Code reads a Grep `file_path` as its `path` when `path` is absent and trims surrounding whitespace, and Pi replaces Unicode spaces and drops a leading `@`. A Claude Code Write with only `path` is refused as malformed input. A `file:` spelling in a Claude Code or Pi tool path is checked as a relative name; Pi itself decodes a lowercase `file://` URL into the path it names, so such a Pi tool path is not checked as that path. Codex's hook input omits the `workdir` an `exec_command` call runs in ([openai/codex#33986](https://github.com/openai/codex/issues/33986)), so relative paths in Codex Bash calls are checked against the session `cwd`.

### What a command does when it runs

- The guard treats a program it does not know as reading every path it is handed, so `aws s3 cp .env s3://bucket/x`, `open .env`, and `python3 script.py .env` are denied. Pass a credential file through the program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`, which the guard allows for the clients its program table models. A file the client uses itself, such as `ssh -i`, `docker run --env-file` or `uv run --env-file`, is allowed by design; the guard does not control what the client, or a command it runs with the loaded values, does with its contents, beyond denying the commands on the dump-command list.
- Launchers that run another command (`uv run`, `uv tool run`, `uvx`, `npm exec`, `npm x`, `npx`, `pnpm exec`, `pnpm dlx`, `bunx`, `bun x`, `op run` and `dotenv`) are read from their documented options. A file the launcher loads itself, and a package source, index or interpreter it may take from a local path, is used rather than read. The inner command is checked as its own program, and a script that `uv run` runs with Python (after `--script`, or a name ending in `.py` or `.pyw`) is read like an interpreter's script. `uv --directory`, `pnpm -C` and `env -C` enter their directory, which becomes the inner command's working directory, while redirections stay in the shell's. An option outside a launcher's table, a directory the shell expands at run time, a launcher form without an inner command (`xargs uv run`), an option after the inner command of a launcher that keeps parsing its own options (`npm exec`, `op run`, `dotenv`) without `--`, and a package name that stands for a differently named binary (`npx dotenv-cli`) leave the launcher an unknown program. `dotenv -p NAME` prints a value from the `.env` it loads implicitly, which has no path operand to check.
- Cargo's builtin subcommands treat `--root`, `--target-dir`, `--artifact-dir` and the operand of `cargo new`, `init` or `vendor` as write targets, `--manifest-path`, `--lockfile-path`, `--path` and `--sync` as reads, `--config`, `--git`, `--index` and `--target` as files cargo uses, and other operands and option values as names; the operands of `cargo run`, arguments after `--` and third-party subcommands are unknown-program operands. Nightly `cargo -C DIR` is checked as entering `DIR`, but later relative arguments are resolved against the original directory.
- The query verbs of `diskutil` and `tmutil` read the metadata of the volumes and backups they name, and those of `dscl` name directory records; their other verbs are unknown programs.
- Git pathspec magic is read for the path it names. `:(top)` and `:/` paths are resolved against the working directory rather than the repository root, `:(icase)` and `:(attr:...)` do not narrow the checked path, and exclusions (`:(exclude)`, `:!`, `:^`) select nothing, so a pathspec list of exclusions alone is checked like a git command without a path operand.
- A read whose target the program picks while it runs cannot be decided before execution: an interpreter opening a file itself, `git diff`, `git log -p`, or `git show` without a path operand, and a walk that reaches credential files it does not treat as hidden (such as `*.pem` under `rg`, `fd -x`, `tar`, or `cp -r`). An interpreter that chooses to print process environment values, such as Python code that prints `os.environ`, is also outside the static dump-command list. A read through an operating system service, such as `defaults export com.example.app -`, has no inferred protected read target.
- Code and names produced at run time are not inspected:
  - command output used as names or code, such as `cat $(echo .env)`, `for f in $(printf '%s\n' .env); do cat "$f"; done`, `env $(echo cat .env)` and `bash -c "$(echo cat .env)"`, including file-mediated names such as `echo .env > l; xargs cat < l` and `cat $(cat l)`, and filenames printed into `xargs` by an unmodelled producer (`ls *.pem | xargs cat`);
  - aliases and sourced stdin, such as `alias c='cat .env'; c` and `source /dev/stdin <<< 'cat .env'`;
  - embedded program languages, such as `sed 'r .env' public`, `sed 'e cat .env' public`, `awk 'BEGIN { getline < ".env" }'`, `awk 'BEGIN { system("cat .env") }'`, `vim -c 'read .env'`, `tmux new 'cat .env'` and `watch 'cat .env'`;
  - command strings a client defines, such as `GIT_SSH_COMMAND='cat .env' git fetch`, `git -c core.pager='cat .env' log`, `git -c alias.x='!cat .env' x` and `ssh -o ProxyCommand='cat .env' host`; generic option handling does not give every client-specific command string an execution model;
  - destinations a program generates at run time, so `d=$(mktemp -d); cp public "$d/file"` is permitted with limited preflight.

  Literal bindings, function arguments, bounded loops and recognized pipeline/process-substitution producers are modelled. Their stdout keeps branch alternatives separate from sequential output; `printf` replaces its input, while `cat` and `tee` forward modelled input. Known shell code and read targets remain checked alongside unknown output.
- A wildcard without credential-identifying fixed text, such as `cat *`, `cat s*`, or a Grep glob `**/*.ts`, is not intersected with the credential catalog, so it can expand to a protected file. Ordinary source globs such as `**/*.{ts,tsx,js}` remain permitted under a public project root. Explicit sensitive patterns such as `*.pem`, `.env*`, `**/.aws/credentials` and `**/.ssh/id_*` are still denied; the guard does not list directories to determine wildcard expansions. A pattern with more fixed text after such a wildcard can still expand to a protected file: `cat .env*.txt` is permitted although `cat .env.txt` is denied. Without listing, the stored spelling is unknown, so a bracket class matches either ASCII case of a protected name: `cat ~/.[!s]sh/id_rsa` is denied as if the directory could be spelled `.Ssh`. Inside a bracket class only `[:name:]` classes are read; Bash `[=x=]` equivalence classes and `[.x.]` collating symbols are ordinary class characters, so `cat ~/.[[.s.]]sh/id_rsa` is not matched to `~/.ssh`.
- A generic file name such as `config.json`, `hosts.yml` or `credentials` identifies a credential only together with its directory, so a wildcard pattern must spell one of those directories, or start a component with a dot where the directory is hidden. A positive filter glob only narrows its search root, so under a public project root a Grep glob `*/config.json`, `**/config.json` or `credentials` is permitted, as the same search without a glob is, while `.config/*/config.yml`, `.*/*/config.yml` and `.aws/**/credentials` are denied. A shell walk narrowed by name filters is checked by name at any depth instead, so `grep -r --include=credentials` is denied.
- A client's credential store is also protected where the client reads it: in its default directory (`~/.aws`, `~/.docker`, `~/.kube`, `~/.gnupg`, Cargo's `~/.cargo`, gh's `~/.config/gh`, glab's `~/.config/glab-cli` and `~/Library/Application Support/glab-cli`), in the directory `CARGO_HOME`, `GH_CONFIG_DIR`, `GLAB_CONFIG_DIR` or `XDG_CONFIG_HOME` names in the hook's own environment, following each client's precedence, and wherever symbolic links lead a default directory. Reading its credential files, the store as a whole, or a directory that holds it below visible names, including one a wildcard names, is refused: `cat "$CARGO_HOME"/*`, `rg token "$XDG_CONFIG_HOME"`, `rg token ~/Library/Application\ Support` and `rg token ~/.config/*` are denied, and when `~/.config` links to `~/dotfiles/config`, so are `cat ~/dotfiles/config/gh/hosts.yml` and `rg token ~/dotfiles`.
- Name filters narrow a walk of a directory that holds a store. When a walk's filters bound the file names it reads, as described in [How the guard decides](#how-the-guard-decides), or a Grep call's globs do (its `type` does not), the walk is refused only if a filter matches a store's credential file name, ignoring case, or the walk reaches a `private-keys-v1.d` directory, whose keys have arbitrary names: `rg -g '*.lua' foo ~/.config` and `grep -r --include='*.lua' foo ~/.config` are permitted, while `rg -g '*.yml' foo ~/.config` is not. A directory whose name marks it as a credential directory, such as `~/.aws`, stays refused whatever the filters; where a link leads it to a directory without that name, such as `~/dotfiles/aws`, only its store's credential files are protected there, for filtered walks as for reads by path.
- A store variable is read from the environment the hook's consumer was started with, which can differ from the command's: a value set only in a shell profile the consumer did not load, or assigned within the command, is not used. Other variables that move a store, such as `DOCKER_CONFIG`, `GNUPGHOME`, `KUBECONFIG` and `AWS_SHARED_CREDENTIALS_FILE`, are not read. In a relocated directory that nothing above describes, including where a link leads a variable's directory and a default directory reached only through bounded or cyclic aliases, only a name that identifies a credential with its directory is protected, so `cat ~/xdg/glab-cli/config.yml` is denied while `cat ~/xdg/glab-cli/config.*` and `cat ~/xdg/gh/hosts.yml` are not. Add such a directory to the consumer's read restrictions as well, such as Claude Code's `sandbox.filesystem.denyRead` for shell commands and a `Read(...)` deny rule for its file tools.
- Cargo reads a project's `.cargo` directory as configuration and its credentials only from its home, so a project's `.cargo` is not a store: `rg token .cargo` and `git diff -- .cargo` in a project are walks like any other. Cargo's credentials file names stay protected in every `.cargo`, so `cat .cargo/*.toml` is refused while `cat .cargo/config.toml` is not.
- Positive Grep tool globs and shell search globs (`rg -g`, `--glob`, `--iglob`, `grep --include`) without a path separator match basenames at any depth under the search root. Globs containing a separator keep their complete relative path; a glob with a `..` component selects nothing, and negative globs do not add read targets. Without `--hidden`, ripgrep still enters each hidden file or directory a positive glob matches. The guard therefore checks hidden names when any glob's last component can match a dot name: `rg -g '*.npmrc' .` and `rg -g .aws -g credentials .` are denied, while `rg -g credentials .` is permitted because ripgrep does not enter `.aws` for it. A file one glob selects is checked below each hidden directory another glob enters, so `rg -g .config -g hosts.yml .` and `rg -g '*' -g credentials .` are denied, while `rg -g '*.ts' -g credentials .` is permitted because `*.ts` does not enter `.aws`. The Grep tools and `grep --include` always reach hidden names. Claude Code's Grep splits its `glob` at whitespace, then at commas in any piece without both `{` and `}`, and passes each piece as its own `--glob`, so each positive piece is checked as a filter and `!*.jsonl, .env` still reads `.env`; Pi's Grep passes the whole value as one `--glob`.
- The Claude Code and Pi Grep tools run ripgrep with `--hidden`, so a Grep call under a public root also searches hidden files its ignore rules do not exclude, such as an untracked `.env` outside a Git repository. A shell search glob whose last component matches any name, such as `rg -g '*' .` or `rg -g 'src/**' .`, does the same. The guard checks the root and glob, not the hidden descendants of a public root; a shell `rg --hidden` over a directory is refused unless its globs bound the file names.
- A walk narrowed by name filters can still read:
  - a file its filter names inside a nested private `.ssh` directory, or a `private-keys-v1.d` directory outside the stores above, below the search root, such as `--include=id_rsa`;
  - a credential or environment variant that only a wildcard selects, such as `auth.json.bak` under `--include='*.bak'` or `.env.local` under `--include='*.local'`;
  - names that a command between `find` and `xargs` rewrites, such as `sed`;
  - a name containing whitespace, which `xargs` splits into separate paths;
  - a credential directory that only a wildcard selects for a consumer that walks it, such as `find . -name '.a*' | xargs rg foo`.
- Some shell state is not evaluated, so the paths it computes are not checked:
  - `read`, `mapfile` and `readarray` bind runtime-unknown values, so `read -r f <<< .env; cat "$f"` gets limited preflight. `readonly` bindings (`readonly f=.env; cat $f`) are not tracked.
  - Arithmetic follows command substitutions written directly in the expression or in quoted `let` text, but not one nested inside another parameter expansion (`let 'a[${x:-$(cat .env)}]'`) or a variable value that contains code (`a='b[$(cat .env)]'; echo $((a))`).
  - Zsh parameter modifiers such as `${f:r}` are not applied, so the modified word gets limited preflight.
  - `CDPATH`, zsh `cdpath`, `cd -` and zsh's two-operand `cd old new` are not followed; `cd` takes its first operand as written, as in `CDPATH=~/Library; cd Containers; cat x/data` and `OLDPWD=~/.ssh; cd -; cat id_rsa`. A tracked `cd` does not record the previous directory, so `$OLDPWD` afterwards is runtime-unknown (`cd ~/.ssh && cd /tmp && cat "$OLDPWD/id_rsa"`). Directory-history expansion (`OLDPWD=~/.ssh; cat ~-/x`), the global physical-directory mode selected by `set -P` (`set -P; cat link/x`) and zsh `hash -d` named directories (`hash -d c=~/Library/Containers; cat ~c/x/data`) are not tracked either.
  - Shell options that change expansion are not tracked: `shopt -s dotglob; cat *` is not covered, while `setopt globdots; cat *` and `setopt CHASE_LINKS; cat link/x` are refused as unsupported executor syntax.
  - `GIT_DIR` and `GIT_WORK_TREE` do not redirect a git command's repository (`GIT_DIR=~/Library/Containers/x git log`), while `--git-dir` and `-C` operands are checked.
  - `env`, `xargs` and the launchers above are unwrapped only as shell command prefixes; inside another program's command list, such as `find -exec` or `fd -x`, they are unknown programs that read every operand, and the command they would run, including `sh -c` code and `env -S` strings, is not inspected (`find . -exec env sh -c 'cat .env' \;`). A prefix is recognized by its file name, so one run from a path inside App Data (`~/Library/Containers/x/uv run app.py`) is unwrapped without checking its own path, and one that enters another directory is not checked at the directory it starts in (`cd ~/Library/Containers/x && uv run --directory /tmp app.py`).
  - A value glued to a short option of an unknown program (`tool -f.env`) is not read as a path.
- Docker build contexts (`docker build ~/.ssh`) and the values of `--build-context`, `--cache-from`, `--cache-to`, `--output`, `--ssh`, `--metadata-file`, and `--security-opt` are treated as names without file-access targets. Examples include `docker build --build-context x=~/.ssh .`, `docker build --ssh default=~/.ssh/id_rsa .`, and `docker build --metadata-file ~/Library/Containers/harness/x .`. Container command words in `docker compose run app cat .env` and `docker compose exec app cat .env` are not analysed as shell commands.
- Some client file settings remain unmodelled: the library path in `ssh -o SecurityKeyProvider=~/Library/Containers/harness/provider.dylib host`, the values of `git clone --reference ~/.ssh/repo url`, `git clone --template ~/.ssh url`, and `git clone --separate-git-dir ~/.ssh/repo url`, and the target of `git worktree add ~/.ssh/x`. Modelled stdin consumers include `curl -K -`, `curl -T -`, and `wget -i -`; protected input redirections and sensitive working directories are checked.
- A command that prints a secret it is allowed to read, such as `gcloud auth print-access-token`, has no path to check, so the guard lists the subcommands it knows and cannot list them all.

### Which file a path names

These limits concern which file a path names when the command runs, compared with when the guard checked it.

- Protected names are compared after the case folding the default APFS volume applies, so `~/.ßh` and `~/Library/Containerſ` name `~/.ssh` and `~/Library/Containers`.
- App Data traversal checks lexical protection before each probe and uses readlink alone. SSH identity comparisons additionally use stat/inode metadata after lexical checks; private-key spellings are decided without stat.
- For an unresolved shell operand or redirect, the fixed prefix may be resolved while the uncertain suffix is judged lexically. Working directories and iterator roots with an unresolved leading expansion, such as `cd "$d/app-link"; cat public` or `find "$d/app-link" -type f`, can lack a fixed path to probe.
- Earlier commands do not update the preflight filesystem model, so `ln -s .env public-link; cat public-link` is not decided as a read of `.env`; a later call that reads the link resolves its target. Moving or hard-linking protected content to an unprotected name is refused instead, including `mv .env public-moved; cat public-moved`.
- Claude Code and Pi tool paths remove each `..` together with the name before it, then follow links, as those tools do, so `link/../x` names the `x` beside `link`; in a shell operand the kernel follows `link` first and `..` leaves its target. The working directory in the hook input, and a `..` that climbs above a relative tool path into it, keep kernel resolution.
- Child links followed by a recursive program (`rg -L CANARY .`, including a search glob through such a link, `rg -L -g 'link/x' CANARY .`) and a wildcard that expands to a link (`cat da*/x` when `data-link` leads elsewhere) are not exhaustively resolved; a literal link operand can still be resolved and denied.
- macOS `/.nofollow` and `/.resolve/<device>` path prefixes are checked using the remaining absolute path, including when a symlink names one of these aliases. Inode-addressed `/.vol` paths are refused without probing them; name the file by its ordinary path and recheck the call.
- `/dev/fd/N` path operands belong to the executing process, not the checker. They remain unknown inherited inputs with limited preflight, without probing the checker's descriptors. Numeric redirection duplicates and moves retain modelled pipeline/process-substitution input routing. File redirections are checked independently, so a protected redirection is still denied.

### Conservative refusals

The guard refuses these calls because it cannot establish that they are safe, even when the command names no protected file.

- Materialized consumer input exceeding 262,144 bytes receives an inspection-budget refusal, as does a word expansion with more than 16,384 fields or any expansion while a modelled value exceeds 262,144 bytes, so repeated doubling such as `a=$a$a` or `set -- "$@" "$@"` is refused within a few statements.
- Some long commands, mainly loops and pipelines with many keywords or array elements, exceed the inspection budget; the refusal advises splitting them into smaller commands with explicit public paths. Conditional field accumulation over a finite literal list keeps each alternative without enumerating subsets. Contiguous or unbounded literal accumulators such as `s=public; while true; do s="$s/public"; done; cat "$s"` still reach the budget; repeated unknown read fragments are widened while their fixed path prefixes and suffixes stay in the protection check. A path or glob whose brace or extglob alternatives need more than 1,024 spellings, counting partial expansions (a product of nine two-member groups), is refused the same way rather than checked in part.
- Data arguments can be read as paths: `git log --grep .key`, `yq -o json '.key' data.yaml`, `sed -n '/\.key/p' notes.txt` and `jaq '.[].key' data.json` are refused although these arguments name no file, while `git log --grep=.key`, `yq -o=json '.key' data.yaml`, `grep '\.key' notes.txt` and `jq '.[].key' data.json` are permitted. From the first word whose argument count the check cannot establish, every argument is checked as a path: an unquoted pattern such as `jq .[0].key data.json`, an unquoted expansion such as `$r` in `jq --arg r $r '.[].key' data.json`, or a list such as `"$@"` in `f() { jq "$@" data.json; }; f .key`. Quoting a single value or filter keeps it data.
- Interpreter source passed on the command line, such as `python3 -c`, `node -e` or `ruby -e` code, is refused when it mentions a protected name, although the spelling may be data, as in `python3 -c "print('~/.ssh/id_rsa')"`.
- A word that Bash and Zsh expand differently is refused, as in `a=(public)#`, and so is a dynamic rewrite such as `eval "val=\$$v"`; command output whose content is unknown remains a separate, unresolved state.

## Operating system read restrictions

The hook installs no operating system read policy. In the [read-enforcement experiment](../experiments/README.md), the guard alone denied the direct operand and inline literal forms but allowed paths chosen inside an external script or runtime configuration; those reads remain outside a command text preflight check and need an operating system restriction.

The runtimes' own sandboxes are one such restriction: [Claude Code](https://code.claude.com/docs/en/sandboxing) and [Codex's Seatbelt implementation](https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt.rs) use Seatbelt. In the experiment's local 2x2 comparison, every run wrapped by `sandbox-exec` failed before its command started, including a control inside the sandboxed Codex environment. These are execution environment failures, likely from nesting, not enforced read denials or proof that Seatbelt is unavailable on macOS 27.

[Apple's container access rules](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox) tie TCC access to another app's container to the recognized client identity and the user's grant; an allowed app may access other app containers until it exits. The experiment observed no prompt and did not establish whether the shell, hook or child was the responsible client. [App Sandbox](https://developer.apple.com/documentation/xcode/configuring-the-macos-app-sandbox) belongs to the owning app's entitlements, while [Endpoint Security](https://developer.apple.com/documentation/BundleResources/Entitlements/com.apple.developer.endpoint-security.client) requires its client entitlement. macOS 27 [AppSettings privacy defaults](https://developer.apple.com/documentation/devicemanagement/appsettings) configure consent defaults, not arbitrary file read restrictions.

For managed Macs, the [App Data profile generator](../cmd/agent-guard-profile/main.rs) creates an unsigned policy for an explicitly selected client and prints the exact inspection, MDM deployment, and test steps. Run this optional tool with Cargo from a checkout of the release being evaluated:

```sh
cargo run --bin agent-guard-profile -- --instructions
```

Follow the printed prerequisite, attribution, deployment, and test steps before applying a profile. They use [Apple's deployment requirements](https://support.apple.com/guide/deployment/privacy-preferences-policy-control-payload-dep38df53c2a/web) and the reviewed [PPPC schema](https://github.com/apple/device-management/blob/09f249a06e7e3289930bf6d05f38fb562f748ebf/mdm/profiles/com.apple.TCC.configuration-profile-policy.yaml). A successful installed-package check or plist validation does not prove OS enforcement.
