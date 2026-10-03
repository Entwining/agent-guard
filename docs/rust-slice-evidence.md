# Offline Rust R4h M1.1 evidence

M1.1 implements D26 2a–g against `6afdc61`. It closes the reviewed quoted Git
pathspec regression and the reviewed P1 file-URL, nested-brace and mixed-range
losses. The original 1,289-row corpus has **zero outcome/category flips and
zero new row or consumer conflicts** against M1. That is a corpus claim, not
arbitrary shell/program equivalence. M1's earlier broad zero-regression claim
missed quoted Git pathspecs because those inputs were absent from the corpus.

Go stays authoritative. Production Go, executables, hooks, public setup,
dependencies, Cargo.lock and `src/main.rs` remain unchanged. The packaged
`agent-guard-rust-slice` supports only `--version`. No holdout material, real
credential store, private key or declared environment file was read. Work stops
at M1.1; M2 has not started. M1 acceptance still requires both coordinator-owned
Opus 5.5 and DeepSeek fix-verification verdicts (D26 1).

## Source and measurement binding

The baseline is `6afdc61a5d0c09fe37d045e5a145ac7edd91a8c3`. Its differential
SHA-256 is `7dda35b7137b387025eed6f03042e93dfaf74f3cb5820110be01204d3d73f806`,
verified through its measurement and final-delivery bindings. The separate
overlay-label commit is `3c2fc0969898e71bae5a57192ddc9efc7eee8df6`, parent
`6afdc61`. It changes four `D24 v2.8` labels to the actual `D24 v2.7` and updates
the checker hash. Parsed overlay expectations are unchanged after replacing
only those labels; no verdict changes are hidden in that commit.

All new raw artifacts/scripts named below are under
`~/.cache/guard-fixtures/fixture-seat/`, with the `r4h-m1-1-` prefix. The measurement
binding records source, fixture, raw artifact, binary and preserved-plan hashes;
the final-delivery binding checks those bytes against the delivered commit and
records its parent and evidence-document hash. HOME, fixtures and build/cache
paths are synthetic or external to the checkout. No concurrency, configuration
bypass or threshold was added. Existing event/delimiter limits are unchanged.

| Input | Current SHA-256 | Change in M1.1 |
| --- | --- | --- |
| Original Go corpus | `1e223c6453d6883acc88af9967beab4251ba0fc6d636a1186482b6e4b524c695` | None |
| Filesystem setup | `b1d51062925ccbfdfae5c8ffbc3e130e25391f31b05207448ee02be8cc874b3e` | None |
| D22 scope inventory | `a3e41fd04bf1daf575c7312f9a275d298203ac7493228f6517dbcbb1fccd18ac` | None |
| Dev manifest | `385e395dbee8855c2985a7b6d2f92083b898e1614573ad4f7b9ac71b8597be10` | None |
| Classification overlay | `87171281c243ebceefed14d6fbb103c5f75187239dab3c449652d3d156b63ed5` | Four provenance labels only |
| D22 review regressions | `44d12aea41a947e0c1996071b70bb9777293bfa6b58f0b3d4bbd63060debc632` | Four quoted-group expectations revised explicitly by D26 2d |
| M1.1 regressions | `3a30251e6159235f512dc07f3bcc947ec5bf13571b57a963a7752609ec47250f` | 21 new source cases |

## Owner repairs and evidence boundary

- Git uses the Go `git.go:95–104` owner override: pathspecs apply when the
  subcommand's effect is not name and the subcommand is not grep. The original
  operand's `*?[` decides Glob for both its target and its `rev:path` target,
  including file-option targets. Shell quoting does not override Git expansion.
  Git grep's quoted literal control remains N. Full P5 Git role parity is pending.
- Brace interpretation follows **D1/D26's Bash/Zsh union**, not Go's sequence
  regex alone or Brush's brace grammar alone. Mixed letter/digit ranges have
  conservative glob reach; signed numeric reach starts with a digit or minus
  sign, and the possible Zsh literal is retained. Quoted list members
  still expand. The filesystem owner splits nested lists by depth, as Go Braces
  does. Existing conservative sequence reach and the alternatives bound remain;
  this finite variant set does not prove arbitrary brace expansion equivalence.
