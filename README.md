# Agent Guard

`agent-guard` checks Claude Code, Codex and Pi tool calls on macOS before they run. It blocks calls that would scan your home directory, open other apps' data or print credentials, and tells the agent what to do instead.

Agents search broadly by default. A `rg` or `find` started from `~` walks into other apps' containers under `~/Library`; macOS records each crossing as App Data access (`SystemPolicyAppDataDetailed`) and can answer with repeated permission prompts. The same habits read `.env` files, private keys or the whole environment into the agent's context.

## What a check looks like

The guard runs as a pre-tool hook. A blocked call never starts, and the agent receives the reason together with a safe alternative:

```text
$ rg -n TODO ~
DENIED: A scan rooted at the home directory or ~/Library reaches every app-data entry. Scope the scan to a project path. Do NOT bypass this restriction or retry the same blocked command.

$ cat .env
DENIED: This reads a credential or environment file. If a client the guard models only needs to use the file, pass it through that program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`; the guard does not control what the client does with the contents. Otherwise read a non-sensitive config file, or ask the user to inspect the file and share only the fact needed. Do NOT bypass this restriction or retry the same blocked command.
```

`rg -n TODO src` inside a project passes without output.

## What it blocks and allows

- **Blocks** walks rooted at `~` or `~/Library`, reads and listings of protected App Data, recursive searches that include hidden files, credential and environment file reads, and commands that dump environment variables.
- **Allows** project-scoped searches, named non-sensitive files, public SSH files such as `known_hosts`, and clients that use a credential file through their own option, such as `ssh -i`.
- **Fails closed.** A check that errors or runs out of time blocks the call. A passing check is not a permission grant; the runtime's own permission rules still apply.

## What it is not

The guard is a preflight check of the call's text, not a sandbox. It sees only calls that reach a registered hook, and it cannot decide reads that a program chooses while it runs, such as a script opening a file itself. Pair it with operating system read restrictions when those reads matter. The [known limits](docs/setup.md#known-limits) list what falls outside the check.

## Get started

Agent Guard needs Apple Silicon macOS, Homebrew and at least one of Claude Code, Codex or Pi. The [setup guide](docs/setup.md) covers installation, hook registration for each runtime, verification and removal.

Maintainers: see [development and release](docs/development.md).

## License

[MIT](LICENSE)
