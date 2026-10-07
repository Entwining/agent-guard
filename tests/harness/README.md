# Synthetic hook evaluation

The Go development harness runs the Rust package on Apple Silicon macOS. Build an assembled package as described in [the setup guide](../../docs/setup.md#development-checks). All evidence directories must be new and outside Git checkouts. The harness uses temporary homes, synthetic files, and scripted model servers bound to loopback. It does not use live model credentials or change the user's runtime configuration.

## Runtime registration and attribution

Build the runtime driver outside the checkout, then select the installed Claude Code, Pi, and Codex clients from `PATH`:

```sh
go build -o "$out/agent-guard-runtime" ./cmd/agent-guard-runtime
"$out/agent-guard-runtime" --source "$PWD" --entry "$out/package/bin/agent-guard" --output "$out/runtime" --runtimes claude,pi,codex
```

Each selected runtime has ten synthetic cases, repeated three times. The driver records the client's resolved path, executable hash and version, the copied guard entry and native binary hashes, and the source manifest. `records.jsonl` retains each completed attempt; `report.json` contains the summary and records. Hook status and output are separate from the runtime tool result. Missing hooks, mismatched commands, missing execution witnesses and conflicting evidence prevent a complete result. If startup fails before the first model request or hook, the driver stops that runtime and reports the attempted count separately from the planned 30 calls.

The Claude Code and Pi drivers use an Anthropic Messages loopback server. Pi's JavaScript extension is generated only inside the synthetic home and invokes the Go harness adapter, which forwards the event to the assembled Rust package. Codex uses a Responses loopback server and synthetic hook configuration. A runtime failure before hooks remains unverified, not a guard denial.

Run the no-guard control separately with the same package and clients:

```sh
"$out/agent-guard-runtime" --source "$PWD" --entry "$out/package/bin/agent-guard" --output "$out/runtime-without-guard" --runtimes claude,pi,codex --ablate
```

Every control case must execute, including the synthetic protected canaries. Compare the recorded identities before comparing reports. The measured hook duration excludes runtime startup; cold runtime durations include startup variability. These reports observe the entry PID only. They do not establish cleanup of runner, checker, watchdog or other descendants.

Before runtime acceptance, require 30 verified, matched calls per selected client in both runs, record client versions and executable hashes, and count `child setpgid` lines in recorded stdout/stderr. The count must be zero; a nonzero count requires investigation of the wrapper. A missing executable or failure before hooks does not establish a zero-warning result for that client. Keep host process-group or loopback refusals with the exact command and error, and rerun on an authorized host into fresh evidence directories; do not modify the harness to avoid the operation.

## Instrumented lifecycle

The lifecycle driver copies the Rust source into its evidence directory and injects faults there. Its fault module is embedded as test data in the Go driver and never compiled into the release runner. It does not edit the checkout. Set the Cargo, Rustup, Go module and build caches outside the checkout and populate them with ordinary development checks first; copied fault builds use locked, offline Cargo dependencies.

```sh
go build -o "$out/agent-guard-lifecycle" ./cmd/agent-guard-lifecycle
"$out/agent-guard-lifecycle" --source "$PWD" --cargo "$(command -v cargo)" --output "$out/lifecycle"
```

Fifteen faults run three times each: pipe input, delayed startup, startup stall, large stdout/stderr denial reasons, slow or absent reasons, checker failure and panic, partial output before panic, hung descendants, stalled or failed supervisor, leftover child, and filesystem dependency failure. Instrumented children record their own PID and process group. The driver samples survivors before its cleanup, records full output, verifies the large-output producer independently, and requires the guard's total deadline and failure contract.

The following negative controls must each exit unsuccessfully with three contract violations. Use a distinct output directory for every invocation:

```sh
"$out/agent-guard-lifecycle" --source "$PWD" --cargo "$(command -v cargo)" --output "$out/control-drain" --control drain
```

Available controls are `drain`, `stderr-drain`, `failclosed`, `deadline`, `cleanup`, and `dependency`. `results.jsonl` records each observation and its violations; `summary.json` binds the source and counts failures. Rerun the unmodified lifecycle driver into a new directory after the controls to establish recovery. These instrumented copies prove the named lifecycle contracts, not all possible runtime descendants.

The local harness contracts run with `go test -race ./tests/harness`. They exercise the HTTP protocols and CLI boundary with scripted clients, byte-preserving hook forwarding, timeout cleanup, report path isolation, full runtime case counts and missing-evidence failures. Actual client acceptance requires the separate runtime invocation above.
