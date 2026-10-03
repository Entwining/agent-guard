# Agent Guard engineering contract

`agent-guard` checks supported tool calls for protected macOS application data access through registered pre-tool hooks. Runtime registration and operating system access policy belong to the consumer, outside this package. `docs/setup.md` owns the public contract and known limits.

The production implementation, development tools and runtime/lifecycle harnesses use Go; historical implementations belong to Git history. The production entry is `bin/agent-guard`, beside the compiled `agent-guard-native`; `cmd/` owns executable boundaries. The root Cargo package (`src/` and the Rust tests in `tests/*.rs`) is an offline migration trial: it is on no hook path, decides no production call and has no Go fallback, and Go stays authoritative until a consumer cutover is separately accepted.

## Before changing the implementation

`native/shell/` turns shell syntax into command records, `native/targets/` infers read/write roles, `native/filesystem/` resolves resource identity, `native/rules/` decides protection and workflow advice, and `native/core/` orchestrates them. Correct target roles in the program adapter rather than adding a policy rule keyed to a command's shape. Keep secret-dump handling at its existing rule owner rather than growing a speculative command list.

- Read [filesystem instructions](native/filesystem/AGENTS.md) before changing `native/filesystem/` or the Rust filesystem module under `src/`, including glob matching. Check lexical protected paths before probing filesystem identity; a preflight probe must not perform the protected read it is meant to prevent.
- Preserve the public failure contract when changing the shell entry, runner or checker. Operational errors must not become permission to proceed, and cancellation must include child completion and reaping. Keep internal deadlines below the consumer's hook timeout. Workflow advice must not override a protection decision.
- Do not add an agent-editable allowlist, bypass switch or equivalent configuration. Keep protection self-contained rather than dependent on user dotfiles. Each denial must provide a concrete safe alternative.

## Verification

`make check` and `native/check` own mechanical checks for Go, and `make rust-check` owns them for the Rust trial. Keep contract fixtures in `tests/fixtures/`; assert verdicts, public exit codes, denial reasons and advice, including absent advice. Do not test source strings or the absence of a retired implementation. Keep installed protocol, hook loading and lifecycle evidence separate; [lifecycle checks](tests/harness/README.md#instrumented-lifecycle) must instrument the child rather than infer its completion from the parent.

- For each new or changed rule or mechanism, temporarily remove or break it, observe a relevant test fail, restore it, and report both the failing test and the recovered result. A test that cannot detect the change needs a stronger assertion or removal; explain any mechanism that cannot be ablated safely.
- Every concurrent construct needs a measured latency, throughput or behavior benefit on its actual workload; prefer a synchronous alternative when it is equally fast. Race correctness and timely child reaping are separate obligations from performance.

## Documentation and release

Keep the root README focused on purpose and capabilities; installation, registration, removal and verification commands belong in `docs/setup.md`.

Release actions follow the [repository-local release skill](.agents/skills/release/SKILL.md) and the workflow that owns publication. Runtime acceptance does not authorize changing machine permissions. Use type-prefixed commit subjects (`type: summary` or `type(scope): summary`).
