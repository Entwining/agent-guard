# Offline Rust R4h M1 evidence

M1 implements D24/D25's overlay revision, P0 records, P1 words and parser-arm
status. Brush is the sole semantic acceptance arm. The 1,289-row differential
has 121 in-slice RETAIN conflicts, down from 140 at `83ac433`; 13 word-semantic
repairs and six adjudicated overlay corrections explain all 19 improvements.
Every previously matching in-slice RETAIN observation and all 21 CHANGE rows
still match. There are zero in-slice row or consumer regressions. This milestone
does not establish Go conformance: `make rust-check` still fails at the complete
differential, as D24 requires.

Go remains authoritative. The Rust trial is on no hook path, and its packaged
`agent-guard-rust-slice` remains version-only. Production Go, executables, hooks,
public setup, dependencies and Cargo.lock are unchanged. No holdout material was
read. Work stops at M1; fresh DeepSeek mechanism review and fresh Opus 5.5
cross-layer review are coordinator-owned gates before M2, not completed verdicts.

## Source and measurement binding

The starting delivery is `83ac433715a64571c7f33bf5badbd92d1e11e601`.
Its R4g measurements were made at `b2626ef5b19d5dcbf4f8f787d396ae1f8561ded8`;
`r4g-final-delivery-binding.json` verifies the documentation-only relationship.
The bound parent differential SHA-256 is
`07d75420f4cabb8e26cb2a1c25ec4d109ddeb5c98b5545b5424325a99e09c130`.
The overlay-only first M1 commit is
`58cefb489459c2215bd0f6a59ca82d21bf2ab618`.

All raw artifacts and scripts named here are under
`~/.cache/guard-fixtures/fixture-seat/`. `r4h-m1-measurement-binding.json` binds 52 Rust
source/test/fixture/example/build inputs by SHA-256, the unchanged production
Git objects, raw artifacts, five binaries, host/tool versions and four preserved
plan-file hashes. Its measured parent is the overlay commit with the recorded
M1 working-tree changes. `r4h-m1-final-delivery-binding.json` records the delivered
commit, parent, evidence-document hash, source/raw/binary equality and final
status. Measurement inputs must match the delivered bytes; commit identity alone
is insufficient.

Measurements use Darwin 27.0.0 arm64, rustc/Cargo 1.98.1 and Go 1.27.1. HOME,
fixtures, Cargo target/cache and Go build/cache paths are synthetic or external
to the checkout. No credential values or process-environment USER are read by
Rust. No concurrency, cache, bypass configuration or threshold was added. The
65,536-byte event and 64-delimiter limits remain unchanged; their separate
calibration evidence remains in `docs/rust-slice-limits.md`.

| Frozen input | SHA-256 |
| --- | --- |
| Original Go corpus | `1e223c6453d6883acc88af9967beab4251ba0fc6d636a1186482b6e4b524c695` |
| Filesystem setup | `b1d51062925ccbfdfae5c8ffbc3e130e25391f31b05207448ee02be8cc874b3e` |
| D22 scope inventory | `a3e41fd04bf1daf575c7312f9a275d298203ac7493228f6517dbcbb1fccd18ac` |
| Dev manifest | `385e395dbee8855c2985a7b6d2f92083b898e1614573ad4f7b9ac71b8597be10` |
| D22 review regressions | `4aa4102035d3bcc92ea04c7261e0dc6e6a57002d5c1415a02958903b413cfdb9` |
| Revised overlay | `a59ddafbc4557a901670a7e7569295257d2c37df2fb92d54c08895d24883c4e6` |

Only the six adjudicated overlay lines changed, with `decision_id` and D24/D25
requirement references. The original Go corpus, scope, setup, dev labels and
review inputs remain byte-identical.

## P0/P1 ownership

