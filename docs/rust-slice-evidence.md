# Offline Rust R4h M1.2 evidence

M1.2 closes D27 2a–2d at the quote, credential-effect and test-oracle owners,
records D27 3 literal-brace divergences, and lists D27 4 deferrals. The baseline
is `5c52c9afe9074cbd608dab6b6303ecee33722443`; decisions.md v3.0 D27/D28 governs
this round. D28 is read for sequencing and owner rulings only: **M2 Phase B has
not started**. M1 remains unaccepted until both coordinator-owned Opus 5.5 and
DeepSeek verifications pass (D27 1).

The 1,289-row corpus has zero outcome/category flips and zero new row or
consumer conflicts. This is a corpus claim, not arbitrary shell/program
equivalence. The M1.1 claim that all flips moved toward Go was valid only for
its 720 observations; it missed the subsequently reviewed N1 ANSI-C brace loss
and N4 recursive-listing over-deny. Both are repaired here and their inputs are
in the current variant set.

Go remains authoritative. Production Go, hook paths, dependencies, Cargo.lock,
the packaged binary entry and public setup are unchanged. The packaged
`agent-guard-rust-slice` remains version-only. No holdout, real credential store,
private key or declared environment file was read. Homes, probes, builds and
scratch source copies are synthetic or external to the checkout.

## Source and measurement binding

All current artifacts named below live under `~/.cache/guard-fixtures/fixture-seat/`
with the `r4h-m1-2-` prefix. `initial-state.json` verifies the prior source and
four preserved untracked-plan hashes against M1.1's delivery binding. The
measurement and final-delivery bindings record final source, fixtures, raw
artifacts, binaries, commit ancestry, status and evidence hash. No threshold,
configuration bypass or concurrency was added.

The baseline differential is bound to M1.1's final delivery: SHA-256
`b706ac065114056db49fca675e2af1516c1bc6952b9f36c2c9fcebf838f1ff4d`.

| Input | SHA-256 | M1.2 change |
| --- | --- | --- |
| Original Go corpus | `1e223c6453d6883acc88af9967beab4251ba0fc6d636a1186482b6e4b524c695` | None |
| Filesystem setup | `b1d51062925ccbfdfae5c8ffbc3e130e25391f31b05207448ee02be8cc874b3e` | None |
| D22 scope | `a3e41fd04bf1daf575c7312f9a275d298203ac7493228f6517dbcbb1fccd18ac` | None |
| Dev manifest | `385e395dbee8855c2985a7b6d2f92083b898e1614573ad4f7b9ac71b8597be10` | None |
| Classification overlay | `87171281c243ebceefed14d6fbb103c5f75187239dab3c449652d3d156b63ed5` | None |
| D22 regressions | `44d12aea41a947e0c1996071b70bb9777293bfa6b58f0b3d4bbd63060debc632` | None |
| M1.1 regressions | `3a30251e6159235f512dc07f3bcc947ec5bf13571b57a963a7752609ec47250f` | None |
| M1.2 regressions | `b1f591d91cec5d1ddd222817af7b98530aae896de0772969a227ac2c4a3375e9` | 21 new rows, including D27 shell/redirect/structured literals |

## Owner repairs and scanner audit

`src/shell/quotes.rs` supplies one raw quoted-span mechanism. Ordinary single
quotes keep backslashes literal; ANSI-C single quotes honor escapes; double
quotes and dollar-double quotes skip escaped delimiters. `brace_text`,
`brace_group` and D1 parenthesis pairing use that mechanism. D1 skips an entire
ANSI-C literal, while active parameter syntax in double/gettext quotes retains
its prior treatment. The detector remains independent of a parser arm.

Heredoc delimiter quote removal also uses the raw scanner and the existing
ANSI-C decoder, without parameter substitution. This fixes D1's false
divergence classification for dollar-quoted heredoc bodies. Brush 0.4.0 still
rejects these delimiter forms, so they remain U-R: the regression asserts the
coverage distinction and an active-tail control, not a permit. Full heredoc
grammar and stdin execution ownership remain P3 work.

The credential-directory walk branch now requires `Effect::Read`, mirroring
`native/rules/credentials.go:25`; List is not added to an exclusion list. The
suffix list remains the D25 shape patch, to be deleted by P6. App Data walking
and credential-content reads retain denial controls.

