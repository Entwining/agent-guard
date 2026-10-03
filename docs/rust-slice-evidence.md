# Offline Rust P1 slice evidence

This development packet exercises a bounded Rust preflight library. Go remains
authoritative; the Rust package has no hook path, arbitrary submitted-code
execution, Go fallback or installed-consumer acceptance. The independent held-out
set and scrutiny files were not read. The evaluator and Gate A surrogate are in
`f63c7b24ff29cd2e628c867132abe5768e91f7be`; the test-only owned writer is in
`6aafdebc9c37fa9f851f61fba2a0a5362e772193`. Rejected recovery scopes also name their
identity, inspection or syntax boundary in `a86854e93f9274ad1d37821974a6ddda2c232555`.
The current R4d measurements include D21's `ls` role repair in
`a0aa4cd35a0a90dd7a5f71ce732fb8be1fc3c849` and its separately committed overlay
revision in `0a1b34292bc67d577be1aecf4af35551a0b68179`. All R4b raw evidence remains
unchanged for comparison.
The completed `ls` role inference also keeps default cwd traversal beside
redirections and treats names after `--` as operands rather than options.

## Development contract

The manifest contains 260 labelled rows: 248 counted non-writer cases, nine
metamorphic variants and W01–W03. The contract test checks all 257 non-writer rows
through both parser arms (514 row/arm checks); nine variants also compare with their
bases through both arms (18 comparisons). Assertions cover outcome and coverage,
wire exit/stdout/stderr, semantic reason owners, advice, concrete recovery,
lexical-before-probe ordering, operation starts, fixture results and child reaping.
The 17 mechanism tests and three parser/word/span unit tests protect distinct
boundaries. The two entry and two error tests preserve the initial failure
contract. With the differential, owned-writer and offline interface tests,
`cargo test --locked` contains 29 tests.

Closed operations read/write/list only owner-created synthetic files. They never
execute submitted shell or interpreter code. The worker is a fixed test program;
its ready PID receipt, `wait` result and subsequent `try_wait` establish completion
and reaping at the injected failure boundary. This does not establish a production
checker deadline. W01–W03 establish single-owner state and byte preservation in
test code, without claiming concurrent-writer or crash-recovery acceptance.

## Offline evaluation interface

Build `examples/evaluate.rs` with `cargo build --release --locked --example evaluate`
and an external `CARGO_TARGET_DIR`. The packaged `agent-guard-rust-slice` remains
version-only. The example accepts JSONL on stdin; the caller materializes all
synthetic filesystem state. Guard mode uses `evaluate_with_arm`, real `DiskProbe`
readlink checks and `adapters::render`; it never executes the submitted operation.

Each request requires string `id`, `consumer` (`claude`, `codex`, `pi`), `arm`
(`structured`, `brush`, `tree`), `home`, `cwd`, and exactly one `event` object or
`event_raw` string (its UTF-8 bytes are passed unchanged). Optional trusted
`context` supplies the library's `project`, `objective`, `public_task`,
`zsh_executor` and `require_execution_owner`. Defaults follow the development
profile: project `home/project`, objective `obtain fixture fact`, public Read of
`project/input.txt`, zsh execution except Pi, and no required execution owner.
`public_task.kind` is `Read`, `Write` or `List` with `path`; `Search` with `pattern`
and `glob`; `LiteralFile` or `Redirect` with `path` and `content`; `Emit` with
`literal`; `Script` with `source`; or `HomeSetting` without payload fields.
These are caller-owned continuation metadata, never inferred from submitted input.

Default `--mode guard` emits `id`, library `outcome` name, `class`
(`N`, `A`, `D`, `UC`, `UR`, `UO`, `F`), `coverage` (`state`, plus `gaps`, `tool` or
`error_kind` when applicable), `disposition`, `cause`, `reason`, `advice` message
array, structured `recovery`, and exact adapter `exit`, `stdout`, `stderr`.
Inapplicable fields are null or empty; F has no completed outcome and uses
`BlockOnCheckError`. `evaluate_ns` includes only evaluation and wire rendering,
excluding request decoding, context construction, response mapping and JSON I/O.
Invalid outer JSON or fields emit only `id` (null if unavailable) and a sanitized
`request_error` object; they never become N. Per-request rejection does not stop
the stream; transport I/O failure exits the example unsuccessfully.

