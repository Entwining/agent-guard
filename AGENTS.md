# Agent Guard engineering contract

`agent-guard` checks supported tool calls for protected macOS application data access through registered pre-tool hooks. Runtime registration and operating system access policy belong to the consumer, outside this package. `docs/setup.md` owns the public contract and known limits.

The Go implementation lives in `native/`; `cmd/` owns executable boundaries. Shell syntax becomes command records in `native/shell/`, program adapters infer target roles in `native/targets/`, filesystem checks resolve resource identity, and `native/rules/` decides policy. Workflow advice must not override a protection decision. The production entry is `bin/agent-guard`, beside the compiled `agent-guard-native`. Development tools and harnesses use Go; historical implementations belong to Git history.

## Security boundaries

- Preserve the public failure contract when changing the shell entry, runner or checker. Operational errors must not become permission to proceed, and cancellation must include child completion and reaping. Keep internal deadlines below the consumer's hook timeout.
- Apply the narrower probing constraints in `native/filesystem/AGENTS.md` before changing filesystem traversal. A preflight probe must not perform the protected read it is meant to prevent.
- Do not add an agent-editable allowlist, bypass switch or equivalent configuration. Keep protection self-contained rather than dependent on user dotfiles. Each denial must provide a concrete safe alternative.

## Requirement families

### Tests

Each test must protect a distinct behavior partition, regression or interaction contract. Derive assertions from observable behavior so a behavior-preserving refactor passes and a plausible violation fails. Do not test source strings or the absence of a retired implementation. Keep contract fixtures in `tests/fixtures/`; assert verdicts, public exit codes, denial reasons and advice, including absent advice. A test that checks concurrent child code must instrument that child, not infer its correctness from the parent.

### Ablation

For each new or changed rule or mechanism, temporarily remove or break it, observe a relevant test fail, restore it, and report both the failing test and the recovered result. A test that cannot detect the change needs a stronger assertion or removal; explain any mechanism that cannot be ablated safely.

### Code

Use explicit error returns and context cancellation or deadlines when work can outlive its caller. Communicate ownership when memory must be shared. Every concurrent construct needs a measured latency, throughput or behavior benefit on its actual workload; prefer a synchronous alternative when it is equally fast. Race correctness and timely child reaping are separate obligations from performance.

Keep one real implementation path, remove orphans created by a change, and add dependencies only for an existing caller or deployment need. Correct target roles at the program adapter instead of adding a policy rule keyed to a command's shape. Keep secret-dump workarounds at their existing rule owner rather than growing a speculative list. Mechanical checks belong to `native/check` and the existing configuration, not instruction prose.

### Documentation

Before writing, decide what the reader needs to know or do and what the author can verify. Keep README focused on purpose and capabilities; installation, registration, removal, and verification commands belong only in `docs/setup.md`. Describe the current contract and useful limits, without a change diary, filler, or omitted steps needed to verify a setup.

## Working decisions

- Ground conclusions in current repository code and test output. Check version-sensitive behavior against installed versions and official documentation; mark unchecked claims `unverified`.
- Diagnose a bug's root cause and causal chain before editing. Rebuild a wrong architecture instead of stacking local patches. Use a workaround only when the owning fix is unavailable; state why and when the workaround becomes invalid at its boundary.
- Release actions follow the repository-local release skill and the workflow that owns publication. Keep distribution changes within their authorized gate; runtime acceptance does not authorize changing machine permissions.
- Use type-prefixed commit subjects (`type: summary` or `type(scope): summary`) whose imperative summary names the main observable change; put rationale in the body. One commit represents one observable outcome with its implementation, cleanup, tests and documentation. Do not commit unless requested.

## Ownership of rules

Put mechanically enforceable constraints in configuration, types, lint, or tests, and do not repeat them here or in README. Keep one rule at its narrowest owner. Code expresses what happens; comments explain a non-obvious reason and its invalidation condition; documentation states contracts, limits, and use; tests prove observable behavior. Remove secondary restatements when two surfaces say the same thing.

Name files for what they hold; do not use opaque stage abbreviations. Group files by their owning responsibility: installed shell entries in `bin/`, executable boundaries in `cmd/`, shell syntax and command records in `native/shell/`, target inference and adapters in `native/targets/`, shared path and link checks in `native/filesystem/`, policy in `native/rules/`, and orchestration in `native/core/`. Shared records and reasons have their own packages. Keep harness drivers in `tests/harness/` and contract fixtures in `tests/fixtures/`; preserve familiar entry locations unless moving them resolves an ownership ambiguity.