RecordingProbe's default `new(fixture, word)` requires Word provenance and
derives `patterned` from `word.globs`. The explicit `literal(fixture)` mode is
used by event-level harnesses that do not carry one word. The word-to-target
probe regression uses the default constructor and directly rejects an
attempted protected glob probe; a quoted-word control permits the literal
probe. The duplicate new oracle test was merged into the existing `m1_1`
test. This does not claim universal per-target provenance tracing for mixed
event-level harnesses and introduces no production testing seam.

The audit covers every hand-written quote/brace/paren scanner under `src/`,
including decoded-pattern and non-shell code boundaries. `$'…'` and `$"…"`
are shell modes only at raw-shell owners.

| Scanner / producer | Dollar-quote result | Disposition |
| --- | --- | --- |
| `quotes::skip` | ANSI-C escaped quotes differ from ordinary single quotes; dollar-double quotes use double-quote escapes | New shared raw scanner; ablated |
| `words::brace_text` | Skips complete ANSI-C/gettext spans before outside brace reach | Fixed; prefix-list regression and ablation |
| `words::brace_group` | Escaped ANSI-C quotes no longer hide the member comma/closing brace | Fixed; member-list regression and ablation |
| `words::brace_sequence` | Receives an unquoted group body; parses endpoints, not quote state | Unchanged; D1/D26 sequence union remains |
| `words::fill` glob detection | Brush ANSI-C pieces do not set Glob; double/gettext pieces recurse with `quoted=true` | Already provenance-aware; unchanged |
| `words::ansi` | Receives quote contents from Brush or the raw delimiter owner; decodes existing escapes | Reused decoder; no second implementation |
| `divergence::closing` paren pairing | Uses shared ANSI-C/gettext span skipping | Fixed; executable-qualifier regression and ablation |
| `divergence::detect` literal skip | Complete ANSI-C literal is inert; double/gettext parameter syntax stays active | Fixed; inert-literal regression and ablation; not a full shell interpreter |
| `divergence::detect` heredoc / `quotes::heredoc_delimiter` | Recognizes dollar quoting and removes delimiter quotes without expanding variables | Fixed classification; ablated; P3 parser limitation retained |
| `divergence::detect` parameter-flag boundary | After `${(` is seen, rejection does not depend on full nested-brace grammar | Unchanged conservative D1 boundary; full parameter grammar not claimed |
| `divergence::qualifier` body/position gate | Body contents stay active even when code is quoted, per D1 | Unchanged; pairing fixed at its owner |
| `glob::brace_members` | Scans decoded path/pattern text; shell quote syntax has already been removed | No raw quote state; unchanged |
| `glob::alternatives` paren gate | Requires `patterned`; ANSI-C/gettext literals therefore do not acquire group reach | No raw quote state; unchanged |
| `glob::tokens` / `class_matches` | Decoded glob escapes/brackets, not shell quotation | Unchanged pattern grammar |
| `targets::code_paths` | Foreign-language token boundary receives already shell-decoded inline code | No shell ANSI/gettext mode belongs here; unchanged P3 boundary |
| `shell::check_nesting` | Deliberately counts raw delimiters for a resource bound, including quoted data | No semantic quote state; unchanged bound |

`shell-readings.json` contains 20 printf-only Bash/Zsh readings for escaped
ANSI-C prefixes/members, gettext, ordinary single quoting, literal braces and
the signed sequence. `heredoc-final-audit.json` uses built-in read/printf only.
An escaped ANSI-C heredoc delimiter is accepted by both host shells but the
Go comparator refuses it as syntax; **that row is stopped as a Go baseline
limitation candidate**, not used to direct a Go-syntax port or claim equivalence.
Rust also refuses it through the pinned parser. Neither checked cat command
was executed on the host.

## P0 record limits (E1 corrected)

| Field | Current producer / limit | Later owner |
| --- | --- | --- |
| Word.role | Always Arg | P3/P5 |
| Word.value | Text clone; operandValue projection absent | P5 |
| Word.pwd | Filled, not consumed as a separate state distinction | P2/P5 |
| Command.cwd | Tracked cwd shares PWD namespace | P2/M2, D28 |
| Command.program / wrappers | Some(0) / empty; effective-program resolution partial | P3/P5 |
| Command.shell | Always true on observed commands | P3 |
| Command.flags | Empty; adapters infer flags from argv | P5 |
| Command.items | None; producer heuristics remain | P4 |
| Command.stdin | None/Data only; Shell/Code absent | P3 |
| Script.uninspectable | Empty | P2/P3 |
| Redirect.target expanding heredocs | Known variables substituted; Go retains raw body | P3 |
| Target.unresolved | Path clone rather than unresolved provenance | P5/P7 |
| Target.command | Some(command_index) on program targets at policy.rs:392; redirects/structured targets retain None | P5 complete maker association |
| Target.sends | Always false | P5 |
| Target.effect | Program-table heuristic beyond repaired List behavior | P5/P6 |
| Target.walk / via | Partial adapter heuristics | P4/P5 |
| Target.search | Implicit-root help heuristic omits Go --version/-V/combined flags; field still unread | P5/P7, D27 4 |
| Target.glob | Word transport plus explicit Git/search overrides; other owners pending | P5/P7 |