- Case-insensitive `file://` stripping exists at both Go-owned boundaries:
  absolute filesystem input and Word-to-Target projection. Each call site is
  independently ablated. Structured and shell variants exercise both.
- Parenthesized reach requires a patterned word. Single/double-quoted groups
  stay literal; unquoted extglob keeps D1 union reach. Four older D22 review
  rows (`group-env`, `group-filter-env`, `group-npmrc`, `group-ssh`) now expect N,
  all confirmed Go N. Their fixtures explicitly cite D26; the Go corpus is intact.
- In/Out Redirect Vars are empty; heredoc/herestring Vars remain. Target's
  constructor defaults Glob/Search to false; structured search roots and
  program-owned search/glob options set their own fields. `ls` records List;
  the credential consumer preserves Go's distinction between listing and reading.
- RecordingProbe has a word-provenance constructor. The dedicated glob-word
  identity check uses the pattern-aware oracle and the quoted-word control uses
  the literal oracle. Direct attempted protected probes are rejected by that
  oracle. Other existing literal-mode callers are not claimed as a universal
  per-target provenance trace. There is no production-only testing seam.

`r4h-m1-1-shell-readings.json` independently confirms D26 with echo-only host
Bash/Zsh commands: `{n..9}` stays literal in Bash and includes n in Zsh;
`{+1..+1}` produces 1 in Bash and stays literal in Zsh; quoted list members
expand in both. Go's N for `ls ~/Library/{"Containers",Public}` remains the
coordinator-recorded baseline false-permit candidate, while Rust stays D.

P0 supplies typed transport, not all later owner semantics. Every currently
unfilled or heuristic field is explicit below; fixed D26 defaults do not imply
the later producers or consumers are ported.

| Record field | Current limitation | Later owner |
| --- | --- | --- |
| Word.role | Always Arg | P3 argv/wrappers, P5 program roles |
| Word.value | Text clone; operandValue option/at-sign projection absent | P5 |
| Word.pwd | Filled, not consumed as a state distinction | P2/P5 |
| Command.cwd | Tracked cwd shares the PWD variable namespace | P2, M2 |
| Command.program / wrappers | Some(0) / empty; no full effective-program resolution | P3/P5 |
| Command.shell | Always true on observed command records | P3 |
| Command.flags | Empty; adapters still infer flags from argv | P5 |
| Command.items | None; stream/listing producer heuristics remain | P4 |
| Command.stdin | Only None/Data; Shell/Code channels not populated | P3 |
| Script.uninspectable | Empty | P2/P3 statement/executable-fragment owners |
| Redirect.target for expanding heredocs | Known variables substituted; Go retains raw body | P3 |
| Target.unresolved | Path clone rather than unresolved provenance | P5/P7 |
| Target.command | None; no complete command association | P5 |
| Target.sends | Always false | P5 client/upload roles |
| Target.effect | Program-table heuristic; Git metadata still uses Name | P5/P6 |
| Target.walk / via | Partial adapter heuristics, not the complete Go maker | P4/P5 |
| Target.search | Explicit producers added; field still not read, SSH search uses Walk | P5/P7 |
| Target.glob | Word transport plus explicit Git/search overrides; other owners pending | P5/P7 |

The App Data unresolved-fragment consumer still omits Go's touches/scan gates
(P6/M4). The credential-directory suffix branch (P6/M3), `json.load` text gap
(P3/M3), empty-HOME Git/tar normalization (P5/M4), content-consumer and wrapper
tables remain pending; M1.1 does not add command-spelling exceptions to them.

## Corpus differential against M1

The frozen Go effective-program inventory and D22 table still own scope:
1,265 operations × three projections = 3,795 observations, plus 24 metadata
checks; 963 operations are in slice and 302 outside. Every prior matching
RETAIN observation and all 21 CHANGE rows still match their complete contracts.
Eight Claude-only advice conflicts account for the non-threefold defect count.

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