`--mode parse-only` decodes the consumer envelope outside timing, then constructs
and runs only the selected parser on the shell source. It emits `id`,
`parse_status` (`parsed`, `parse_failed`, `parser_error`, `input_error`,
`outside_arm_coverage`), `parse_ns` and `error_kind`. Structured-only and non-shell
requests have no parser timing. Timing includes fresh parser construction, parsing
and disposal, without observation lowering, policy, identity or probes. No mode
adds threads or caches. The interface test pipes manifest-derived N/D/F requests
through the handler, reuses the contract's preflight tuple assertions and compares
all wire bytes and denial recovery against direct library evaluation.

## Release arm comparison

One release evaluation/render sample per event was recorded on Darwin 27.0.0
arm64. All arms use the same 257 non-writer inputs. The table excludes the nine
variants from outcome and task counts. Actual D includes the frozen conditional
U-R cases where independent nested extraction found a protected effect.

| Arm | N | A | D | U-C | U-R | U-O | F | Zero starts / required blocks | Closed fixture results | Public controls, original incomplete | Rechecked recoveries |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Structured/native policy only | 16 | 0 | 24 | 10 | 171 | 6 | 21 | 147/147 | 16 | 10 | 100 |
| Brush | 72 | 1 | 96 | 28 | 21 | 6 | 24 | 147/147 | 88 | 13 | 110 |
| Tree-sitter | 72 | 1 | 96 | 28 | 21 | 6 | 24 | 147/147 | 88 | 13 | 110 |

R4d changes none of the outcome, start, fixture-result or recovery-state counts
from R4b. All 257 event tuples per arm retain their outcome, coverage, wire,
operation/effect and recovery observations after normalizing the synthetic root.

The 147-block denominator is the counted requirement-labelled D/U-R/F/U-O cases.
It excludes writers, variants and U-C. All permitted calls obtain their asserted
closed-fixture result or explicitly limited public control. Each parser arm also
has six U-C configuration cases whose witness reads a synthetic protected file;
these expose the preflight gap and are not protection successes. Each parser arm
has 13 owner-action recoveries. Structured-only has 70 unsupported shell recovery
proposals and 31 owner actions; its additional original rejections do not establish
better task completion.

| Arm | Evaluation/render samples | p50 µs | p95 µs | p99 µs | Max µs | Worker samples | Worker p50 ms | Worker max ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Structured/native policy only | 248 | 5.125 | 47.750 | 81.667 | 434.167 | 9 | 1.953 | 32.107 |
| Brush | 248 | 23.708 | 106.125 | 1618.833 | 1830.875 | 9 | 1.951 | 32.141 |
| Tree-sitter | 248 | 27.875 | 114.458 | 2398.875 | 2501.375 | 9 | 1.945 | 32.041 |

Latency samples include variants. Timing starts after fixture construction and
event materialization, covers evaluation and rendering, and excludes the original
closed operation and recovery recheck. Worker rows include process arrangement
and reaping and are separated from normal evaluation. Quantiles use nearest rank.
R4b's 385.301 ms worker sample and its earlier 590.417 ms sample remain in
`r4b-arm-structured.jsonl` and `r4b-initial-arm-structured.jsonl`; neither is
filtered. `r4d-deltas.json` records both rounds' quantiles. These rerun timing
differences do not establish a causal performance improvement.
A single small host sample does not establish production tail latency, memory use
or parser equivalence.
The evaluator is synchronous and adds no threads, async execution or shared cache.

## Classification differential

Each parser arm accounts for all 1,289 overlay rows: 1,265 operations through
Claude, Codex and Pi projections, plus 24 link-metadata checks. Source and overlay
hashes bind every ID, input, rule ID and label. Input mechanisms select scope before
expectation comparison; every outside row still has its evaluation and reason.
Historical Codex Read/Grep/Write/Edit projections do not claim those operations are
enrolled Codex runtime tools. Go's quiet permit has no coverage tuple, so a quiet
Rust U-C permission match does not establish SupportedPreflight.