## Complete corpus differential

1,265 operations × three consumers = 3,795 observations, plus 24 metadata
checks; 963 operations are in slice and 302 outside. All prior matching RETAIN
contracts and 21 CHANGE rows remain matched (D28 7). Eight Claude-only advice
conflicts explain the non-threefold defect count.

| Family | RETAIN match | CHANGE match | Conflict | Outside | Metadata | Total |
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

Categories: 2,479 matching RETAIN, 63 matching CHANGE, 347 Rust_defect and 906
outside observations. Remaining conflicts: 121 rows. **Corpus rows flipped:
none.** New row conflicts: 0; new consumer conflicts: 0. The raw differential,
mismatch inventory and summary retain all unsupported/outside buckets.

| Corpus Go-deny/Rust-permit | N | U-C | Total |
| --- | ---: | ---: | ---: |
| In-slice Rust defects | 168 | 123 | 291 |
| In-slice decided changes (D16/D25) | 6 | 0 | 6 |
| All in slice | 174 | 123 | 297 |
| Outside | 0 | 285 | 285 |
| Total | 174 | 408 | 582 |

## Reviewer variants, flips and sanctioned divergences

455 input records × three consumers = 1,365 observations. The 400 reviewer
inputs (1,200 observations) retain every Opus v11 old/new/cred/extra input and
all six original DeepSeek harnesses plus m11, gitgrep and nested inputs.
Overlapping forms remain separate by provenance, not independent examples.
21 current fixture rows, six scanner controls and 28 M1.1 controls add 165
observations. These denominators are separate from the corpus.

Inputs were read only from the reviewer directories and copied as data into
this cache; no reviewer runner was executed or modified. The Go comparator is
freshly built from unchanged `cmd/agent-guard`, offline with external caches.
Pi gets lower-case names and path for Rust read/write/edit; Go receives the
canonical file_path projection (native/core/protocol.go:52). Both exact events,
Go stdout/stderr/exit and Rust wire/coverage/effects are preserved.

Reviewer permission mismatches: 147→117; combined: 210→147. Class flips: 69; full semantic-projection flips: 75. No new Go-deny/Rust-permit is introduced. The only new Go permission mismatch is the executable-qualifier regression (three consumers), now U-R under D1 rather than baseline N.

| Flipped input ID (each row is three consumers) | Baseline → M1.2 | Go |
| --- | --- | --- |
| `Opus/v11/new.json:b-ansi-quote-desync` | N → D | D |
| `Opus/v11/new.json:l-ls-R-docker` | D → N | N |
| `Opus/v11/new.json:l-ls-R-config` | D → N | N |
| `Opus/v11/extra.json:a-desync-dir` | N → D | D |
| `Opus/v11/extra.json:a-desync-seq` | N → D | D |
| `Opus/v11/extra.json:a-desync-home` | N → D | D |
| `Opus/v11/extra.json:a-desync-appdata` | N → D | D |
| `Opus/v11/extra.json:a-desync-npmrc` | N → D | D |
| `Opus/v11/extra.json:l-ls-R-kube` | D → N | N |
| `Opus/v11/extra.json:l-ls-R-cargo` | D → N | N |
| `Regression/brace_text:ansi-prefix-list` | N → D | D |
| `Regression/brace_text:ansi-quote-directory` | N → D | D |
| `Regression/brace_text:ansi-prefix-sequence` | N → D | D |
| `Regression/brace_text:ansi-home-list` | N → D | D |
| `Regression/brace_text:ansi-appdata-list` | N → D | D |
| `Regression/brace_text:ansi-npmrc-list` | N → D | D |
| `Regression/brace_group:ansi-member-list` | N → D | D |
| `Regression/divergence_literal:ansi-inert-divergence` | UR → N | N |
| `Regression/divergence_closing:ansi-active-qualifier` | N → UR | N |
| `Regression/credential_effect:list-docker` | D → N | N |
| `Regression/credential_effect:list-kube` | D → N | N |
| `Regression/credential_effect:list-cargo` | D → N | N |
| `Regression/credential_effect:list-config` | D → N | N |