The current categories are 2,479 matching RETAIN, 63 matching CHANGE, 347
Rust_defect and 906 outside observations. Remaining conflicts are 121 rows.
**Corpus rows flipped against 6afdc61: none.** Row regressions: 0; consumer
regressions: 0. `r4h-m1-1-summary.json` asserts these invariants; the full raw
differential and mismatch inventory retain every source, reason/advice mismatch
and outside bucket rather than dropping unimplemented behavior.

| Go-deny/Rust-permit corpus scope | SupportedPreflight N | LimitedPreflight U-C | Total |
| --- | ---: | ---: | ---: |
| In-slice Rust defects | 168 | 123 | 291 |
| In-slice decided changes | 6 | 0 | 6 |
| All in-slice | 174 | 123 | 297 |
| Outside | 0 | 285 | 285 |
| Total | 174 | 408 | 582 |

These counts are unchanged from M1. The six decided permits are search[84]
(D16) and appdata[64] (D25), three consumers each. clients[41] and quoted curl
file-URL globs stay outside under D26 3, Go D / Rust U-C, and must be closed
before any consumer cutover. Their acceptance as outside does not prove safety.

## Reviewer variants and moved observations

All 212 inputs from the Opus variants and six DeepSeek harnesses are retained,
including controls: 636 consumer observations. An additional 28 D26 inputs
(21 source regressions, four revised review rows, three structured shapes) add
84 observations, for 720 total. Reviewer permission mismatches fall 102→48;
the combined set falls 174→54. Exactly 120 observations (40 inputs × three
consumers) change class, all toward Go; no new permission mismatch is introduced.
The corpus and variant denominators are separate.

Every input/output, Go stdout/stderr/exit and Rust class/coverage/wire is retained
in `r4h-m1-1-variants-{baseline,final,annotated}.jsonl`. The Go binary is freshly
built from unchanged `cmd/agent-guard`. Matching synthetic HOME/cwd/USER is used.
For structured Pi inputs, Rust receives read/path while Go --checker receives
its canonical Read/file_path projection, as native/core/protocol.go specifies;
both exact events are stored. These are offline adapter checks, not Pi loading.

The table lists every moved input ID; each row represents Claude/Codex/Pi with
identical before/after/Go classes. Exact expanded source and all 120 observations
are in `r4h-m1-1-variant-flips.jsonl`.

| Input ID (all three consumers) | M1 → M1.1 | Go |
| --- | --- | --- |
| `Opus/variants.json:brace-mixed-seq` | N → D | D |
| `Opus/variants.json:brace-plus-seq` | D → N | N |
| `Opus/variants2.json:pg-git-log-pathspec` | N → D | D |
| `Opus/variants2.json:pg-git-show-pathspec` | N → D | D |
| `Opus/variants2.json:pg-git-diff` | N → D | D |
| `Opus/variants3.json:sv-redirect-out-var` | D → N | N |
| `Opus/variants3.json:pg-git-show-colon` | N → D | D |
| `DeepSeek/fs-harness.py:file-url-appdata` | N → D | D |
| `DeepSeek/glob-harness.py:brace-deep` | N → D | D |
| `DeepSeek/glob-harness.py:brace-env-nested` | N → D | D |
| `DeepSeek/paren-harness.py:quoted-extglob-plus` | D → N | N |
| `DeepSeek/paren-harness.py:quoted-extglob-star` | D → N | N |
| `DeepSeek/paren-harness.py:quoted-paren-list` | D → N | N |
| `DeepSeek/paren-harness.py:quoted-paren-single` | D → N | N |
| `DeepSeek/paren-harness.py:quoted-paren-env` | D → N | N |
| `DeepSeek/paren-harness.py:quoted-paren-env2` | D → N | N |
| `DeepSeek/paren-harness.py:dquote-paren` | D → N | N |
| `DeepSeek/redirect-secret.py:echo-out-secret` | D → N | N |
| `D26/git:0` | N → D | D |
| `D26/git:1` | N → D | D |
| `D26/git:2` | N → D | D |
| `D26/git:3` | N → D | D |
| `D26/git:4` | N → D | D |
| `D26/brace:7` | N → D | D |
| `D26/brace:8` | D → N | N |
| `D26/brace:9` | N → D | D |
| `D26/brace:10` | N → D | D |
| `D26/file_url:12` | N → D | D |
| `D26/paren:14` | D → N | N |
| `D26/paren:15` | D → N | N |
| `D26/paren:16` | D → N | N |
| `D26/redirect:18` | D → N | N |
| `D26/redirect:19` | D → N | N |
| `D26/review:group-env` | D → N | N |
| `D26/review:group-filter-env` | D → N | N |
| `D26/review:group-npmrc` | D → N | N |
| `D26/review:group-ssh` | D → N | N |
| `D26/structured-file-url:file://` | N → D | D |
| `D26/structured-file-url:FiLe://` | N → D | D |
| `D26/structured-glob:literal-bracket` | D → N | N |

