# Agent Guard engineering contract

`agent-guard` is a macOS pre-tool hook for Claude Code, Codex, and Pi, not a sandbox. It checks only calls delivered by a registered hook. Reading another app's data under `~/Library` can be recorded as App Data access (`SystemPolicyAppDataDetailed`). A shell entry starts a Bun runner that supervises the guard process group. The `mvdan-sh` GopherJS port turns shell syntax into command records; `src/targets.ts` resolves them into the paths each command touches and what it does with each, from the program table in `src/programs.ts`; App Data and credential rules decide from those targets, and Claude workflow rules provide advice without overriding those decisions. Runtime registration belongs to each consumer, outside this package.

## Security boundaries

- Keep `bin/agent-guard` as the shell entry. It must change to the package directory before starting Bun and pass the hook's original directory as `--cwd`; otherwise an agent project's `bunfig.toml` preload can exit before the guard checks anything. Require the shipped `tsconfig.json` before starting Bun so an ancestor config cannot redirect `mvdan-sh`; verify that the parser resolves from the dependency package.
- Fail closed: exit `0` means no objection, exit `2` denies, and every other outcome must become a denial. Do not turn an operational failure into a successful-looking `null`, empty list, or similar value. Keep the guard deadline below each runtime hook timeout, and terminate a hung guard with its child processes.
- Check lexical App Data paths before following links. App Data link traversal in `src/links.ts` uses `readlink` alone; `stat`, `realpath`, and dereferencing directory listings there can trigger the access being prevented. The separate `~/.ssh` check in `src/paths.ts` uses inode identity to reject aliases and directories posing as public files. Do not use those probes for App Data traversal.
- Do not add an agent-editable allowlist, bypass switch, or equivalent configuration. Each denial reason must offer a concrete safe alternative; keep that behavior covered by tests. After a denial, do not retry the same intent with a different tool, path, or argument spelling.
- Keep the installed package self-contained: home-directory scan protection must not depend on `~/.ignore` or another dotfiles configuration.

## Requirement families

### Tests

Each test must protect a distinct behavior partition, regression, or interaction contract. Derive assertions from observable behavior so a behavior-preserving refactor passes and a plausible violation fails. Do not add tests that only find a string in source, assert that a retired implementation stays absent, or duplicate the same input partition, production path, observation, and failure mode. Keep typed TypeScript fixtures beside tests in `tests/fixtures/`; use their `reason` and `claude_suggestions` fields to check message content, including an empty suggestion list when advice must stay absent.

### Ablation

For each new or changed rule or mechanism, temporarily remove or break it, observe a relevant test fail, restore it, and report both the failing test and the recovered result. A test that cannot detect the change needs a stronger assertion or removal; explain any mechanism that cannot be ablated safely.

### Code

Prefer Bun and language facilities already in use, including `bun:test`, `Bun.file`, and `Bun.spawn`. Add a dependency only for a real caller or deployment need that existing facilities cannot meet; do not add a second test runner or duplicate library. Keep one real path, remove orphans created by a change, and limit guards to reachable failures or explicit contracts. Record rare shell semantics as known limits instead of adding speculative defenses. A denylist decides only from what the command text names. A read whose target the program picks while it runs belongs to the operating system's read restrictions. A command that prints a secret it may read has no path to check, so the listed subcommands stay as workarounds and the list is not extended. A program outside the reader list that is handed a credential path, such as `aws s3 cp .env s3://bucket/x` or `rclone copy .env remote:`, is allowed. Keep as known limits only rare shell semantics: cross-command variable flow such as a `for` loop variable, a move or link followed by a read, process substitution as input (`xargs cat < <(echo …)`), names another command prints into `xargs` (`ls *.pem | xargs cat`), wrappers outside the wrapper table such as `xcrun`, `rg -L` child links, globs inside links, `env -C` with redirection, `cd -`, `~-`, `readonly`, and `/System/Volumes/Data` aliases.

Prefer blank lines around block statements when they improve readability. Oxlint 1.86 has no general rule to enforce this preference, so review it manually.

### Documentation

Before writing, decide what the reader needs to know or do and what the author can verify. Keep README focused on purpose and capabilities; installation, registration, removal, and verification commands belong only in `docs/setup.md`. Describe the current contract and useful limits, without a change diary, filler, or omitted steps needed to verify a setup.

## Working decisions

- Ground conclusions in current repository code and test output. Check version-sensitive behavior against installed versions and official documentation; mark unchecked claims `unverified`.
- Diagnose a bug's root cause and causal chain before editing. Rebuild a wrong architecture instead of stacking local patches. Use a workaround only when the owning fix is unavailable; state why and when the workaround becomes invalid at its boundary.
- Never read, print, copy into an artifact, or fill in credentials, including OTPs, tokens, and credential stores. Leave credential-store repair to the user. Pushes, publishing, and other remote changes require the user's specific authorization; pushing a `v*` tag publishes to npm. Publishing uses GitHub Actions trusted publishing with OIDC, without an `NPM_TOKEN` or bypass-2FA token. The consuming Brewfile uses `trusted: true`; the Bun-dependent formula and its update policy belong to `LoopHubs/homebrew-tap`.
- Use type-prefixed commit subjects (`type: summary` or `type(scope): summary`, such as `fix:`, `chore(deps):`, and `release:`) whose imperative summary names the main observable change; put rationale in the body. One commit represents one observable outcome with its implementation, cleanup, tests, and documentation. Do not commit unless requested.

## Ownership of rules

Put mechanically enforceable constraints in configuration, types, lint, or tests, and do not repeat them here or in README. Keep one rule at its narrowest owner. Code expresses what happens; comments explain a non-obvious reason and its invalidation condition; documentation states contracts, limits, and use; tests prove observable behavior. Remove secondary restatements when two surfaces say the same thing.