Six more semantic observations change cause/coverage, not class, on the two
dollar-quoted heredoc rows. Their class stays U-R through the existing Brush
parser refusal. All current fixture/control expectations pass.

| Sanctioned Go-deny/Rust-permit, counted separately | N | Decision |
| --- | ---: | --- |
| cat < ~ | 6 | D25 4 |
| Signed {-1..-1} sequence | 9 | D26 2b |
| Literal quoted shell/redirect and structured Read braces | 45 | D27 3 |

| Variant Go-deny/Rust-permit scope | N | U-C | Total |
| --- | ---: | ---: | ---: |
| In-slice deferred owners | 30 | 6 | 36 |
| In-slice sanctioned D25/D26/D27 | 60 | 0 | 60 |
| Existing assigned-PWD false denial (D28 4) | 3 | 0 | 3 |
| All in slice | 93 | 6 | 99 |
| Outside curl URL-glob (D26 3) | 0 | 6 | 6 |
| Total | 93 | 12 | 105 |

D27's 45 observations contain 33 shell/redirect and 12 structured observations.
The fixture marks both halves with decision_id D27. D26's signed row is marked
D26. These are baseline false denials under the rulings, not protection losses.
The sanctioned counts overlap neither the deferred-owner nor outside counts.
D28 4 also rules `PWD=/tmp; ls $PWD/Containers` Go D/Rust N a baseline
false denial (three observations). It is listed separately: no P2 producer or
D28 regression fixture was added before Phase B authorization. The other
PWD row, `PWD=/tmp; ls Containers`, remains a Rust P2 loss (three observations).

| Remaining permission difference | Observations | Rust / Go | Owner / ruling |
| --- | ---: | --- | --- |
| Quoted brace-list member | 9 | D / N | D26 2b union / Go baseline false permit |
| Split readings | 3 | D / N | D1/D22 union |
| Unquoted extglob | 21 | D / N | D1/D26 union |
| Executable qualifier with ANSI-C body | 3 | U-R / N | D1 required refusal; newly detected |
| Dollar-quoted heredoc parser refusal | 6 | U-R / N | P3/M3, pre-existing |
| Literal braces | 45 | N / D | D27 3 sanctioned |
| Signed sequence | 9 | N / D | D26 2b sanctioned |
| cat < ~ | 6 | N / D | D25 4 sanctioned |
| Assigned PWD variable | 3 | N / D | D28 4 Go baseline false denial; producer work pending |
| PWD vs tracked cwd | 3 | N / D | P2/M2 Rust loss; D28 read, not implemented |
| Option-equals operandValue | 3 | N / D | P5/M4 |
| Git grep -f pattern files | 12 | N / D | D27 4, P5/M4; add regression in M4 |
| Bash heredoc/herestring code | 6 | U-C / D | P3/M3 |
| Secret vars in stdin data | 12 | N / D | P6/M4 |
| Curl file-URL globs | 6 | U-C / D | D26 3 outside; before cutover |
| Total | 147 | | |

Permission-equivalent Go N/Rust U-C and Go D/Rust U-R observations retain their
coverage distinctions. The Glob tool remains outside observed coverage; no
adapter was added. Structured Grep glob patterns and unquoted shell brace
reach retain their controls. Full Glob-tool ownership remains open before
cutover.

## Baseline failures and exact-byte ablations

Final regressions compile against an external 5c52c9a archive. Six m1_2 tests
fail at runtime, and the revised default-oracle m1_1 test fails separately.
The baseline constructor gets only call-signature compatibility and retains
its literal-only oracle; no repaired test support is copied into it.
baseline-final-{owners,oracle}.log and baseline-final-tests.json retain the
failing assertions. All seven repaired owner/oracle tests pass on final source;
the literal-brace fixture already passed at baseline and records sanctioned
behavior rather than a newly fixed mechanism.

Each final ablation changes one mechanism in ablation-src-v2, compiles it,
runs the named assertion plus the full corpus and variant set, restores the
exact original bytes by SHA-256, then reruns the assertion and reports. Final
restored reports match the normal checked projections. No compile failure is
counted as an ablation failure.

