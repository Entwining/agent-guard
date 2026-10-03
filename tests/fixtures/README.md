`contract.jsonl` contains 1,265 behavior partitions, each with Claude Code, Codex and Pi expectations for verdicts, denial reasons and advice. `$H` and `$R` denote the synthetic home and its parent; `$U` denotes the synthetic username. Command placeholders do not replace longer shell variable names. `filesystem.json` declares empty files and links created inside that synthetic home.

The initial expectations were captured from the v0.5.0 implementation at `a8c3a38f6c1f1be473ed869c8e1d74a038a31b75`. They are regression data, not an executable reference implementation. Update them only for an intentional contract change, together with a regression that explains it. Go tests always load these fixtures; no exporter or environment opt-in is required.

## Rust slice development manifest

`rust-slice-dev.jsonl` is a requirement-owned draft, with one consumer/scenario case per line. It merges the round-1 Sol and DeepSeek scenarios with the frozen round-2 decisions. These expectations have not been executed, and `make rust-check` does not load this draft. Go results are comparators, not labels. This file contains development cases only; it neither imports nor establishes coverage of the independent held-out set.

Source paths beginning `fixture-reference/` refer to `/Users/fixture/.local/state/guard-fixtures/migration-fixtures/`. The normative record is `decisions.md` v2, whose Round-2 resolutions override its earlier text. `origin_ids` identifies merged scenarios in `r1/sol-slice-design.md` and `r1/deepseek-challenge.md`; fixture family/index references identify historical comparators. A source object has `path` or `url`, plus a named `section`; no source's current Go output supplies the expected label.

### Synthetic inputs

`$E` is a new external harness-owned directory; `$H=$E/home`, `$P=$H/project`, and `$C=$H/Library/Containers/com.example.fixture`. These are substitution markers, including inside quoted source/data, not real shell variables. Replace only complete marker tokens, never prefixes of `$HOME` or another shell variable. The Shell environment binds its actual `HOME` to the synthetic `$H`. No row authorizes executing submitted Shell, reading real stores/App Data, changing permissions, or resuming the stopped OS experiment.

Every row names `synthetic_setup.profile=base-v1`, which declares only owner-created UTF-8 surrogates: `$P/input.txt` is `needle\n.env\nprocess.env.SHELL\n`; `$P/notes.txt` is `ORIGINAL_NOTES\n`; `$P/untracked-work.txt` is `UNTRACKED_WORK\n`; `$P/out.txt` is `KEEP\n`; `$P/fixture.txt` starts absent. `$C/data.txt`, `$H/.ssh/id_rsa`, and `$P/.env` each hold `SYNTHETIC_CANARY\n`, never credentials. `$P/data-link` points to `$H/Library/Containers`. Per-row `files`, copy, target and fault declarations extend or replace this setup. A session ID in a `write_stdin` input denotes a fixture session, never a live session.

The evaluator only observes input. A separate closed, harness-owned operation may supply the named task/effect witness and positive control. A recording Probe checks lexical protection before each probe, stops before protected App Data, and uses only readlink on safe prefixes in the selected alias traversal. Witnesses must detect actual surrogate access, not infer absence from empty stdout. The offline driver is Gate A's surrogate; its permit control starts and completes, denied/failed checks have zero operation starts, and bypassing the gate must fail that assertion. Installed Gate A and actual execution Gate B remain separate open gates.

### Row schema