Both arms produce the same row categories and family counts:

| Family | RETAIN match | CHANGE match | RETAIN conflict in slice | Outside slice | Link metadata match | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| appdata | 58 | 4 | 0 | 80 | 0 | 142 |
| clients | 0 | 0 | 0 | 119 | 0 | 119 |
| credentials | 111 | 2 | 0 | 180 | 0 | 293 |
| options | 3 | 0 | 0 | 11 | 0 | 14 |
| programs | 3 | 0 | 0 | 64 | 0 | 67 |
| readers | 24 | 0 | 0 | 216 | 0 | 240 |
| search | 41 | 1 | 0 | 60 | 0 | 102 |
| cwd | 2 | 0 | 0 | 67 | 0 | 69 |
| shell | 77 | 8 | 0 | 96 | 0 | 181 |
| interpreters | 10 | 0 | 0 | 28 | 0 | 38 |
| filesystem_link | 0 | 0 | 0 | 0 | 24 | 24 |
| Total | 329 | 15 | 0 | 921 | 24 | 1289 |

There are zero Rust-defect classifications or RETAIN conflicts within the selected
mechanisms, 45 CHANGE consumer observations and 987 matching RETAIN observations
per arm. The 921 outside operations remain outside the conformance denominator;
all-RETAIN conformance beyond the slice is not claimed. D21 supersedes the R4b
baseline-defect classification: five rows were Rust defects and two were overlay
defects. Each changed row applies to all three consumers in both parser arms.

| ID | Input/context | D21 resolution | R4b actual → R4d actual |
| --- | --- | --- | --- |
| appdata[58] | `ls ~`, project cwd | RETAIN N, non-recursive names | D → N |
| appdata[62] | `ls /`, project cwd | RETAIN N, non-recursive names | D → N |
| appdata[92] | `ls /tmp`, App Data cwd | CHANGE D, D17 protected cwd | D → D |
| appdata[120] | `ls -ltr ~`, project cwd | RETAIN N, non-recursive names | D → N |
| appdata[121] | `ls -ltr`, HOME cwd | RETAIN N, non-recursive names | D → N |
| credentials[87] | `ls`, SSH cwd | CHANGE D, D17 protected cwd | D → D |
| shell[133] | `if true; then ls; fi`, HOME cwd | RETAIN N, non-recursive names | D → N |

By family, RETAIN matches increase by four in appdata and one in shell; CHANGE
matches increase by one in appdata and one in credentials. Other family category
counts, outside selection and link metadata counts do not move. Every selected
mismatch now fails the differential; the baseline-defect exception was removed.
All 15 CHANGE records match outcome, coverage, reason/advice and canonical recovery
rechecks. The two new protected-cwd recovery operations establish a public read
control, not completion of the original listing. Only those two overlay lines
changed; the other 1,287 lines and original Go fixtures remain byte-identical.
The new overlay SHA-256 is
`af65478c60c1a997a01d1e0bd4bd00a174c6351a998eb1f62b800a5ccb4a27c4`.

One additional outcome moves: appdata[2], `ls -a | xargs wc` from HOME, changes
D → U-C on all consumers in both arms. Non-recursive `ls` no longer supplies a
broad-root denial; `xargs wc` remains an UnknownProgram/stream-fed operand gap.
Its pre-existing outside-slice category is unchanged, and this is not a protection
success. The modeled hidden-listing/content-consumer rule remains unchanged and
its `xargs cat` controls still deny. Across all consumer observations, appdata's
actual D count falls by 15, N increases by 12 and U-C by three; shell D falls by
three and N increases by three. No other actual-class count moves.

Tree-sitter also changes only the reason/recovery scope for shell[137],
`ls &> ~/Library/Containers/x` from HOME: D and RETAIN match stay unchanged, while
the erroneous broad-root reason yields to protected App Data redirect-write
protection. Tree-sitter records the listing before the separate redirect record;
removing the spurious recursive role lets the write owner supply the denial.
Brush's reason/recovery is unchanged. Recursive controls appdata[75], cwd[6] and
cwd[7] remain D for every consumer in both arms; cwd[6–7] retain their explicitly
limited/outside `cd` coverage.