| Mechanism | Test | Corpus changed | Variant class / semantic changes | Broken / restored |
| --- | --- | --- | ---: | --- |
| brace-group | `ansi_member_preserves_brace_reach` | None | 3 / 3 | 101 / 0; exact bytes |
| brace-text | `ansi_prefix_preserves_brace_reach` | None | 36 / 36 | 101 / 0; exact bytes |
| credential-effect | `credential_directory_walk_requires_read` | None | 24 / 24 | 101 / 0; exact bytes |
| d1-closing | `ansi_qualifier_keeps_parenthesis_pairing` | None | 3 / 3 | 101 / 0; exact bytes |
| d1-heredoc | `quoted_heredoc_body_is_inert_to_divergence` | None | 0 / 9 | 101 / 0; exact bytes |
| d1-literal | `ansi_literal_is_inert_to_divergence` | None | 3 / 3 | 101 / 0; exact bytes |
| probe-default | `glob_probe_oracle_uses_word_provenance` | None | 0 / 0 | 101 / 0; exact bytes |
| quote-ansi | `ansi_prefix_preserves_brace_reach` | None | 45 / 45 | 101 / 0; exact bytes |

The heredoc mutation changes coverage assertions without changing class,
because the pinned parser independently refuses the forms. The oracle mutation
tests the required probe-order guard, not a production verdict. Their zero
class deltas do not replace the runtime assertion. D27 2d evidence corrections
have no production verdict mechanism; their checks are source/accounting and
offline cargo-deny evidence rather than a simulated verdict ablation.

Deslop consolidated raw quote scanning, kept D1 independent of a parser arm,
and merged the duplicate oracle test. cleanup-equivalence.json proves identical
1,365 variant semantic projections and 1,289 corpus projections before/after
that cleanup. precleanup-* artifacts retain the earlier successful runs but
are superseded by final source-bound runs. invalid-marker-* preserves a
generator's incorrect partial $H replacement; corrected inputs keep $HOME
intact. The missing-target differential error and initial control/Clippy
errors are diagnostic history, excluded from accepted passes.

## Mechanical gates and remaining acceptance

| Gate | Final result | Artifact |
| --- | --- | --- |
| cargo fmt -- --check | Pass | fmt.log |
| cargo clippy --offline --locked --all-targets -- -D warnings | Pass | clippy.log |
| cargo deny --offline --locked check | Advisories/bans/licenses/sources pass; five existing duplicate warnings; no fetch | deny.log |
| Locked offline release, all targets | Pass | release.log |
| All-target tests, no-fail-fast | 70 pass; only every_legacy_row_is_accounted_for fails (121 rows / 347 observations) | tests.log |
| make rust-check (CARGO_NET_OFFLINE=true) | make 2 / Cargo 101; same sole strict failure | rust-check.log |
| Packaged binary | --version 0.6.0 exit 0; no args/checker exit 1; no check performed | binary-smoke.json |

cargo-deny's prior fetch-enabled M1.1 row is superseded by the explicit offline
check here (E4); no claim about that old run's fetch provenance is carried into
this result. E1 corrects Target.command, E2 scopes the old 720-observation claim,
and E3 records D25/D26/D27 separately.

The complete strict differential remains open at 121 conflicts. D27 4's Git
grep -f read owner and Target.search help-flag producer remain P5/P7 work.
The D25 suffix shape patch, App Data touches/scan gates, wrapper/stdin/stream
owners, operandValue, content/secret rules and directory-collapse cases remain
pending. D28's tracked-cwd/PWD/CDPATH and identity work awaits M2 Phase B; the
Phase A plan remains external and unchanged. The stopped escaped-heredoc Go
syntax limitation is not adjudicated here.

The unchanged dev/lifecycle/writer/JSONL/SSH tests pass within the all-target
suite; their M1.1 runtime reports are historical, not fresh M1.2 measurements.
Installed loading, real consumer Gate A, Gate B, execution owners, production
performance and cutover remain unaccepted. No installation, permission change,
remote write, release, paid call or real protected-data operation occurred.

Changed: quote owners, Read gate, explicit oracle construction, regression
fixtures and this source-bound evidence. Retained: distinct prefix/member,
literal/qualifier/heredoc coverage, effect and probe-order assertions, plus
literal-brace decision rows. Candidates left: the named deferred owners and
coordinator-owned verifications above. Validation: final gates, regressions,
differential and eight restored ablations as recorded. M1.2 stops here.
