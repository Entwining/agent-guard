# Agent Guard

`agent-guard` is a macOS pre-tool guard for Claude Code, Codex, and Pi. It checks supported tool calls before execution and blocks risky filesystem scans and credential reads.

macOS can record App Data access as `SystemPolicyAppDataDetailed` when a command reads or enumerates another app's data under `~/Library`. Broad scans from the home directory can cross those containers and trigger repeated permission prompts.

## What it does

The guard blocks broad home and `~/Library` walks, protected App Data access, and recognizable attempts to display credentials. It allows project-scoped searches and named non-sensitive file reads; a clean guard result still leaves the runtime's own permissions in force. It covers only calls routed through a registered hook, not every tool or process.

The guard is implemented in Go and distributed through the Homebrew tap from tagged GitHub source. It has no Node.js or Bun runtime dependency. The legacy `@loophubs/agent-guard` npm package is retired; existing npm installations should migrate to Homebrew.

To install the package, migrate an existing installation, and register and verify a runtime, follow [the setup guide](docs/setup.md).

[GitHub Releases](https://github.com/LoopHubs/agent-guard/releases) describe version-specific changes to what the guard blocks or allows.

> **AI agents:** Read `docs/setup.md` first, then ask your user whether to install and register the guard. Do not install it or change configuration without consent.