Outside mechanisms include program adapters outside the development subset
(including Shell grep beyond the rg development partition), SSH public-file
stat/inode identity, stream-fed operands/code, non-development glob/expansion and
workflow partitions, command-local/exported assignments, complex heredoc grammar,
post-link parent traversal and case-folded firmlink spellings. Every outside row,
consumer observation and specific reason is retained in the mismatch inventories.

## Causal ablations

The R4b rows below record a compiled, named test failure (exit 101), exact-byte
restoration with matching SHA-256, and that same test passing (exit 0). No required
mechanism needed a safety exemption. Lexical-first failure is intercepted by the
recording probe before a protected spelling can reach real readlink; Gate A's
negative reads only a synthetic canary. Parser failure may independently retain
U-R, so detector evidence asserts the gap after success and failure rather than
claiming every rejected verdict changes.

| Mechanism | Broken behavior | Failing test |
| --- | --- | --- |
| Detector | Suppress divergence gap | `detector_success_and_failure` |
| Complete argv union | Drop unsplit reading | `alternative_argv_union_preserves_roles` |
| Glob position | Ignore trailing-group requirement | `glob_group_position_and_body` |
| Lexical first | Remove both lexical checks | `lexical_protection_precedes_probe` |
| Identity bound | Return public at depth exhaustion | `identity_depth_bound_is_not_syntax_or_success` |
| Probe fault | Treat probe error as absence | `probe_fault_is_not_permission` |
| Backslash boundary | Accept escaped code token | `interpreter_backslash_boundary` |
| D16 name-only | Treat `--files` as content search | `name_only_listing_and_content_consumer` |
| Protected cwd | Suppress independent cwd check | `protected_cwd_is_an_independent_owner` |
| Broad recovery | Remove excluded HOME scope | `broad_root_recovery_preserves_excluded_scope` |
| Coverage state | Mark gaps SupportedPreflight | `limited_coverage_is_quiet_on_wire` |
| U-C quiet wire | Emit a warning | `limited_coverage_is_quiet_on_wire` |
| Gate A | Permit every result | `gate_a_zero_starts_and_bypass_negative` |
| Input bound | Increase bound one byte | `dev_contract` |
| Nesting bound | Increase bound one delimiter | `dev_contract` |
| Inspection budget | Raise 512 to 4096 | `inspection_budget_bounds_function_expansion` |
| Original UTF-8 spans | Skip span validation | `shell::divergence::tests::original_utf8_spans_are_checked` |
| Env dump | Suppress bare/env -i dump flag | `command_boundaries_preserve_protected_operands` |
| Recovery boundary | Remove identity/budget/syntax exclusions | `every_legacy_row_is_accounted_for` |
| R4d ls recursion role | Restore always-recursive `ls` roles | `listing_recursion_controls_broad_root` |
| R4d ls cwd role | Let a redirect suppress the default cwd target | `listing_recursion_controls_broad_root` |
| R4d ls option boundary | Treat names after `--` as options | `listing_recursion_controls_broad_root` |

The R4d test first fails on the original implementation with `ls ~` yielding D
instead of N. After repair, restoring the exact original `src/targets.rs` repeats
that named failure (101); restoring the repaired bytes passes (0). Repaired and
restored SHA-256 are both
`87719955accf0f0795ebcd225ea422e0ff0c521b65f1273e50d8e6df4e53ddea`.
Two additional regressions first fail before repair: HOME-cwd
`ls -R > out.txt` yields N instead of D, and `ls -- -R ~` yields D instead of N.
Each owning branch is then separately broken, compiled and observed failing
(101), restored to the same exact repaired hash and observed passing (0).
The same test covers non-recursive root/cwd listings, recursive HOME/Library/`/`
roots with `-R`, `--recursive`, `-laR`, `-Rl`, project-scoped recovery rechecks and
protected SSH/App Data identities in both arms and all three consumers.
The cwd default depends on `ls` operands, not the previously accumulated redirect
targets; option termination and operand extraction share one loop. These repairs
introduce no additional manifest or differential count changes.