`src/record.rs` provides owned Word, Redirect, Items, Command, Script, Fragment
and Target records with typed roles, directions, flags, effects, walk and via.
Words carry Text/Raw/Expands/Globs/Vars/Role/Value/Pwd through complete alternative
argv readings and target projection. Option values retain the originating Word's
metadata, including independently supplied `fd --search-path` values; target
identity receives Globs and Expands directly rather than reconstructing them
from text. Command stores program index, wrappers, flags, items, stdin channel,
cwd and pipeline/nesting provenance. Heredoc/herestring bodies are redirects,
with stdin data referencing their indices instead of duplicating the body.
Script records parse failure and can carry uninspectable fragments.

`src/shell/words.rs` uses pinned brush-parser 0.4.0 `word::parse`,
`WordPiece` and `parse_brace_expansions`. The old text scanner is deleted.
ANSI-C decoding follows Go's hex, Unicode, octal and escape mapping. Brace
sequences become glob reach; quoted braces and glob characters remain literal.
`~+`, `$PWD`, `$(pwd)` and backticks with `pwd -L/-P` carry cwd and Pwd metadata.
Unknown parameter/command/arithmetic expansions preserve their spelling and set
Expands; nested parameter fragments, indices and arithmetic retain variables
and nested observations. Quoted heredoc data stays literal; expanding heredoc
bodies use the heredoc parser, including literal apostrophes.

USER is an immutable optional host fact in Context and the example's accepted
context. Missing USER uses Go's `unknown` fallback; an explicitly empty USER
stays empty. A shell USER assignment does not rebind the host fact. Account
lookup and process-environment USER access are absent from this Rust path.
The unresolved App Data fragment consumer follows `native/rules/appdata.go`;
ordinary resolved App Data identity remains HOME-scoped. D22's conservative
parenthesized-group reach remains an explicit decided layer, including quoted
groups; P1 quoting gates ordinary brace/glob reach.

P0 establishes the representation and current transport, not all later owner
population. Word role assignment, Command wrapper/program resolution, flags,
Items and executable stdin kinds, Script uninspectable fragments, Target Sends,
detailed Via and unresolved-path provenance still belong to P2/P3/P4/P5/P6/P7.
Current defaults are not evidence that those owners have been ported.

## Parser arms and comparator

Tree-sitter's semantic adapter and Arm variant are removed. All semantic
acceptance loops use Brush, and the evaluation interface rejects `arm: tree`.
Structured-only remains an explicit observation-limited development control,
not an acceptance arm. Tree-sitter dependencies remain pinned and unchanged
because `tests/parser_compare.rs` is a test-only parse comparator.

The comparator accounts for 1,289 legacy/setup rows, 248 dev rows (including the
nine existing variants), and 13 additional parse variants: 1,550 rows total.
There are 1,397 shell parse comparisons, 243 review leads, and 41 parse-success
disagreements. It reports parse status and checked UTF-8 statement/word spans;
its AST traversal and grammar boundaries differ, so span differences are leads,
not semantic failures or Go parity. Non-shell/malformed/setup rows carry explicit
status. There are zero semantic verdicts or protection observations in this
report. The five historical Tree-only semantic conflicts leave the denominator
under D25; no corpus or scope row was removed.

## Requirement-owned differential

Scope remains the frozen Go effective-program inventory plus D22's independent
program table; expected labels and Rust outcomes do not select it. The report
accounts for 1,265 operations through three consumer projections (3,795
observations), plus 24 setup-metadata checks. There are 963 in-slice operations
and 302 outside operations (906 observations). Metadata is neither an operation
nor an outside verdict. Historical Codex structured projections are offline
adapter checks, not enrolled-runtime claims.

| Family | RETAIN match | CHANGE match | RETAIN conflict | Outside | Metadata | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| appdata | 119 | 5 | 10 | 8 | 0 | 142 |
| clients | 0 | 0 | 0 | 119 | 0 | 119 |
| credentials | 233 | 3 | 9 | 48 | 0 | 293 |
| options | 14 | 0 | 0 | 0 | 0 | 14 |
| programs | 38 | 3 | 6 | 20 | 0 | 67 |
| readers | 112 | 0 | 50 | 78 | 0 | 240 |
| search | 88 | 1 | 8 | 5 | 0 | 102 |
| cwd | 45 | 1 | 23 | 0 | 0 | 69 |
| shell | 137 | 8 | 12 | 24 | 0 | 181 |
| interpreters | 35 | 0 | 3 | 0 | 0 | 38 |
| filesystem_link | 0 | 0 | 0 | 0 | 24 | 24 |
| Total | 821 | 21 | 121 | 302 | 24 | 1289 |