The remaining 54 permission differences are all explicitly classified:

| Classification | Observations | Current vs Go | Owner/decision |
| --- | ---: | --- | --- |
| Quoted brace-list member | 6 | D / N | D26 2b baseline false-permit candidate |
| Split-union reading | 3 | D / N | D1/D22 |
| Unquoted extglob reach | 12 | D / N | D1/D26 |
| `cat < ~` | 3 | N / D | D25 decided permit |
| Quoted curl file-URL family | 6 | U-C / D | D26 3, outside, before cutover |
| PWD assignment vs tracked cwd | 6 | N / D | P2/M2 |
| `cat --file=~/.ssh/id_rsa` | 3 | N / D | P5 operandValue/M4 |
| Bash heredoc/herestring code | 6 | U-C / D | P3 stdin shell/M3 |
| Secret variables in stdin data | 9 | N / D | P6 secrets/M4 |

Variant Go-deny/Rust-permit falls 90→33 (24 deferred-owner, 6 outside and 3
decided D25 observations). All D26 regression expectations pass. Go N / Rust
U-C controls retain limited coverage, rather than being claimed as Go class
equivalence. Earlier malformed Pi/Go structured projections are preserved as
`*-invalid-pi-projection.jsonl` and `*-invalid-go-pi-projection.jsonl`; their
results are superseded and excluded from the accepted counts. The first missing
VERSION baseline archive compilation is likewise retained as a build-error log,
not counted as a regression or ablation failure. Valid baseline runs follow it.

## Runtime baseline failures and causal ablations

The final regression source/fixture is copied into an external 6afdc61 archive.
All seven named integration tests and both private record-field tests compile
and fail at runtime there, then pass on the repaired source. The parent probe
oracle gets only a compatibility constructor that retains its literal-only
behavior; no repaired oracle is copied into the parent. No compile failure is
counted. `r4h-m1-1-baseline-final-{integration,fields}.log` and the tests JSON
record the exact failing names and reasons.

Each ablation runs in `r4h-m1-1-ablation-src/`, breaks one mechanism, runs its
named test plus the complete corpus and variant set, restores exact bytes by
SHA-256, and reruns the same test/reports. Every runtime failure is Cargo 101,
every restored test is 0, and every restored report recovers the normal classes.

| Mechanism | Named test | Corpus rows affected | Variant observations changed | Broken/restored |
| --- | --- | --- | ---: | --- |
| brace-list | `brace_union_and_nested_lists` | appdata[82], credentials[189] | 48 | 101 / 0; exact bytes |
| brace-mixed | `brace_union_and_nested_lists` | None | 6 | 101 / 0; exact bytes |
| brace-nested | `brace_union_and_nested_lists` | None | 12 | 101 / 0; exact bytes |
| brace-signed | `brace_union_and_nested_lists` | None | 6 | 101 / 0; exact bytes |
| file-url-absolute | `file_urls_reach_protected_identity` | None | 6 | 101 / 0; exact bytes |
| file-url-maker | `file_urls_reach_protected_identity` | None | 6 | 101 / 0; exact bytes |
| git | `git_owns_quoted_pathspec_globs` | None | 27 | 101 / 0; exact bytes |
| ls-effect | `ls_records_list_effect` | None | 0 | 101 / 0; exact bytes |
| paren | `quoted_parentheses_stay_literal` | None | 42 | 101 / 0; exact bytes |
| probe-oracle | `glob_probe_oracle_uses_word_provenance` | None | 0 | 101 / 0; exact bytes |
| redirect-vars | `redirect_variables_follow_direction` | None | 12 | 101 / 0; exact bytes |
| search-field | `search_flag_has_an_explicit_owner` | None | 0 | 101 / 0; exact bytes |
| structured-glob | `structured_targets_do_not_infer_shell_globs` | None | 3 | 101 / 0; exact bytes |

