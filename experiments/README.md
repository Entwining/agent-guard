# macOS App Data 2x2 experiment

Run manually from a terminal without an agent hook. Do not retry a blocked agent command through another tool. Python 3.9 or later, Apple Command Line Tools and codesign must already be installed. For `--guard-mode on`, pass `--guard` with the assembled executable built using [the development checks](../docs/development.md#development-checks) or installed by Homebrew; the checkout entry alone is not an assembled package. The script reads only its random canary, never real application data, credentials or TCC.db, and never changes permission configuration.

Each invocation measures one cell: choose `--guard-mode off/on`, `--os-mode off/on`, and `--form direct_operand/inline_literal/external_script/runtime_config_path`. Run all 16 combinations separately in disposable accounts or restored snapshots with equivalent initial permissions and the same terminal launch arrangement. Record existing App Data and Full Disk Access status. Prior setup, reads, grants and entries can mask later effects; sequential reads in one session cannot establish independent TCC prevention. Do not reset permissions or change settings to make a result pass.

From the repository root, the discriminator cell removes command analysis and enables the OS restriction:

```sh
python3 experiments/read-enforcement-comparison.py --guard-mode off --os-mode on --form runtime_config_path --output /private/tmp/agent-guard-off-os-on-runtime.jsonl
```

Use a new output name in an existing directory outside the checkout and protected user folders, or omit `--output` for a generated `/tmp` name. The adjacent `.artifacts` directory retains scripts, settings, source and an ad hoc signed CanaryWriter.app. Only the sandboxed writer creates the native canary in its unique container, after confirming its container HOME and refusing to overwrite a file. The controller does not enumerate containers or read back the canary during setup; it requires a matching owner write receipt. An ordinary mkdir does not establish TCC protection. [Apple's privacy presentation](https://developer.apple.com/videos/play/wwdc2023/10053/) describes protection for sandboxed app data, session grants and the Full Disk Access exception. A prompt or entry in the guard off / OS off control must establish that this fixture exercises TCC on the tested host; otherwise TCC conclusions remain unverified.

OS on first validates the profile outside Library: unrestricted read, allowed read under the profile, then a denied open with an EPERM/EACCES receipt from Python. Failure exits 3 with a skipped operation, never a prevented read. The profile explicitly allows default operations and denies `file-read*` under the canary container; guard preflight and access both inherit it. Installed `man sandbox`, `man sandbox-exec` and `man sandbox_init` document resource acquisition, inheritance, existing descriptor limits and deprecation. [Apple's CUPS generator](https://github.com/apple/cups/blob/master/scheduler/process.c) also explicitly selects a default action. These sources do not guarantee ordering relative to TCC.

Watch each phase, then record prompt presence, literal prompt text, new entry presence and the responsible application's literal UI label separately for baseline, setup, guard and operation. Inspect System Settings > Privacy & Security > Files & Folders without changing settings. Use `unknown` when the UI cannot establish an answer. Leave permission prompts unanswered; the 20 second deadline terminates the request. Setup prompts or entries contaminate a later negative observation. Preserve contaminated rows without claiming prevention. Never infer responsibility or absence of a TCC effect from a failed read.

JSONL retains exact argv, exit code, operation status, stderr, stdout byte count, matched canary byte count, OS control status and manual UI observations. Build commands have 60 second deadlines; subprocesses own groups, which timeout cleanup kills. Guard on checks the exact command and skips access on every guard failure. Outputs and partial setup artifacts remain for review. Delete only recorded experiment artifacts afterwards; deleting a container does not remove a TCC entry. OS off retains existing host permissions, and one terminal arrangement does not establish behavior for other responsible applications or agent runtimes.

For credential free validation outside the user's Library, use a new synthetic HOME and output name:

```sh
python3 experiments/read-enforcement-comparison.py --guard-mode on --guard "$out/package/bin/agent-guard" --os-mode off --form direct_operand --synthetic-home /private/tmp/agent-guard-validation-home --output /private/tmp/agent-guard-validation.jsonl
```

Synthetic mode builds and verifies the native fixture but never launches it. It creates the canary under the supplied new HOME's ordinary `canaries` directory, runs the same access commands and guard preflight, and marks every UI observation `not_observed_synthetic`. It does not recreate the previously blocked synthetic Library/Containers setup or validate App Data rejection. OS on can still exit 3 if this execution context cannot apply the sandbox. Synthetic results validate setup and execution, never TCC.