Consumer categories are 2,479 matching RETAIN, 63 matching CHANGE, 347
Rust_defect and 906 outside observations. Eight Claude-only advice conflicts
explain why defect observations are not three times conflict rows. All original
15 CHANGE rows and the six new ones match their complete class, coverage,
reason/advice, blocking wire, recovery boundary and continuation contracts.
`r4h-m1-summary.json` and `r4h-m1-summarize.py` reproduce these counts.

Go-deny/Rust-permit includes N/A/U-C with a permitting consumer wire. The current
coverage split is:

| Scope/category | SupportedPreflight N | LimitedPreflight U-C | Total |
| --- | ---: | ---: | ---: |
| In-slice Rust defects | 168 | 123 | 291 |
| In-slice decided changes | 6 | 0 | 6 |
| All in-slice | 174 | 123 | 297 |
| Outside programs | 0 | 285 | 285 |
| Total | 174 | 408 | 582 |

The six decided permits are search[84] (D16) and appdata[64] (D25), three
consumers each. R4g had 201 SupportedPreflight N and 129 LimitedPreflight U-C
Rust defects (330 total), plus three decided permits. M1 reduces the defect
escapes to 291; it does not claim protection for any of the remaining permits.

There is one outside outcome delta, `clients[41]`, on all three consumers:
Go D, prior Rust D, current Rust U-C. Its quoted curl file URL uses curl's own
bracket-range expansion. P1 correctly preserves the shell word's Globs=false;
the generic unmodelled-program path no longer treats it as a shell glob. Curl's
URL-glob owner is outside P1 under D25, and its frozen bucket stays outside.
This increases outside Go-deny/Rust-permit from 282 to 285. The zero-regression
claim applies to acceptance-owned observations, not outside program behavior.
`r4h-m1-outside-class-deltas.json` preserves this complete delta; the full raw
report and summary retain every outside bucket, including zero-escape buckets.

## Parent failures and current passes

The following are all rows that change from a parent Rust_defect classification
to a matching current contract. All three consumer observations agree on each
listed class. Six are expectation corrections, not implementation repairs;
their actual classes are unchanged. The other 13 are word-semantic repairs.

