# Development and release

This guide is for maintainers who change, check or release `agent-guard`. Installation and the public behavior contract live in the [setup guide](setup.md); engineering rules live in [AGENTS.md](../AGENTS.md).

## Toolchain

rustup selects the Rust toolchain from `rust-toolchain.toml`. `make check` also needs cargo-deny and `make build` needs cargo-about, both on `PATH`:

```sh
cargo install --locked cargo-deny
cargo install --locked --features cli cargo-about
```

The [check workflow](../.github/workflows/check.yml) pins the versions CI uses; install those with `--version` when a local result differs from CI. `cargo deny` fetches the RustSec advisory database, so `make check` needs network access.

## Development checks

Keep build outputs and evidence outside every checkout; `make` refuses a Cargo target directory inside one, because integration tests place their scratch evidence under it. Reuse one target directory across runs, since each new one holds a full set of builds:

```sh
export CARGO_TARGET_DIR=/absolute/path/outside/checkouts/agent-guard-target
out=/absolute/path/outside/checkouts/agent-guard-evidence
make check
make build OUT="$out/package"
cargo run --locked --bin agent-guard-verify -- "$out/package/bin/agent-guard"
```

`make build` stages the runtime package in `OUT`, including `LICENSE-THIRD-PARTY.md`, which cargo-about renders from `about.toml` and `about.hbs`. When a dependency update changes a license file that `about.toml` clarifies, cargo-about drops the clarification with only a warning and `make build` fails; review the changed file and update its clarification.

The [installed verifier](../cmd/agent-guard-verify/main.rs) runs the protocol cases against an assembled package, `bin/agent-guard` with the adjacent `agent-guard-native` and `VERSION`. It records both executables' hashes and versions and fails unless every case passes; it does not prove hook loading or all descendant cleanup. To evaluate an installation, run it from a checkout of the release being evaluated and pass the absolute installed executable, for example `/opt/homebrew/bin/agent-guard`. The verifier is not part of the runtime package.

The [synthetic runtime and lifecycle harnesses](../tests/harness/README.md) cover hook loading and child process cleanup, which the verifier does not. Check registration and runtime acceptance separately before replacing an installation.

## Release

This project remains in `0.x` until the maintainer explicitly prepares `1.0.0`. Until then every release raises the patch version, including one that widens or narrows the scope of what the guard blocks or allows. The [release workflow](../.agents/skills/release/SKILL.md) owns authorization, version selection, preparation folding and tag safety.

`VERSION` is the single version owner. From a validated default-branch checkout, update it and write `docs/releases/<version>.md`, then create an annotated `v<version>` tag and push it only after the release gate clears. The notes become the GitHub Release body, where a relative link does not resolve, so link to repository files through URLs pinned to the tag.

Pushing the tag starts `publish.yml`, which checks the version and notes, runs the checks, creates the GitHub Release from the notes, then dispatches `agent-guard-release` to the tap. The tap finds the version through Homebrew livecheck's `github_latest` strategy, so a tag alone cannot trigger an update before the Release passes its checks. It then opens a `brew bump-formula-pr` pull request that pins the tag and its commit revision; `brew test-bot` builds the bottle on that pull request and `brew pr-pull` publishes it. Verify the Release, tap revision and a built installation before declaring a release complete.

The bump pull request changes only the tag and its commit, and the tap publishes as soon as it builds, so packaging belongs in `make build`: the formula installs the package `make build` stages, so a packaging change ships with the release that contains it. A change to `make build`'s interface (`OUT`, `CARGO_TARGET_DIR`) or to the tools it runs also needs the formula. Make that change in a tap pull request, so that test-bot shows it still builds the version the formula pins: add a build dependency before the release that needs it, and remove one after. Such a pull request rebuilds a version the bump already bottled, so it raises the formula's package `revision`, not its pinned commit; and since only `bump-` branches publish automatically, dispatch the tap's `brew pr-pull` workflow with its number and head commit once test-bot passes.

If a job fails, inspect the Release and tap state before retrying; an existing Release is reused and only its tap notification is repeated.
