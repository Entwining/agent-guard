# Agent Guard engineering contract

`agent-guard` checks supported tool calls for protected macOS application data access through registered pre-tool hooks. Runtime registration and operating system access policy belong to the consumer, outside this package. `docs/setup.md` owns the public contract and known limits.

The production implementation uses Rust; Go owns development tools and runtime/lifecycle harnesses. Historical implementations belong to Git history. The production entry is `bin/agent-guard`, beside the compiled `agent-guard-native`; `src/entry.rs` owns the runner and checker boundaries, and `cmd/` owns development executables. Source cutover does not replace an installed hook or authorize a release or client configuration change.

## Before changing the implementation

`src/shell/` turns shell syntax into command records, `src/targets.rs` and `src/targets/` infer read/write roles, `src/filesystem.rs` and `src/filesystem/` resolve resource identity, and `src/policy.rs` decides protection and workflow advice. `src/adapters.rs` owns consumer decoding and framing; `src/entry.rs` orchestrates native execution. Correct target roles in the program adapter rather than adding a policy rule keyed to a command's shape. Keep secret-dump handling at its existing rule owner rather than growing a speculative command list.

- Read [Rust instructions](src/AGENTS.md) before changing `src/`, `tests/*.rs`, `examples/` or `Cargo.toml`.
- Check lexical protected paths before probing filesystem identity; a preflight probe must not perform the protected read it is meant to prevent. `src/AGENTS.md` owns the filesystem probe and glob boundaries.
- Preserve the public failure contract when changing the shell entry, runner or checker. Operational errors must not become permission to proceed, and cancellation must include child completion and reaping. Keep internal deadlines below the consumer's hook timeout. Workflow advice must not override a protection decision.
- Do not add an agent-editable allowlist, bypass switch or equivalent configuration. Keep protection self-contained rather than dependent on user dotfiles. Each denial must provide a concrete safe alternative.

## Verification

`make check` owns mechanical checks for the Rust implementation and Go development tools; `make rust-check` runs the Rust portion and `scripts/check-go` runs the Go portion. Keep contract fixtures in `tests/fixtures/`; assert verdicts, public exit codes, denial reasons and advice, including absent advice. Do not test source strings or the absence of a retired implementation. Keep installed protocol, hook loading and lifecycle evidence separate; [lifecycle checks](tests/harness/README.md#instrumented-lifecycle) must instrument the child rather than infer its completion from the parent.

- For each new or changed rule or mechanism, temporarily remove or break it, observe a relevant test fail, restore it, and report both the failing test and the recovered result. A test that cannot detect the change needs a stronger assertion or removal; explain any mechanism that cannot be ablated safely.
- Every concurrent construct needs a measured latency, throughput or behavior benefit on its actual workload; prefer a synchronous alternative when it is equally fast. Race correctness and timely child reaping are separate obligations from performance.
- Fixtures contain only synthetic, machine-independent inputs and behavioral expectations; derive HOME-dependent paths from the test's own temporary root. Tests assert product behavior and account for each consumed row, rather than checking fixture provenance, self-consistency or fixed corpus totals; exact values belong only to a documented behavior partition or resource boundary.

## Documentation and release

Keep the root README focused on purpose and capabilities; installation, registration, removal and verification commands belong in `docs/setup.md`.

Release actions follow the [repository-local release skill](.agents/skills/release/SKILL.md) and the workflow that owns publication. Runtime acceptance does not authorize changing machine permissions. Use type-prefixed commit subjects (`type: summary` or `type(scope): summary`).
