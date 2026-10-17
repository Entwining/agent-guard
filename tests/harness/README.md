# Synthetic hook evaluation

The development harness runs the assembled package on Apple Silicon macOS. Build an assembled package as described in [the development guide](../../docs/development.md#development-checks). All evidence directories must be new and outside Git checkouts. The harness uses temporary homes, synthetic files, and scripted model servers bound to loopback. It does not use live model credentials or change the user's runtime configuration.

## Runtime registration and attribution

Run the runtime driver from the checkout root; it selects the installed Claude Code, Pi, and Codex clients from `PATH`:

```sh
cargo run --locked --bin agent-guard-runtime -- --source "$PWD" --entry "$out/package/bin/agent-guard" --output "$out/runtime" --runtimes claude,pi,codex
```

The driver registers Claude Code and Pi with the registrations in the `--source` checkout's [setup guide](../../docs/setup.md), replacing only the guard path with its own executable, which records each hook and forwards it to the copied package. Pi discovers that extension in the synthetic home beside a separate synthetic provider. Codex gets the Bash hook its managed requirements would declare. The sessions enable Claude Code's Grep and Pi's grep, which neither client offers by default.

Each runtime runs its applicable cases three times: the shell cases, and for Claude Code and Pi at least one call of every other registered tool. Both also run a shell call whose guard exits without a reason, which Pi's extension must block with the `guard_unavailable` marker and Claude Code's `onFailure` setting with its own diagnostic. A call is bound to its hook by the tool name and input the model sent. Shell, Read and Grep calls prove execution through their output, Write and Edit through the target file, which the driver resets before each call. The driver records the client's resolved path, executable hash and version, the copied guard entry and native binary hashes, and the source manifest. `records.jsonl` retains each completed attempt; `report.json` contains the summary and records. Hook status and output are separate from the runtime tool result. An unverified call, a verdict or input mismatch, a missing hook or execution witness, or conflicting evidence makes the run incomplete, and the driver exits unsuccessfully. If startup fails before the first model request or hook, the driver stops that runtime and reports the attempted count separately from the planned calls.

The Claude Code and Pi drivers use an Anthropic Messages loopback server and Codex a Responses loopback server. A runtime failure before hooks remains unverified, not a guard denial.

Run the no-guard control separately with the same package and clients:

```sh
cargo run --locked --bin agent-guard-runtime -- --source "$PWD" --entry "$out/package/bin/agent-guard" --output "$out/runtime-without-guard" --runtimes claude,pi,codex --ablate
```

Every control case must execute, including the synthetic protected canaries. Compare the recorded identities before comparing reports. The measured hook duration excludes runtime startup; cold runtime durations include startup variability. These reports observe the entry PID only. They do not establish cleanup of runner, checker or other descendants.

Runtime acceptance needs both runs to succeed and zero `child setpgid` lines in the recorded stdout/stderr; a nonzero count requires investigation of the wrapper. A missing executable or failure before hooks does not establish a zero-warning result for that client. Keep host process-group or loopback refusals with the exact command and error, and rerun on an authorized host into fresh evidence directories; do not modify the harness to avoid the operation. Put the client executables themselves on `PATH`: a launcher that finds its client through `HOME` fails in the synthetic home. Codex also runs the hooks of the host's managed requirements, so on a host that registers its own Bash hook the Codex control cannot execute.

## Instrumented lifecycle

The lifecycle driver copies the Rust source into its evidence directory and injects faults there. Its fault module is embedded as test data in the Rust driver and never compiled into the release runner. It does not edit the checkout. Set the Cargo, Rustup and build caches outside the checkout and populate them with ordinary development checks first; copied fault builds use locked, offline Cargo dependencies. Pass the toolchain's own `cargo`: the driver limits `PATH` to that executable's directory, where a wrapper cannot find the real one.

```sh
cargo run --locked --bin agent-guard-lifecycle -- --source "$PWD" --cargo "$(rustup which cargo)" --output "$out/lifecycle"
```

Each fault in `FAULTS` ([lifecycle.rs](lifecycle.rs)) runs three times. Instrumented children record their own PID and process group. The driver samples survivors before its cleanup, records full output, verifies the large-output producer independently, and requires the guard's total deadline and failure contract.

The native runner makes itself the leader of its process group before it reads input or starts a child, and kills that group at the three-second total deadline. The shell entry kills the same group after the runner exits. The `deadline` control alters the runner's deadline in the copied source and `cleanup` removes the entry's post-exit group kill.

The following negative controls must each exit unsuccessfully with three contract violations. Use a distinct output directory for every invocation:

```sh
cargo run --locked --bin agent-guard-lifecycle -- --source "$PWD" --cargo "$(rustup which cargo)" --output "$out/control-drain" --control drain
```

`--help` lists the controls. `results.jsonl` records each observation and its violations; `summary.json` binds the source and counts failures. Rerun the unmodified lifecycle driver into a new directory after the controls to establish recovery. These instrumented copies prove the named lifecycle contracts, not all possible runtime descendants.

The local Rust harness contracts run with `cargo test --locked --test harness`. They exercise the HTTP protocols and CLI boundary with scripted clients, byte-preserving hook forwarding, timeout cleanup, report path isolation, full runtime case counts and missing-evidence failures. Actual client acceptance requires the separate runtime invocation above.
