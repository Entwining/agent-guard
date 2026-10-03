# Offline Rust P1 slice evidence

This development packet exercises a bounded Rust preflight library. Go remains
authoritative; the Rust package has no hook path, arbitrary submitted-code
execution, Go fallback or installed-consumer acceptance. The independent held-out
set and scrutiny files were not read. The evaluator and Gate A surrogate are in
`f63c7b24ff29cd2e628c867132abe5768e91f7be`; the test-only owned writer is in
`6aafdebc9c37fa9f851f61fba2a0a5362e772193`. Rejected recovery scopes also name their
identity, inspection or syntax boundary in `a86854e93f9274ad1d37821974a6ddda2c232555`.

## Development contract

The manifest contains 260 labelled rows: 248 counted non-writer cases, nine
metamorphic variants and W01–W03. The contract test checks all 257 non-writer rows
through both parser arms (514 row/arm checks); nine variants also compare with their
bases through both arms (18 comparisons). Assertions cover outcome and coverage,
wire exit/stdout/stderr, semantic reason owners, advice, concrete recovery,
lexical-before-probe ordering, operation starts, fixture results and child reaping.
The 16 mechanism tests and three parser/word/span unit tests protect distinct
boundaries. The two entry and two error tests preserve the initial failure
contract. With the differential and owned-writer tests, `cargo test --locked`
contains 27 tests.

Closed operations read/write/list only owner-created synthetic files. They never
execute submitted shell or interpreter code. The worker is a fixed test program;
its ready PID receipt, `wait` result and subsequent `try_wait` establish completion
and reaping at the injected failure boundary. This does not establish a production
checker deadline. W01–W03 establish single-owner state and byte preservation in
test code, without claiming concurrent-writer or crash-recovery acceptance.

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
| Structured/native policy only | 248 | 5.791 | 58.209 | 141.916 | 287.334 | 9 | 5.280 | 385.301 |
| Brush | 248 | 23.083 | 100.791 | 1563.292 | 1856.416 | 9 | 1.966 | 32.227 |
| Tree-sitter | 248 | 26.625 | 130.250 | 2485.417 | 2549.958 | 9 | 1.946 | 32.059 |

Latency samples include variants. Timing starts after fixture construction and
event materialization, covers evaluation and rendering, and excludes the original
closed operation and recovery recheck. Worker rows include process arrangement
and reaping and are separated from normal evaluation. Quantiles use nearest rank.
The first worker sample in the final run took 385.301 ms. The earlier 590.417 ms
sample is retained in `r4b-initial-arm-structured.jsonl`; neither is filtered.
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

| Family | RETAIN match | CHANGE match | Baseline conflict | Outside slice | Link metadata match | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| appdata | 54 | 3 | 5 | 80 | 0 | 142 |
| clients | 0 | 0 | 0 | 119 | 0 | 119 |
| credentials | 111 | 1 | 1 | 180 | 0 | 293 |
| options | 3 | 0 | 0 | 11 | 0 | 14 |
| programs | 3 | 0 | 0 | 64 | 0 | 67 |
| readers | 24 | 0 | 0 | 216 | 0 | 240 |
| search | 41 | 1 | 0 | 60 | 0 | 102 |
| cwd | 2 | 0 | 0 | 67 | 0 | 69 |
| shell | 76 | 8 | 1 | 96 | 0 | 181 |
| interpreters | 10 | 0 | 0 | 28 | 0 | 38 |
| filesystem_link | 0 | 0 | 0 | 0 | 24 | 24 |
| Total | 324 | 13 | 7 | 921 | 24 | 1289 |

There are zero Rust-defect classifications within these selected mechanisms,
39 CHANGE consumer observations, 972 matching RETAIN observations and 21
baseline-conflict observations per arm. This is not all-RETAIN conformance.
The seven unchanged legacy N labels conflict with current requirements:

| ID | Input/context | Current requirement | Actual, every consumer |
| --- | --- | --- | --- |
| appdata[58] | `ls ~`, project cwd | Broad HOME listing | D |
| appdata[62] | `ls /`, project cwd | Broad root traversal | D |
| appdata[92] | `ls /tmp`, App Data cwd | D17 protected cwd | D |
| appdata[120] | `ls -ltr ~`, project cwd | Broad HOME listing | D |
| appdata[121] | `ls -ltr`, HOME cwd | Broad HOME listing | D |
| credentials[87] | `ls`, SSH cwd | D17 protected cwd | D |
| shell[133] | `if true; then ls; fi`, HOME cwd | Broad HOME listing | D |

The baseline-conflict classification is an implementation report grounded in the
frozen requirement and command records; it does not change the overlay or close
coordinator adjudication. All 13 CHANGE records match the new outcome, coverage,
reason/advice and canonical recovery/recheck requirements. No original Go
expectation or classification row was changed.

Outside mechanisms include program adapters outside the development subset
(including Shell grep beyond the rg development partition), SSH public-file
stat/inode identity, stream-fed operands/code, non-development glob/expansion and
workflow partitions, command-local/exported assignments, complex heredoc grammar,
post-link parent traversal and case-folded firmlink spellings. Every outside row,
consumer observation and specific reason is retained in the mismatch inventories.

## Causal ablations

Every row below records a compiled, named test failure (exit 101), exact-byte
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

## Frozen limits and raw evidence

`021ad5d6c213c93997026445d57f73c5ec24a2fb` froze 65,536 serialized event bytes and
64 simultaneous delimiters before evaluator use. The 90-observation public-only
sweep and conservative selection are described in [the limits record](rust-slice-limits.md).
The 65,536-command-byte sweep includes a 65,584-byte event envelope; S20's actual
boundary materialization independently targets exactly 65,536 complete event bytes.
All twelve S20 rows are labelled: six N boundary cases and six F excess cases.

Raw evidence lives outside the checkout at `~/.cache/guard-fixtures/fixture-seat/`:

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
- `r4b-summary.json`, `r4b-summarize.py`: counts, all outside reasons, calibration
  maxima, raw/fixture/source hashes and current restoration-hash verification.

## Open acceptance boundaries

The seven RETAIN conflicts need coordinator adjudication. The 921 outside rows
and six observed U-C dynamic configuration reads remain explicit limits. SSH
public-file identity/stat/inode, hard links, full shell/glob semantics and full
program tables are not accepted by this packet. Installed protocol, consumer
hook loading, actual Gate A, execution Gate B, verified execution owners X01/X02,
arbitrary-writer preservation and Go cutover remain open. Held-out and adversarial
review belong to the coordinator after the implementation is reported frozen.
No installation, registration, permission/TCC change, stopped OS experiment,
remote write or release was performed.
