# Synthetic hook evaluation

Run these checks from the checkout on macOS with Bun and Python 3.9 or newer installed. The runtime drivers resolve Claude Code, Pi, or Codex from PATH. They use temporary homes, synthetic files, and local scripted model servers; they do not require live model credentials.

The focused entry and adapter checks run with the project test runner:

```sh
bun test tests/harness/fault-injection.test.ts tests/harness/runtime-fidelity.test.ts
```

Runtime observations write to a new `/tmp` result path when no output is given. To keep paired runs together, pass explicit output paths outside the checkout:

```sh
bun tests/harness/runtime-suite.ts /tmp/agent-guard-runtime-normal.json --paired /tmp/agent-guard-runtime-without-guard.json
bun tests/harness/runtime-paired-runs.ts /tmp/agent-guard-runtime-normal.json /tmp/agent-guard-runtime-without-guard.json /tmp/agent-guard-runtime-paired.json
bun tests/harness/runtime-codex.ts /tmp/agent-guard-runtime-codex.json
```

The drivers record hook status and output separately from the runtime tool result. A runtime that cannot start yields incomplete evidence, not a guard verdict.
