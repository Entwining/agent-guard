# Agent Guard

Agent Guard checks Claude Code, Codex and Pi tool calls on macOS before they run. It blocks home-directory scans and credential reads, then tells the agent what to do instead.

A scan from `~` enters other apps' data under `~/Library`. macOS records this as App Data access. This can trigger repeated permission prompts. Broad reads can pull `.env` files, private keys or the whole environment into the agent's context.

## Keep searches inside the project

The guard blocks scans rooted at `~` or `~/Library` before the tool runs:

```text
$ rg -n TODO ~
[policy:agent-guard/broad_scan] DENIED: A scan rooted at the home directory or ~/Library reaches every app-data entry. Scope the scan to a project path.
```

Project-scoped searches such as `rg -n TODO src` [pass without output](docs/setup.md#example-decisions).

## Use credentials without printing them

The guard [blocks credential reads, environment dumps and recursive searches that can reach hidden credential files](docs/setup.md#how-the-guard-decides). Clients can still use credential files through modelled options such as `ssh -i`. The guard does not control what the client does with the file's contents.

## Fail closed

A check that errors or times out [blocks the tool call](docs/setup.md#how-the-guard-decides). A passing check leaves the runtime's permission rules in force.

## Get started

Requires Apple Silicon macOS and Homebrew. [Install Agent Guard and register its hook](docs/setup.md) for Claude Code, Codex or Pi.

## Limits

The guard is a text preflight check, not a sandbox. It only sees [calls that reach a registered hook](docs/setup.md#known-limits). Use [operating system read restrictions](docs/setup.md#operating-system-read-restrictions) for reads a script chooses at runtime.

## Contributing

Contributors [build, test and release the Rust implementation](docs/development.md) from a source checkout.

## License

Agent Guard is available under the [MIT license](LICENSE).