## Frozen limits and raw evidence

`021ad5d6c213c93997026445d57f73c5ec24a2fb` froze 65,536 serialized event bytes and
64 simultaneous delimiters before evaluator use. The 90-observation public-only
sweep and conservative selection are described in [the limits record](rust-slice-limits.md).
The 65,536-command-byte sweep includes a 65,584-byte event envelope; S20's actual
boundary materialization independently targets exactly 65,536 complete event bytes.
All twelve S20 rows are labelled: six N boundary cases and six F excess cases.

Raw evidence lives outside the checkout at `~/.cache/guard-fixtures/fixture-seat/`:

- `r4d-arm-{structured,brush,tree}.jsonl` and `r4d-measure.py` retain all 257
  records per arm; `r4d-differential-{brush,tree}.jsonl` retain all 1,289 per arm.
- `r4d-mismatch-inventory-{brush,tree}.jsonl` retain all 15 intended changes and
  921 outside operations, including every consumer observation.
- `r4d-summary.json`, `r4d-summarize.py`, `r4d-deltas.json` and `r4d-deltas.py`
  bind raw/source hashes, per-family counts, every changed tuple/wire row, both
  rounds' latency quantiles and exact two-line overlay/protected-plan preservation.
- `r4d-regression-before-fix.log`, `r4d-{cwd,options}-regression-before-fix.log`,
  `r4d-mechanisms-complete.log`, `r4d-ablation-{recursion,cwd,options}-{broken,restored}.log`,
  `r4d-ablation-summary.json` and `r4d-ablate.py` retain the three regressions and
  exact-byte fail/restore/pass proofs.
- `r4d-initial-*` preserve the first R4d measurement and ablation before the
  default-cwd and option-termination repairs; they are not the final source state.
- `r4d-release-build.log`, `r4d-rust-check-final.log` and
  `r4d-differential-test.log` record successful required checks: 28 tests, fmt,
  Clippy and cargo-deny. Existing duplicate-dependency warnings do not fail deny;
  dependencies and Cargo.lock did not change.

Preserved R4b evidence records the preceding implementation:

- `r4b-calibration.jsonl`, `r4b-calibration.py`: all 90 calibration observations.
- `r4b-arm-{structured,brush,tree}.jsonl`, `r4b-measure.py`: 257 records per arm.
- `r4b-differential-{brush,tree}.jsonl`: all 1,289 rows per arm.
- `r4b-mismatch-inventory-{brush,tree}.jsonl`: all intended changes, baseline
  conflicts and outside rows, with every consumer's actual tuple/wire comparison.
- `r4b-final-ablation-summary.json`, `r4b-final-ablate.py`,
  `r4b-final-ablation-<mechanism>-{broken,restored}.log`: all 19 final fail/restore/pass
  pairs and original byte hashes. Earlier 18-pair logs remain under `r4b-ablation-*`.
- `r4b-recovery-{before,after}-fix.log`: the stronger differential rejection of
  missing recovery boundaries and the recovered development/differential tests.
- `r4b-initial-*`: preserved earlier measurement, summary and mechanical logs.
- `r4b-owned-writer.log`: W01–W03 receipts, final bytes and preservation assertions.
- `r4b-release-build.log`, `r4b-rust-check-final.log`: release build and final
  required mechanical checks.
- `r4b-summary.json`, `r4b-summarize.py`: prior counts, outside reasons, calibration
  maxima, raw/fixture/source hashes and that round's restoration-hash verification.

## Open acceptance boundaries

D21's seven RETAIN conflicts are resolved in the selected slice. The 921 outside
rows, appdata[2]'s unmodeled content consumer and six observed U-C dynamic
configuration reads remain explicit limits. SSH
public-file identity/stat/inode, hard links, full shell/glob semantics and full
program tables are not accepted by this packet. Installed protocol, consumer
hook loading, actual Gate A, execution Gate B, verified execution owners X01/X02,
arbitrary-writer preservation and Go cutover remain open. Held-out and adversarial
review belong to the coordinator after the implementation is reported frozen.
No installation, registration, permission/TCC change, stopped OS experiment,
remote write or release was performed.