Search/List are record-fidelity assertions without a current verdict change;
the probe-oracle mutation tests a guard assertion, not a production verdict.
Their zero variant deltas do not invalidate those explicit contracts. Other
mechanisms have affected variant observations; list recognition also changes
appdata[82] and credentials[189] in the original corpus when broken. D26 2g is
an evidence/provenance correction, not a verdict mechanism, so its verification
is parsed label-only equivalence and corpus byte equality rather than a verdict
ablation. The scripts and thirteen `*-binding.json` files retain all exact
mutations, source hashes, compiled failures, report deltas and restored passes.

## Gates, dev evidence and open work

| Check | Final result | Artifact |
| --- | --- | --- |
| fmt | Pass | `r4h-m1-1-rust-check.log` |
| warnings-denied Clippy, all targets | Pass | `r4h-m1-1-clippy.log` and rust-check log |
| Locked release, all targets | Pass | `r4h-m1-1-final-release.log` |
| cargo-deny with fetch enabled | Advisories/bans/licenses/sources pass; five existing duplicate warnings | `r4h-m1-1-final-deny.log` |
| All tests, no-fail-fast | 63 pass; only complete differential fails (121 rows / 347 observations) | `r4h-m1-1-final-tests.log` |
| make rust-check | make 2 / Cargo 101, only `every_legacy_row_is_accounted_for` | `r4h-m1-1-rust-check.log` |
| Parse-only comparator | 1,550 rows, 1,397 comparisons, 243 leads, zero semantic observations | `r4h-m1-1-parse-comparator{.jsonl,-final.log}` |
| Packaged binary | version 0.6.0, exit 0; no args/checker exit 1, no check performed | `r4h-m1-1-binary-smoke.json` |
| SSH D23 tests/probes | Five pass; six protected/private spellings have empty readlink/stat call lists | `r4h-m1-1-ssh-probes.log` |

Brush remains the sole semantic acceptance arm. Tree-sitter remains only a
parse-comparator dependency; moving it awaits the comparator-removal decision.
The five historical Tree-only conflict rows match on Brush and no Brush row
leaves the denominator. Parse spans remain review leads, not semantic evidence.

The dev manifest is unchanged: 239 counted cases + nine metamorphic variants +
nine lifecycle-only + three writer rows = 260. The fresh 248-row evaluator report
has the unchanged counted vector N70/A3/D96/U-C28/U-R21/U-O6/F15, and required
zero starts remain 138/138 (144/144 with variants). Lifecycle/writer tests are
separate from protection denominators. Six U-C runtime-configuration witnesses
still permit synthetic protected reads: Gate B is open. The 87 revised D22
cases, JSONL interface, F-kind blocking, lifecycle and writer checks pass.

No real protected-data read/probe, installation, hook registration, TCC or
permission change, remote write, release, paid API call or cutover occurred.
The four untracked Rust plan files are preserved. Deslop cleanup consolidated
the Git add owner and the quote/depth brace scan; the distinct runtime, field,
oracle and provenance tests are retained. Candidates left are the explicitly
deferred owners above, not additional fixes made in this round.

The 121 corpus conflicts still require P2/P7 statement/cwd/identity work (M2),
P3/P4 wrapper/stdin/stream and P6 credentials (M3), and P5 roles plus P6
advice/secrets (M4), including cwd[26] directory collapse. App Data fragment
touches/scan gates remain P6/M4. The current protection evidence is offline;
complete Go conformance, both M1 fix verifications, installed protocol/loading,
Gate A, Gate B, execution owners, arbitrary-writer preservation, production
performance and consumer cutover remain unaccepted. M1.1 stops here.
