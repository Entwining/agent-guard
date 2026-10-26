# Development and release

This guide is for maintainers who change, check or release `agent-guard`. Installation and the public behavior contract live in the [setup guide](setup.md); engineering rules live in [AGENTS.md](../AGENTS.md).

## Toolchain

rustup selects the Rust toolchain from `rust-toolchain.toml`. `make check` also needs cargo-nextest and cargo-deny, and `make build` needs cargo-about, all on `PATH`:

```sh
cargo install --locked cargo-nextest
cargo install --locked cargo-deny
cargo install --locked --features cli cargo-about
```

The install step of the [check workflow](../.github/workflows/check.yml) prints the versions CI uses; install those with `--version` when a local result differs from CI. `cargo deny` fetches the RustSec advisory database, so `make check` needs network access.

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

Pushing the tag starts `publish.yml`, which checks the version and notes, runs the checks, creates the GitHub Release from the notes, and notifies the [Homebrew tap](https://github.com/Entwining/homebrew-tap#upstream-contract), which owns formula PRs, bottle publication and recovery. Verify the Release, the formula's pinned tag and commit revision, and a built installation before declaring a release complete.

The formula installs the package staged by `make build`, so packaging changes belong there and ship with their upstream release. A change to `make build`'s interface (`OUT`, `CARGO_TARGET_DIR`) or to the tools it runs also needs a tap pull request that builds the version the formula currently pins: add a build dependency before the release that needs it, and remove one after.

If a job fails, inspect the Release and tap state before retrying; an existing Release is reused and only its tap notification is repeated.
