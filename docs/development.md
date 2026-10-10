# Development and release

This guide is for maintainers who change, check or release `agent-guard`. Installation and the public behavior contract live in the [setup guide](setup.md); engineering rules live in [AGENTS.md](../AGENTS.md).

## Toolchain

Use the Rust toolchain pinned in `rust-toolchain.toml` (1.99.0) for the production runner, development tools and runtime harnesses. Cargo's exact parser pins preserve policy semantics, and `Cargo.lock` binds dependency resolution.

Install cargo-deny 0.20.2 and cargo-about 0.9.2 before running the checks:

```sh
cargo install --locked --version 0.20.2 cargo-deny
cargo install --locked --version 0.9.2 --features cli cargo-about
```

Keep Cargo's executable directory on `PATH` so `cargo deny --version` reports `cargo-deny 0.20.2` and `cargo about --version` reports `cargo-about 0.9.2`. `cargo deny --locked check` fetches the RustSec advisory database and requires network access; advisory, license, ban and source checks remain enabled. CI installs the same versions from the official arm64 macOS release archives and verifies their pinned SHA-256 before extraction.

`about.toml` and `about.hbs` describe the license notices of the crates compiled into `agent-guard-native`; `make build` renders them into the package's `LICENSE-THIRD-PARTY.md`. The accepted licenses cover `deny.toml`'s allow list and exceptions, and the clarifications add the ICU and Unicode notices that tree-sitter and regex-syntax omit from their license expressions. A dependency update that changes a clarified file makes cargo-about drop the clarification with only a warning, so the render fails on any cargo-about warning.

## Development checks

Keep build outputs and evidence outside every checkout. Reuse one Cargo target directory across runs, because each new one holds a full set of builds; it stays outside checkouts because integration tests place their scratch evidence under it:

```sh
export CARGO_TARGET_DIR=/absolute/path/outside/checkouts/agent-guard-target
out=/absolute/path/outside/checkouts/agent-guard-evidence
make check
make build OUT="$out/package"
cargo run --locked --bin agent-guard-verify -- "$out/package/bin/agent-guard"
```

`make check` runs `make rust-check`: rustfmt, the source file length check, Clippy across all targets with warnings denied, locked Rust tests and cargo-deny. `make build` stages the package: both executables, `VERSION`, `LICENSE`, `README.md` and a cargo-about render of the license notices into `LICENSE-THIRD-PARTY.md`, which fails on a license `about.toml` cannot place or on any cargo-about warning. The render first runs `cargo fetch --locked`, because `cargo metadata` needs the crates of every target, which no build downloads. Set `CARGO` to an absolute executable path when absent from `PATH`. Rust tests check frozen contract fixtures for exact verdicts, public exit codes, denial text and advice.

The [installed verifier](../cmd/agent-guard-verify/main.rs) requires the assembled `bin/agent-guard`, adjacent `agent-guard-native` and `VERSION`; it resolves the executable paths, records both hashes and checks that both executables report the package version. Require all 33 protocol cases to pass. It does not prove hook loading or all descendant cleanup. To evaluate an installation, run it from a checkout of the release being evaluated and pass the absolute installed executable, for example `/opt/homebrew/bin/agent-guard`. The verifier is not part of the runtime package.

The [synthetic runtime and lifecycle harnesses](../tests/harness/README.md) check the assembled entry and adjacent binary. Runtime verdicts, direct protocol checks and instrumented lifecycle checks are separate evidence. A runtime failure before hooks is not a guard denial. Check registration and runtime acceptance separately before replacing an installation.

## Entry statuses and input bounds

A completed refusal, including oversized input, excess nesting, a relative working directory and invalid UTF-8, is native checker status `2`, native runner status `3` and hook status `2`. The shell entry blocks every other runner status as a failed check.

The byte cap was measured on release builds using public literal arguments, data-heavy Git pathspec substitutions and repeated statements from 64 KiB through 8 MiB. The substitution shape took 0.318 seconds at 256 KiB, 0.631 at 512 KiB and 1.286 at 1 MiB. 256 KiB was the largest measured warm size below 0.5 seconds; its serialized envelope was 262,286 bytes, so the cap is 262,144. The first cold literal launch took 0.988 seconds. These finite measurements describe those shapes on that host, not a universal latency guarantee. Depth bounds simultaneous nesting, not statement count or list position.

## Release

This project remains in `0.x`; do not prepare `1.0.0` under the current policy. Use patch for internal, fix, dependency and documentation changes that leave user-visible behavior unchanged, and minor only for a real contract change such as a widened or narrowed scope of what the guard blocks or allows. The [release workflow](../.agents/skills/release/SKILL.md) owns authorization, version selection, preparation folding and tag safety.

`VERSION` is the single version owner. From a validated default-branch checkout, update it and write `docs/releases/<version>.md`, then create an annotated `v<version>` tag and push it only after the release gate clears.

Pushing the tag starts `publish.yml`, which checks the version and notes, runs the checks, creates the GitHub Release from the notes, then dispatches `agent-guard-release` to the tap. The tap finds the version through Homebrew livecheck's `github_latest` strategy, so a tag alone cannot trigger an update before the Release passes its checks. It then opens a `brew bump-formula-pr` pull request that pins the tag and its commit revision; `brew test-bot` builds the bottle on that pull request and `brew pr-pull` publishes it. Verify the Release, tap revision and a built installation before declaring a release complete.

The bump pull request changes only the tag and revision, and the tap publishes as soon as it builds, so packaging belongs in `make build`: once the formula installs the package `make build` stages, a packaging change ships with the release that contains it. Until then the formula renders the notices with its own copy of these steps; switch it to `make build` in a tap pull request after the first release whose `make build` renders them. That pull request rebuilds a version the bump already bottled, so it raises the formula's package `revision`, not its pinned commit; and since only `bump-` branches publish automatically, dispatch the tap's `brew pr-pull` workflow with its number and head commit once test-bot passes. A change to `make build`'s interface (`OUT`, `CARGO_TARGET_DIR`) or to the tools it runs also needs the formula. Make that change in a tap pull request, so that test-bot shows it still builds the version the formula pins: add a build dependency before the release that needs it, and remove one after.

If a job fails, inspect the Release and tap state before retrying; an existing Release is reused and only its tap notification is repeated.