| Row | Actual at 83ac433 | Actual at M1 | Overlay | Owner |
| --- | --- | --- | --- | --- |
| `appdata[48]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `appdata[64]` | N | N | RETAIN → CHANGE | D25 |
| `appdata[83]` | D | N | RETAIN → RETAIN | P1 quoting/Globs |
| `appdata[85]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `appdata[132]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `credentials[172]` | F | F | RETAIN → CHANGE | D24 |
| `credentials[266]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `options[8]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `options[9]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `options[10]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `options[11]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `programs[28]` | D | D | RETAIN → CHANGE | D24 |
| `programs[36]` | D | D | RETAIN → CHANGE | D24 |
| `programs[39]` | D | D | RETAIN → CHANGE | D24 |
| `cwd[15]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `cwd[39]` | UC | D | RETAIN → RETAIN | P1 word semantics |
| `cwd[40]` | UC | D | RETAIN → RETAIN | P1 word semantics |
| `cwd[41]` | N | D | RETAIN → RETAIN | P1 word semantics |
| `cwd[67]` | D | D | RETAIN → CHANGE | D25 |

`r4h-m1-differential-brush.jsonl` is the release differential.
`r4h-m1-mismatch-inventory-brush.jsonl` retains all 121 current conflict rows
with source, scope and complete consumer observations. The summary compares
both row and consumer categories against the hash-bound R4g parent, asserts
zero in-slice regressions, unchanged scope, 963 in-slice operations, all 21
CHANGE matches and all prior matching RETAIN observations. No expectation,
consumer, source string or advice-only mismatch is silently dropped.

## Dev, review, D23 and wire checks

The unchanged dev manifest has 260 rows: 239 counted evaluator cases, nine
metamorphic variants, nine harness-only lifecycle rows and three owned-writer
rows. Brush checks 248 evaluator rows and nine variant/base comparisons. The
counted class vector is N 70, A 3, D 96, U-C 28, U-R 21, U-O 6, F 15. Required
zero starts pass 138/138 counted requirements, or 144/144 including variants.
Neither writer nor lifecycle rows enter protection denominators. Six U-C
runtime-configuration witnesses still permit synthetic protected reads;
execution Gate B remains open. The two non-Claude replacement-advice rows still
have library A and quiet wire; conditional denials require the typed nested
App Data effect, independently of the synthetic operation receipt.

All 54 non-differential tests pass, including dev, variants, review regressions,
D23, eight F kinds on three consumers, JSONL interface, lifecycle and writer
contracts. The 87 named D22 review cases are checked through Brush. Historical
R4g two-arm before/after evidence and 60 ablations remain in the R4g artifacts;
they are historical records, not fresh M1 two-arm measurements.

D23's five tests still pass public-looking directory denials (`.ssh/config.d`,
`directory.pub`, `known_hosts.backup`), public regular/hard-linked controls,
ProbeFault blocking, symlinked SSH-root identity and parent/search inode
comparisons. App Data stays lexical-first/readlink-only. SSH stat is confined
to the Go identity comparisons after lexical protection. Private/protected
candidate spellings retain the following direct identity probe receipts:

| Candidate | readlink | stat |
| --- | --- | --- |
| `$H/.ssh/id_rsa` | [] | [] |
| `$H/.ssh/id_ed25519` | [] | [] |
| `$H/.ssh/keys/id.pub` | [] | [] |
| `$H/.ssh/CONFIG` | [] | [] |
| `$H/.ssh/id.PUB` | [] | [] |
| `$H/.ssh/known_HOSTS` | [] | [] |

`r4h-m1-other-tests.log` records these receipts. A whole event can separately
inspect its public cwd. Directory hard-link identity remains tested at the Probe
metadata boundary; regular hard links and root aliases are real synthetic
filesystem objects. This does not exercise real credentials or private keys.

A built Go comparator and Rust release `examples/evaluate` additionally agree
on permission for 30 out-of-corpus word variants across three consumers: 90/90
observations. Three harmless unresolved-fragment controls have Go N / Rust U-C,
explicitly permitted under the coverage contract. The variants cover ANSI-C,
brace quoting/sequences, immutable/missing/empty USER, pwd forms, Expands,
escapes, both heredoc modes, nested variables and independent option metadata.
`r4h-m1-go-variants{,-requests,-rust}.jsonl`, the summary and script retain
inputs and exact outputs. An earlier harness incorrectly represented absent Go
USER as present-empty; its three mismatches are preserved in
`r4h-m1-go-variants-harness-empty-vs-absent.*`. After matching Go's LookupEnv
contract, both absent and empty cases agree without an implementation change.
This finite variant set is not arbitrary shell or program equivalence.

## Causal ablations

Every new mechanism below has a compiled named-test assertion failure (101),
a failing relevant differential row, exact-byte SHA-256 restoration and the
same named test passing (0). Each restored full differential has 1,289 rows and
its selected rows match. All restored file hashes equal the final measured
source. No compile failure is counted as an ablation success.

| Ablation ID | Named test | Differential rows that fail | Broken / restored |
| --- | --- | --- | --- |
| `ansi` | `ansi_c_words` | `credentials[266]` | 101 / 0; exact bytes |
| `brace` | `brace_sequence_and_quoting` | `options[8]`, `options[9]`, `options[10]`, `options[11]` | 101 / 0; exact bytes |
| `denominator` | `parser_acceptance_denominator` | `appdata[85]`, `credentials[266]`, `options[8]` | 101 / 0; exact bytes |
| `expands` | `unresolved_word_fragments` | `appdata[48]`, `appdata[132]`, `cwd[15]` | 101 / 0; exact bytes |
| `globtransport` | `brace_sequence_and_quoting` | `appdata[83]` | 101 / 0; exact bytes |
| `nestedvars` | `unresolved_word_fragments` | `credentials[135]` | 101 / 0; exact bytes |
| `pwd` | `pwd_word_semantics` | `cwd[39]`, `cwd[40]`, `cwd[41]` | 101 / 0; exact bytes |
| `user` | `user_is_a_host_fact` | `appdata[85]` | 101 / 0; exact bytes |
| `wordpieces` | `word_piece_record_transport` | `appdata[49]`, `appdata[50]`, `shell[20]` | 101 / 0; exact bytes |

The denominator mutation adds StructuredOnly as a semantic acceptance arm.
It produces 2,578 rows and fails the selected rows on that added arm; restoring
Brush alone restores 1,289 rows and passing selected rows. Tree's removed
semantic adapter cannot be re-enabled with a flag. This mutation demonstrates
that the acceptance boundary is observable rather than claiming Tree parity.

`r4h-m1-ablate.py` implements the mutations and guaranteed byte restoration.
For each ID, `r4h-m1-ablation-<id>-binding.json` records the exact mutations,
source hashes, runtime test failure, failing differential observations and
restored passes. `-broken-test.log`, `-broken-differential.jsonl`,
`-restored-test.log` and `-restored-differential.jsonl` are the raw evidence.
Unpopulated later-owner model fields are not claimed as completed decision
mechanisms; their P0 shape does not substitute for later causal validation.

## Mechanical checks and remaining owners

| Check | Current result | Raw evidence |
| --- | --- | --- |
| fmt | Pass in required check | `r4h-m1-rust-check.log` |
| warnings-denied Clippy, all targets | Pass | `r4h-m1-clippy.log` |
| `make rust-check` | Fails only at `every_legacy_row_is_accounted_for`: Cargo 101, make 2; 121 conflict rows | `r4h-m1-rust-check.log` |
| Separate remaining tests | 54 pass, zero failures; does not replace the failed complete check | `r4h-m1-other-tests.log` |
| cargo-deny | Advisories/bans/licenses/sources pass; existing duplicate-version warnings | `r4h-m1-deny.log` |
| Locked release bins/examples | Pass | `r4h-m1-release-build.log` |
| Parse comparator | 1,550 rows, zero semantic observations | `r4h-m1-parse-comparator{.log,.jsonl}` |
| Packaged binary | `--version` succeeds; no arguments fail with no check performed | `r4h-m1-version{,-only-no-args}.log` |
| Dependencies, lock, production and four plan files | Byte/object-identical to parent | Measurement and final-delivery bindings |

The remaining 121 conflicts require later semantic owners, not row exemptions:
P2/P7 statement scope, bindings/cwd and physical resolution (M2); P6 lexical
credentials plus P3 wrapper/stdin and P4 stream producers (M3); P5 target roles
and P6 advice/secrets rules (M4). `cwd[26]` still needs Go's bounded-directory
collapse. `programs[29]`, `programs[44]` and other Git/tar/reader role differences
remain Rust defects. `shell[175]` still treats a long git commit message as a
path and returns ProbeFault; it is a target-role defect, not ResourceLimit.
Go-permit RETAIN rows such as appdata[81], credentials[20]/[206], shell[0],
programs[42]/[64] and cwd[20,22,23,24,57] remain defects; appdata[83] now matches.

The old scanner and Tree semantic path are deleted. Later-owner patches remain
explicitly pending: the credential-directory suffix branch in `src/policy.rs`
(P6 lexical owner, M3), `json.load` text gap in `src/targets.rs` (P3 interpreter
owner, M3), and Git/tar normalization with empty HOME in `src/targets.rs`
(P5 role owner, M4). Existing content-consumer and wrapper tables likewise await
their owners. They were not expanded into additional command-shape exceptions.

Fresh milestone reviews, complete Go conformance, installed protocol/hook
loading, real consumer Gate A, Gate B/configuration reads, execution owners,
arbitrary-writer preservation, production performance acceptance and Go cutover
remain open. No M2 work, install, registration, TCC/permission change, remote
write, release or real protected-data experiment was performed.