| Field | Contract |
| --- | --- |
| `id` | Unique scenario/consumer ID. |
| `parent_task_family_id` | Requirement-owned split unit, independent of consumer and expected outcome; all cases of a parent stay together. |
| `requirement_sources`, `origin_ids` | Governing requirements and merged scenario provenance. |
| `consumer` | `claude`, `codex`, `pi`, or `owned-writer` for the three writer experiments. |
| `dialect_executor_assumption` | Syntax, executor evidence boundary and trusted fixture context; tool name is not an executor attestation. |
| `provenance_form` | Direct operand, data/code sink, expansion, external script/config, interactive input, fault or boundary recipe. |
| `tool`, `input`, `cwd` | Consumer-native offline event fields and synthetic cwd. Codex equivalents of Read/Grep use Bash. Pi uses lowercase tool names and native `path`; adapter projection is separate. `raw_event_bytes` replaces the serialized event for malformed-JSON cases. |
| `task_objective`, `synthetic_setup` | Intended result and base profile plus case-specific resources/faults. Cwd alone does not supply the user's objective. |
| `allowed_effects`, `prohibited_effects` | Effects observed in the closed witness and claims/effects that must be rejected. Limited continuation does not promise that a hidden dynamic protected effect is prevented. |
| `outcome_class` | N, A, D, U-C, U-R, U-O, F, W; `null` when unresolved. |
| `expected_coverage` | Supported/limited preflight, named outside-tool coverage, failed check, owned writer, or unresolved. Failed checks have no completed Evaluation. |
| `reason_contract` | Semantic effect/failure, concrete operation or owner-action alternative and excluded scope; no exact Go sentence or raw event/value requirement. Quiet outcomes have no reason. |
| `advice_expectation` | Presence/absence and exact consumer; A is Claude offline context only, and protection always takes priority. |
| `recovery_objective` | Rechecked next operation or explicit owner action, task result, excluded scope and automatic-application support; `null` where no recovery applies. A narrowed result cannot complete a whole-HOME task. |
| `observer` | Outcome/wire, probe order, operation-start, task/effect, state or lifecycle observations that would reject the plausible faulty mechanism. These are future assertions, not recorded passes. |
| `coverage_limit`, `status` | Evidence boundary and `labelled` or `unresolved`. `labelled` means a requirement label is assigned, not implementation acceptance. |
| `competing_readings` | Required for unresolved rows: candidate outcomes/coverage and their rationale. Unresolved rows are excluded from protection denominators until adjudicated. |
| `conditional_outcome` | Frozen S14 rule: U-R for active unsupported Zsh syntax; D is mandatory if independent nested-effect extraction identifies the protected read. Both branches block original execution. This is a settled condition, not permission to choose whichever label is easier. |
| `fault_injection`, `go_comparator`, `writer_state` | Optional explicit injection, historical comparator, or durable writer state. |

N permits quiet ordinary work; A permits with relevant soft advice; D denies an identified protected effect. U-C continues the existing limited preflight quietly while preserving the gap in the library record. U-R rejects unsupported syntax with a concrete compatible continuation. U-O reports the missing verified execution owner for a newly required guarantee. F blocks on operational failure, never an empty success. W records `Committed`, `Conflict` or `PartialStagedFailure` in the single harness-owned writer and is excluded from every protection denominator; it promises no production Write/Edit/Bash preservation.

The named classification step `effect-sink-v1` derives effect and sink from the requirement-owned operation: structured path fields and modeled argv roles supply read/write/search/list operands; inline data, regex/patterns, messages and quoted data heredocs remain data; substitutions, interpreter/eval sources and unquoted heredoc expansions carry nested code effects. Unknown programs retain their gap while explicit protected operands still deny; unknown tools retain their tool class without inventing an effect adapter. Resource class follows lexical scope and the selected alias. Apply this derivation before the D9 six-axis stratification key (consumer, tool class, effect, outcome class, resource class, sink class), with dialect/executor state and provenance as additional axes. These axes never replace the parent-family split unit.

Ordinary `+(a|b).txt`, `*(.)` and `!(x)` remain globs, with conservative Bash-extglob/Zsh reach, and the public cases stay N. Executable qualifier bodies and active parameter flags/process substitution/options remain U-R/D; inert quoted data stays N. The detector/source-span/dual-parser assertions are future implementation gates. Whole-script parse failure independently remains U-R. Any verdict-ablation claim must account for that independent rejection rather than call unchanged rejection detector evidence.

HOME-rooted structured Grep includes explicit, empty and omitted paths and a HOME root with a glob. Its reason names broad traversal and a project-scope continuation, including excluded HOME scope; it does not inherit Go's credential-specific reason. Probe EACCES is F; identity bounds and additional link chains/firmlinks/loops/SSH identity are outside this selected development slice. Codex `apply_patch` uses patch text under `command` and remains outside P1 coverage, as does `write_stdin`; neither is a fallback for a blocked Bash call.

The unresolved rows retain the R2 name-only hidden-listing N candidate versus the old HiddenSearch denial, and separate input-byte/nesting boundary/excess recipes whose numeric bounds have not been calibrated or frozen. Each recipe denotes one frontier case. Materialize its concrete event and freeze its numeric bound before acceptance: supported boundary yields the ordinary result, one-unit excess yields F. No candidate threshold or Go output resolves these rows automatically.
