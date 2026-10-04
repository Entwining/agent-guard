# Offline Rust migration evidence

## M1.5 substitution boundaries and adapter-owned divergence

The baseline is `aaf179abaefee26f243736bbb524de71ab4ec2e4`. Binding rulings
are `r5/m1-5-rulings.md` (24–29) and `r5/m1-5-rulings-b.md` (30).
Raw evidence lives under `~/.cache/guard-fixtures/fixture-seat/r4h-m1-5-final/`;
`sol-m1-5-report-final.md` contains every changed observation, receipt and
owner/scope backlog row. Go remains authoritative, the packaged Rust entry
remains version-only, and M2 Phase B has not started. Coordinator checks and
fresh Opus plus DeepSeek acceptance are pending.

Brush 0.4.0 closes a word substitution at a comment's `)` or `}` and previously
forwarded only that truncated body. The lexer now owns command/backquote body
bounds in original bytes. Following Brush pieces within that lexical body
cannot become literal word text. Command grammar inside parameter or arithmetic
contexts preserves physical nesting depths while enabling its own comments and
heredocs. The boundary sweep also replaces arithmetic-for serialization and
heredoc AST text with original lexical regions; mismatched boundaries produce
explicit unsupported coverage. A quoted empty heredoc delimiter remains a real
delimiter, with its own red regression and ablation.

Array-assignment closers followed by a word byte have a lexical divergence
span. This preserves the Bash literal-`#` reading while refusing the incompatible
host reading. `setopt`, `unsetopt` and `emulate` now add the dialect gap at the
program adapter, covering assignments, negation, timing and redirection prefixes.
The raw command-shape detector is retired. Parser/lexer `Unterminated`
disagreements become U-R; genuine span, resource, checker and probe faults remain F.

| Check | Final M1.5 observation |
| --- | --- |
| Original manifest | Same 7,459 inputs / 22,377 observations and 49 matching provenance hashes; exact original materialized request bytes |
| Manifest changes | 147 semantic changes: 84 class changes and 63 same-class changes |
| Review replay | req4–req14 plus 74 three-consumer regression rows: 346 observations, 142 semantic changes |
| Protection | No new wire permit except 36 ruled literal/non-command false refusals; required blocking rows pass |
| Recovery | No new F, including no U-R-to-F |
| Full corpus | 1,289 records / 3,795 observations; zero semantic changes after normalizing only synthetic run PID paths |
| Legacy conflicts | Exactly the same 121 parents / 347 observations; frozen owner/scope backlog retained |
| Temporary raw-detector oracle | 36 accepted raw-only permits in the manifest, zero in the review replay, zero raw hits in the corpus |
| Brush oracle | 199,301 compared bytes, zero unknown disagreements, 20 counted substitution-end limits, 856 token-length mismatches, 575 span mappings |
| Oracle exclusions | 267 tokenizer refusals and one frozen F3 limit; no word/nested refusals |
| Mechanism evidence | 13 compile-success / named failure / exact-byte restore / passing rerun receipts |
| Full test suite | 117 pass; only `every_legacy_row_is_accounted_for` fails |
| Mechanical checks | fmt, Clippy, release build and cargo-deny pass offline; existing duplicate dependency warnings remain |
| make rust-check | make 2 / Cargo 101 at the unchanged strict differential; not fully green |

The final oracle key uses measured token-length mismatch, including the eight
actual Opus inputs and the tab variant. It records Brush substitution-end limits
rather than hiding them with replacement body text. Applying this observer to
aaf179a also counts those limits: a zero quote-context disagreement count does
not prove that production forwarded complete code. The earlier 16,036-to-zero
claim included narrowing that hid the body loss; it was not entirely owner fixes.

V2 has the actual `${v:-${w@Z}}` witness: its outer region's support, cause and
span detect removal of the closer check. V3 has a pinned-grammar capture argument,
not a natural fragment-Err runtime witness; malformed captures, PEG backtracking,
other parser options and future versions remain unproved. The defensive refusal
branch stays, without a fabricated entry point. Region fixtures assert each own
span and cause and distinguish inactive single-quoted `${` text. Count-only
manifest observers remain separate from class pins.

| Cutover blocker recorded without Go repair | Owner |
| --- | --- |
| Go permits `a=(x)#$(cat .env)z` and the backquote form although Bash executes the substitution | Go shell framing / target inference before cutover |
| Go permits F1 quoted/commented heredoc qualifier and option-changing builtin rows that Zsh executes | Go shell framing / qualifier owner before cutover |
| Go permits scalar-prefixed, negated and redirected option-changing builtins | Go program adapter before cutover |
| `builtin setopt`, `command setopt`, `noglob setopt` remain UnknownProgram U-C in Rust and permit in Go | P2/P5 precommand-wrapper owner |

The original 121-row backlog is unchanged; these cutover blockers are a separately
bound supplement. Existing arithmetic binding, inherited environment and CDPATH
limits remain outside P1. No Go source, dependency, hook, installation, machine
permission or public protocol entry changed, and no submitted protected command
was executed. All new probes and host witnesses use synthetic data.

## Historical M1.4 lexical framing, parameter coverage and visible nested effects

The baseline is `61e29334db08835f100a8e5bfc398b0c96834691`. The binding scope
is `r5/m1-4-rulings-final.md`, SHA-256
`290d67ef85aff693fdc5fad085a862602baf62d8ebb83608b052ba9e4286706e`.
Go remains authoritative. The packaged Rust binary remains version-only.
M2 Phase B has not started; coordinator checks and fresh Opus 5.5 plus
DeepSeek acceptance remain required.

All raw evidence is external under
`~/.cache/guard-fixtures/fixture-seat/r4h-m1-4-phase2/`. The final report contains the
complete changed-observation table, the 121-row owner/scope backlog and every
mechanism receipt. `changed-observations.jsonl` preserves complete before,
after and Go records; `changed-observations.md` shows all changed fields.
Only evaluator timing is excluded. These are historical packet measurements,
not acceptance of M1. The raw-shape token remapping also hid a pre-existing
substitution body loss, corrected in M1.5 above.
`corpus-changed-observations.json` records
every full-corpus delta, and `oracle-before-after.json` retains every original
oracle discrepancy and final disposition.

The observers ran before product edits: nine wire/context regressions failed
at runtime with unchanged production source, apart from test registration.
An external archive confirms the frozen failures at 61e2933. The corrected
escaped-dollar assertion also fails against that archive. The original R2
fixture's D expectation was wrong: an escaped dollar followed by brace
expansion yields `$.env` and `$x`, so that call remains N. Its separate reach
assertion, not the mistaken denial label, detects the old owner defect.

`rust-m1-4-input-manifest.json` freezes 7,459 input occurrences: 7,258 shell
and 201 structured inputs, each replayed for three consumers (22,377
observations). Its SHA-256 is
`9f27697356e242f5b145c2e560d921d9705cab5086aa900b1decbc40f4c3a0df`.
All 49 provenance file hashes match. The audit corrected 260 Astra shell
field classifications without changing IDs or original events, and retained
F3 from the Phase 1 host inputs as an explicit known-limit row. Two additional
lexical regression inputs and F3 extend the initial 7,456-input observer set;
none are denominator exclusions. The full corpus remains 1,289 records:
1,265 operations / 3,795 consumer observations plus 24 filesystem setup rows.
Its frozen sources, scope selection and CHANGE labels are unchanged.

The materialized request replay maps cached fixture homes and task markers
to an empty synthetic HOME. It checks word/role changes, not every reviewer's
filesystem topology or execution-owner harness context. The full differential
uses its original synthetic identity fixtures; dev and D23 tests retain their
own authoritative fixtures. No submitted protected command is executed.
An independently built archive evaluator matches all 22,377 observations
from the retained 61e2933 evaluator, excluding timing.

Word-start comments now respect escaped separators and word-level groups;
`<<<` is consumed atomically before heredoc recognition. Brace reach queries
the escaped-dollar owner. Qualifier extraction removes one outer quote layer,
and closing delimiters stay in their opening nesting context. Each active
original parameter region has supported bounded-node coverage or explicit
unsupported coverage. Known fragment parse limits become U-R; genuine faults
still propagate. Fragment re-entry preserves quote context and completes
independent code observations. D30 modifiers have their own cause and span;
array assignments no longer trigger equals-process substitution. Visible
assignment-index code is forwarded through arithmetic observation, while
quoted assignment data stays inert.

| Check | Final observation |
| --- | --- |
| Full corpus | 121 conflict rows / 347 conflict observations, unchanged |
| Previously matching RETAIN | 2,479 observations remain matched; no regressions |
| CHANGE | All 21 rows / 63 observations pass their frozen contracts |
| Full-corpus changes | shell[149], all three consumers: D stays D; coverage improves and probes decrease from 95 to 32 |
| Manifest semantic changes | 3,960 observations: 3,726 class changes plus 234 same-class changes |
| New wire permits | 252 false-refusal/fault corrections; all are Go permits |
| Protection regression | No D-to-permit, no new Go-deny/Rust-permit |
| Recovery regression | No U-R-to-F |
| Historical Brush oracle | 16,036 parent disagreements became zero under the then-current token remapping; 198,731 compared bytes. This did not verify substitution body forwarding |
| Oracle limits | 267 tokenizer refusals and one retained F3 known limit; no word/nested refusals |
| Mechanism ablations | 16 compile-success / named runtime failure / exact-byte restore / passing rerun receipts |
| Full test suite | 103 pass; only `every_legacy_row_is_accounted_for` fails |
| fmt, Clippy, release, cargo-deny | Pass offline; cargo-deny retains existing duplicate warnings |
| make rust-check | make 2 / Cargo 101 at the unchanged strict differential |
| Packaged binary | Version 0.6.0 exits 0; no args or checker exits 1 with no check performed |

The sole corpus change is ruling 23: `values=(a $(printenv))` is no longer
masked into phantom command operands. The independent dump remains D, with
the same reason, advice and recovery. Historic M1.3 probe counts also differed
because its fixture directory was one component shallower. Rebuilding
61e2933 and replaying at the current fixture depth removes those topology
deltas; `historical-corpus-observations.jsonl` retains them separately.

| Manifest class change | Observations | Disposition |
| --- | ---: | --- |
| N to D | 1,038 | Framing or fragment context reveals independently protected code |
| N to U-R | 1,023 | Active unsupported source regions / listed D1 modifiers block |
| F to U-R | 873 | Known adapter limit refuses syntax rather than faulting |
| U-R to D | 432 | Independent protected effects complete and take precedence |
| F to N | 210 | Literal-context controls no longer fault; Go permits |
| D to U-R | 84 | Redirect followed by a real comment is incomplete syntax; both results block |
| U-R to N | 42 | Array assignment, escaped delimiter and here-string false refusals; Go permits |
| U-C to U-R | 15 | Explicit unsupported coverage blocks |
| F to D | 9 | Known fragment limit no longer aborts an independent protected observation |

Each mechanism receipt binds all build inputs, the changed source and test
binaries, exact mutation, successful compile, named failing assertions,
restore hash and successful reruns. Receipts cover comment framing,
here-strings, escaped dollar, F2 mapping, qualifier quotes, parameter pairing,
region state, coverage forwarding, known fragment error class, genuine
piece faults, D30 detection, array-assignment exclusion, fragment context,
assignment-index forwarding, legacy arithmetic and nested closing context.
The first here-string mutation and the single-depth closing mutation did not
fail assertions and are diagnostics, not successful ablations.

The 121 conflict rows remain assigned individually in
`owner-scope-backlog.jsonl` and its Markdown table. Shell[175] remains a P5
option-value role defect: a long Git `-m` message becomes a path, then fails
with ProbeFault. The probe error must remain a fault while the target producer
is repaired. Deferred owners are not implemented by this packet.

The two A3 binding-to-arithmetic rows remain Go exit 0 / Rust N while Bash
executes the echo-only HIT witness. They are the first bounded P2 binding item
in M2. Inherited arithmetic values remain unobservable and block cutover,
like D28-CDPATH. The R1 evaluated-variable sibling is separately recorded as
a Go baseline false permit under ruling 20. F3 retains the exact input:
Bash's outer echo exits 0 while its backtick child fails and runs no printf;
Zsh exits 1. The unparenthesized case-pattern depth limit has syntax-only host
evidence (Bash rejects, Zsh accepts) and remains a documented divergence,
with U-R unchanged. No Go repair or consumer cutover is claimed.

## Historical M1.3 evidence

M1.3 replaces partial raw-shell quote scanners with one lexical owner under
decisions.md v3.1 D29 2–7 and records D29 8. The baseline is
`577218f2147e097d8f746272e8ff0aef552c19a4`. M1 remains unaccepted until the
same coordinator-owned Opus 5.5 and DeepSeek reviewers complete successful verification
(D29 1/10). **M2 Phase B has not started.** P2 binding producers and P3 heredoc
parsing retain their assigned owners.

The 1,289-row corpus has zero verdict/category/exit/reason/advice flips and
zero new row or consumer conflicts. The expanded 1,875-observation variant
set has 114 semantic/class changes, all assigned below, and zero new
Go-deny/Rust-permit. These are finite-set claims, not arbitrary shell or
production equivalence. Go remains authoritative; hook paths, Go code,
dependencies, public setup and the version-only packaged Rust entry are
unchanged.

No holdout, real credential store, private key or declared environment file
was read. Test homes, filesystem canaries and probes are synthetic. Cargo
HOME/target and Go caches are external. Host probes print names or benign
markers, or use built-in read/printf on synthetic heredocs; they do not read
protected contents. No concurrency, bypass, configuration switch, installation,
permission change, remote write, release or paid call was added.

## Source and measurement binding

Current artifacts live under `~/.cache/guard-fixtures/fixture-seat/r4h-m1-3-*`.
`initial-state.json` records the clean tracked baseline and verifies M1.2's
source and four untracked-plan hashes against its delivery binding.
`input-binding.json` binds all 19 original reviewer data sources. Reviewer
runners were not executed or modified. `measurement-binding.json` binds the
final source, fixtures, raw artifacts and binaries; `final-delivery-binding.json`
adds commit ancestry and final index/tree checks. The four untracked plans
and external M2 Phase A plan remain byte-identical.

Baseline corpus raw SHA-256: `bd291c3ecee20d2e367b81f3449aa0c77ff580025999c755ae5b71a1f5d6521a`.

| Input | SHA-256 | M1.3 change |
| --- | --- | --- |
| `contract.jsonl` | `1e223c6453d6883acc88af9967beab4251ba0fc6d636a1186482b6e4b524c695` | None |
| `filesystem.json` | `b1d51062925ccbfdfae5c8ffbc3e130e25391f31b05207448ee02be8cc874b3e` | None |
| `rust-d22-scope.jsonl` | `a3e41fd04bf1daf575c7312f9a275d298203ac7493228f6517dbcbb1fccd18ac` | None |
| `rust-slice-dev.jsonl` | `385e395dbee8855c2985a7b6d2f92083b898e1614573ad4f7b9ac71b8597be10` | None |
| `rust-contract-classification.jsonl` | `87171281c243ebceefed14d6fbb103c5f75187239dab3c449652d3d156b63ed5` | None |
| `rust-review-d22.json` | `44d12aea41a947e0c1996071b70bb9777293bfa6b58f0b3d4bbd63060debc632` | None |
| `rust-m1-1-regressions.json` | `3a30251e6159235f512dc07f3bcc947ec5bf13571b57a963a7752609ec47250f` | None |
| `rust-m1-2-regressions.json` | `b1f591d91cec5d1ddd222817af7b98530aae896de0772969a227ac2c4a3375e9` | None |
| `rust-m1-3-regressions.json` | `edd0e80da1343c3c5feec25c977e91065d913bb1c662cbe47847316756dd8ddc` | 28 owner/control rows |
| `rust-m1-3-reviewer-inputs.json` | `cc82d3ee5a748cd65b72e7d2d67e23f46a9ab5e2ded5c5986cce70fe6f6f45f4` | 491 reviewer shell inputs with provenance |

Installed versions (`tool-versions.json`): Rust/Cargo 1.98.1, Clippy 0.1.98,
cargo-deny 0.20.2, Go 1.27.1, Bash 3.2.57(1), Zsh 5.9. Brush is locked to
0.4.0. Final evaluator and comparator hashes appear in the measurement binding;
the external 577218f evaluator is `47ed05f0…540c`.

## One lexical owner and corrected inventory

`src/shell/lexer.rs` supplies per-byte `Unquoted`, `Single`, `Double`, `AnsiC` and
`Gettext` context, command/parameter/backtick/arithmetic depth, escape, comment,
heredoc delimiter and quoted/unquoted body context. A dollar-apostrophe or
apostrophe inside Double is ordinary text. Nested executable code receives
its own context. Heredoc declarations are queued and terminators match complete
lines (with the existing tab-stripping syntax); bodies and end tags are marked
at the same owner. `quotes.rs` and scanner-owned quote tracking are deleted.
The raw lexer runs independently of Brush, including after a parser failure.
Brush still owns decoded word values; it is not a production quote-state oracle.

M1.2's `docs/rust-slice-evidence.md:89` claimed the D1 literal skip was "Fixed".
That claim was false for M12-N1: the partial scanner treated apostrophes inside
Double as openers and hid subsequent executable syntax. The inventory below
replaces that audit and includes the formerly omitted Target tilde test.

| Consumer / boundary | Before at 577218f | Current query / disposition |
| --- | --- | --- |
| `words::brace_text` | quotes::skip walked raw spans; no shared full-source quote state | Lexed::scan and Context::word_syntax; brace nesting only remains in consumer |
| `words::brace_group` | quotes::skip before brace/comma scan | same Lexed context word_syntax for braces and separators |
| `words::fill splitting/glob` | recursive quoted bool from Brush Double/Gettext pieces and heredoc flag | Lexed context unquoted/heredoc; decoding remains Brush-owned; resolved unquoted parameter text supplies Bash glob reach |
| `D1 literal masking` | skipped backslash, Single and ANSI-C wherever seen, including inside Double | Context::active excludes literal quote modes, escapes, comments, literal heredoc bodies and delimiter |
| `D1 parameter flags` | raw ${ syntax after partial literal skipping | Context::active, including active Double/Gettext and nested command code |
| `D1 paren/glob/process gates` | divergence::closing ran independent quoted-span skipping | Context::unquoted and Lexed::closing; consumer retains only content/position classification |
| `D1 dialect command gates` | raw statement-boundary scan after partial skipping | Context::unquoted and existing statement-boundary contract |
| `D1 heredoc masking` | raw prefix/newline search plus quotes::heredoc_delimiter | Lexed delimiter/body/end-tag contexts, full-line end-tag queue and nested expansion contexts |
| `Target tilde test` | Raw starts with ordinary apostrophe or double quote | lexer::initial_quote query; Single/Double only per native/targets/infer.go:17 |
| `words::brace_sequence / ansi decoder` | group-endpoint or quote-body decoding | same non-state role; ANSI decoder reused by lexer delimiter removal |
| `filesystem glob grammar` | decoded pattern brace/paren/escape grammar | unchanged; no raw shell quote state |
| `targets::code_paths` | foreign-language CodeFile token quote boundary | unchanged foreign-language owner, outside raw shell quotation |
| `shell::check_nesting / variable context prefilter` | raw resource bound / overinclusive name prefilter | unchanged; neither interprets shell quote state |

`Target::from_word` queries `lexer::initial_quote`; it retains Go's
`native/targets/infer.go:17` Raw-prefix specification: only an ordinary leading
single/double quote suppresses tilde expansion. ANSI-C and escaped tilde
spellings retain the shared Go/Rust over-deny candidate (M12-N3), supported by
host name-printing readings. Gettext also retains the Go result; Bash keeps
literal tilde and Zsh prints literal dollar/tilde. No baseline behavior was
ported from those host differences in this round.

`RecordingProbe::literal_for_quoted_paths` names the word-free oracle's literal
path intent. `new(fixture, word)` still derives `patterned` from Word.globs.
The existing default-oracle regression protects pre-probe glob ordering.
Event-level mixed-word harnesses still lack universal per-target Word tracing;
the rename does not claim that stronger boundary.

Resolved plain unquoted parameter values containing `*`, `?` or `[` now set
Word.globs, preserving Bash pathname expansion under D1's union reading.
Quoted uses remain literal. No unresolved binding producer or shell-state
implementation was added (P2 remains pending).

A final boundary check found heredoc end-tag bytes were initially left active:
`cat <<'${(f)v}'` with body DATA and that literal terminator was Rust U-R / Go N,
while both hosts printed DATA using read/printf. The correction marks the tag
inert at the lexical owner. An active-tail control remains U-R.
`tag-boundary-check.json` retains the before/host evidence; current variant
records and the end-tag ablation retain the after result.

## Brush oracle and host readings

`m1_3::lexer_matches_brush_word_quoting` visits the 1,289-row corpus and all
JSON/JSONL `tests/fixtures/rust-*` fixtures, including the reviewer-input fixture.
Its 491 source-field inputs did not cover all 536 reviewer records: the other
45 included 24 Bash events and 21 structured events. Twelve distinct Bash
commands were omitted from this oracle, although the event-level comparator
replay retained them. M1.4's unified manifest includes those event schemas and
their shell command fields; the M1.3 counts below retain their historical scope.

| Oracle measure | Final |
| --- | ---: |
| fixture_files | 9 |
| oracle_inputs | 3456 |
| tokenized | 3449 |
| parsed_programs | 3429 |
| word_parses | 17721 |
| nested_scripts | 61 |
| compared_bytes | 109066 |
| tokenizer_refusals | 7 |
| Quote-context disagreements | 0 |
| Word-piece / nested-tokenizer refusals | 0 / 0 |
| Span discrepancies, explicitly mapped | 68 |

All 68 span discrepancies are Brush's synthetic heredoc end-tag token location:
it reports a position after consuming the tag, occasionally after its newline.
An independent AST body boundary plus exact original tag bytes identifies the
actual position. Every discrepancy records id, reported byte, actual byte and
disposition in `oracle-final.json`. The mapped bytes still undergo quote and
end-tag inertness comparisons; no quote disagreement is suppressed. This is
source-position metadata, not a different host execution reading.

Seven tokenizer refusals are the four existing M1.2 dollar-quoted heredoc rows
and their three DeepSeek reviewer forms. Both host shells accept the escaped
ANSI-C delimiter using benign read/printf. Go and Brush refuse that escaped
form; both guards stop it. **That row remains stopped as a Go baseline parser
limitation candidate (D29 6, P3/M3).** Plain dollar-quoted delimiter variants
remain Go N / Rust U-R through the pinned Brush parser. There is no P3 port.

The oracle compares Brush's available word-piece quoting, recursively including
Double/Gettext and command/backtick bodies. Parameter/arithmetic interiors for
which Brush supplies no nested word pieces are opaque apart from their markers;
the separate metadata contract tests their lexical nesting depth. Oracle
coverage does not establish complete shell grammar or universal host equivalence.

`host-readings.json` contains 22 probes; `tag-boundary-check.json` adds the tag
boundary reading. All use isolated HOME and no user startup files. Their
observable partitions are:

| Form | Bash | Zsh | Disposition |
| --- | --- | --- | --- |
| Escaped ANSI-C brace prefix | Both brace members printed | Same | N1 brace reach retained |
| Double apostrophe / dollar-apostrophe then eval flag | Bad substitution | CODE_MARKER executed | M12-N1 active code; D1 union |
| Balanced executable qualifier / +f | Syntax refusal | QUALIFIER_MARKER / PLUS_MARKER | Active even when qualifier code is quoted |
| Equals process substitution | Syntax refusal | PROCESS_MARKER and temporary-file path | Nested target extraction; no file contents read |
| Resolved unquoted star/question/bracket | Existing synthetic pathnames | Literal binding by default | D29 Bash reach |
| Quoted binding use | Literal pattern | Literal pattern | N control |
| Zsh globsubst | Not a Bash option reading | Existing synthetic pathnames | D29 reach / D1 command gate |
| Quoted heredoc nested substitution | Body stays literal | Same | D1 inert body |
| Escaped ANSI-C heredoc delimiter | Accepted, literal data | Same | Go/Brush limitation candidate; row stopped |
| ANSI-C / escaped tilde | Literal tilde | Same | Shared guard over-deny candidate; unchanged |
| Gettext tilde | Literal tilde | Literal dollar/tilde | Go Raw semantics retained |
| Six unterminated quote forms (syntax-only) | Refused | Refused | Go also exit 2; Rust malformed blocking path |
| Quoted heredoc end tag | DATA only | DATA only | Tag is inert; following active flags still U-R |

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

## Complete corpus differential against 577218f

The corpus compares the frozen Go contract fixtures: 1,265 operations × three
consumers = 3,795 observations, plus 24 metadata checks. There are 963 in-slice
operations and 302 outside. All previously matching RETAIN contracts and all
21 CHANGE rows remain matched; eight Claude-only advice conflicts explain the
non-threefold defect count. Live built-Go comparison is the variant run below.

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
outside observations. Remaining conflicts: 121 rows. Verdict/category/exit/
reason/advice flips: none. New row and consumer conflicts: zero. Raw reports
retain every unsupported/outside observation.

| Corpus Go-deny/Rust-permit | N | U-C | Total |
| --- | ---: | ---: | ---: |
| In-slice Rust defects | 168 | 123 | 291 |
| In-slice sanctioned D16/D25 changes | 6 | 0 | 6 |
| All in slice | 174 | 123 | 297 |
| Outside | 0 | 285 | 285 |
| Total | 174 | 408 | 582 |

## Every reviewer variant and every change

625 input records × three consumers = 1,875 observations: 536 reviewer inputs
(1,608 observations), 55 prior controls, 28 new owner/control rows and six
tracked credential-root inputs. Provenance duplicates are preserved rather
than treated as independent examples. Opus v11 contributes 179 records; v12
new12/new12b/new12c/new12d/ds12 contributes 80. All nine prior DeepSeek sets
contribute 221, and m12-harness contributes 56. The reviewer-input fixture
preserves all shell forms for the oracle; structured events stay in the raw
variant dataset.

Go is freshly built offline from unchanged cmd/agent-guard. Both exact events,
Go stdout/stderr/exit and Rust wire/coverage/effects are stored. Pi lower-case
read/write/edit sends path to Rust; Go receives canonical file_path, matching
native/core/protocol.go. The same inputs run at the external 577218f evaluator
and the final evaluator. Semantic comparison excludes timing alone. Go N is
checker exit 0; Go D is exit 2, including syntax refusals. Raw stderr preserves
that distinction from a protected policy denial.

M1.2's "No new Go-deny/Rust-permit" claim at `:182` covered only its 1,365
observations; it did not cover Opus's later five M12-N1 forms (15 observations).
Those forms are in the current set. The current zero-new claim is scoped to
the 1,289 corpus rows and the separate 1,875 variant observations.

| Variant measure | 577218f | M1.3 |
| --- | ---: | ---: |
| Combined permission mismatches | 267 | 285 |
| Reviewer-only permission mismatches | 192 | 201 |
| Class / semantic changes | | 114 / 114 |
| New required refusals differing from Go | | 57 |
| Previously differing permissions now matching Go | | 39 |
| New Go-deny/Rust-permit | | 0 |

All fixture/control expectations pass. UR regressions require the D1 executor/
dialect coverage gap, so independent parser refusal alone cannot pass them.
D regressions assert protected-denial reason, recovery scope, exit 2 and absent
advice; N controls assert exit 0 and absent advice.

| Every changed input ID (each is three consumers) | 577218f → M1.3 | Go | Owner |
| --- | --- | --- | --- |
| `Opus/v12/new12.json:d1-dq-dollar-sq-active` | N → UR | D | D29 double-quote / nested active parameter flags |
| `Opus/v12/new12.json:d1-dq-sq-active` | N → UR | D | D29 double-quote / nested active parameter flags |
| `Opus/v12/new12.json:d1-qual-after-dq-dollar-sq` | N → D | N | D29 active executable qualifier |
| `Opus/v12/new12.json:d1-qual-after-dq-sq` | N → D | N | D29 active executable qualifier |
| `Opus/v12/new12.json:d1-procsub-after-dq-dollar-sq` | UR → D | D | D29 nested process target extraction |
| `Opus/v12/new12.json:d1-setopt-after-dq-sq` | N → UR | N | D29 command gate / resolved glob reach |
| `Opus/v12/new12b.json:x-eval-flag-dq-sq` | N → D | D | D29 double-quote / nested active parameter flags |
| `Opus/v12/new12b.json:x-eval-flag-dq-dollar-sq` | N → D | D | D29 double-quote / nested active parameter flags |
| `Opus/v12/new12b.json:x-eval-flag-unq-dq-sq` | N → D | D | D29 double-quote / nested active parameter flags |
| `Opus/v12/new12b.json:x-qual-reply-dq-sq` | N → D | N | D29 active executable qualifier |
| `Opus/v12/new12b.json:x-qual-plus-dq-sq` | N → UR | N | D29 active executable qualifier |
| `Opus/v12/new12b.json:x-globsubst-dq-sq` | N → D | N | D29 command gate / resolved glob reach |
| `Opus/v12/new12b.json:x-globsubst-control` | UR → D | N | D29 command gate / resolved glob reach |
| `Opus/v12/new12b.json:x-procsub-dq-sq` | UR → D | D | D29 nested process target extraction |
| `Opus/v12/new12b.json:x-qual-dq-sq-later-cmd` | N → D | N | D29 active executable qualifier |
| `Opus/v12/new12d.json:y-var-glob-unquoted` | N → D | N | D29 command gate / resolved glob reach |
| `D29-regression/double_context:eval-double-apostrophe` | N → D | D | D29 double-quote / nested active parameter flags |
| `D29-regression/double_context:eval-double-dollar-apostrophe` | N → D | D | D29 double-quote / nested active parameter flags |
| `D29-regression/double_context:eval-unquoted-after-double` | N → D | D | D29 double-quote / nested active parameter flags |
| `D29-regression/double_context:flag-double-apostrophe` | N → UR | D | D29 double-quote / nested active parameter flags |
| `D29-regression/double_context:flag-double-dollar-apostrophe` | N → UR | D | D29 double-quote / nested active parameter flags |
| `D29-regression/qualifier:qualifier-balanced-statement` | N → D | N | D29 active executable qualifier |
| `D29-regression/qualifier:qualifier-balanced-word` | N → D | N | D29 active executable qualifier |
| `D29-regression/qualifier:qualifier-balanced-dollar` | N → D | N | D29 active executable qualifier |
| `D29-regression/qualifier:qualifier-balanced-path` | N → D | N | D29 active executable qualifier |
| `D29-regression/qualifier:qualifier-plus` | N → UR | N | D29 active executable qualifier |
| `D29-regression/command_gate:setopt-extendedglob` | N → UR | N | D29 command gate / resolved glob reach |
| `D29-regression/command_gate:setopt-globsubst` | N → D | N | D29 command gate / resolved glob reach |
| `D29-regression/process:equals-process-apostrophe` | UR → D | D | D29 nested process target extraction |
| `D29-regression/process:equals-process-dollar-apostrophe` | UR → D | D | D29 nested process target extraction |
| `D29-regression/expansion_glob:resolved-star` | N → D | N | D29 command gate / resolved glob reach |
| `D29-regression/expansion_glob:resolved-question` | N → D | N | D29 command gate / resolved glob reach |
| `D29-regression/expansion_glob:resolved-bracket` | N → D | N | D29 command gate / resolved glob reach |
| `D29-regression/expansion_glob:resolved-globsubst` | UR → D | N | D29 command gate / resolved glob reach |
| `D29-regression/expansion_glob:resolved-appdata` | N → D | N | D29 command gate / resolved glob reach |
| `D29-regression/masking:inert-comment` | D → N | N | D1/D29 inert comment |
| `D29-regression/masking:nested-command` | N → UR | D | D29 double-quote / nested active parameter flags |
| `D29-regression/masking:quoted-end-tag` | UR → N | N | D1/D29 inert heredoc end tag |

`variant-semantic-flips.jsonl` retains all 114 changed observations, including
wire and coverage. `variants-annotated.jsonl` assigns every remaining permission
difference to a ruling or later owner, without dropping unchanged differences.

| Sanctioned Go-deny/Rust-permit, counted separately | Observations | Decision |
| --- | ---: | --- |
| cat < ~ | 9 | D25 |
| Signed sequence | 9 | D26 |
| Literal shell/redirect/structured brace spellings | 51 | D27 |
| Assigned PWD baseline false denial | 3 | D28 |

The D25/D26/D27 count is 69; D28 contributes three separately ruled observations
(total 72). D27's 51 comprise the prior 45 plus six later Opus literal ANSI-C/
Gettext brace observations. Counts are provenance-specific and do not overlap
the deferred-owner or outside counts.

| Variant Go-deny/Rust-permit scope | N | U-C | Total |
| --- | ---: | ---: | ---: |
| In-slice deferred owners | 66 | 6 | 72 |
| In-slice sanctioned D25/D26/D27 | 69 | 0 | 69 |
| Assigned PWD baseline false denial (D28) | 3 | 0 | 3 |
| All in slice | 138 | 6 | 144 |
| Outside curl/cp | 0 | 9 | 9 |
| Total | 138 | 15 | 153 |

| Every remaining permission difference | Observations | Rust / Go | Ruling / later owner |
| --- | ---: | --- | --- |
| D26 2b quoted-member union | 9 | D / N | D26 |
| D26 4 deferred operandValue | 3 | N / D | P5/M4 |
| D26 4 deferred cwd | 3 | N / D | P2/M2; D28 state rulings |
| D28 4 assigned PWD baseline false denial | 3 | N / D | P2/M2 producers remain pending |
| D1/D22 split union | 3 | D / N | D1/D22 |
| D26 4 deferred stdin shell | 6 | UC / D | P3/M3 |
| D26 3 outside curl URL-glob loss | 6 | UC / D | curl URL-glob before cutover |
| D26 4 deferred stdin secrets | 15 | N / D | P6/M4 |
| D26 2b signed sequence | 9 | N / D | D26 |
| D27 3 literal brace baseline false denial | 51 | N / D | D27 |
| D1/D26 extglob union | 30 | D / N | D1/D26 |
| D25 4 decided permit | 9 | N / D | D25 |
| D27 4 deferred Git pattern file | 18 | N / D | P5/M4 |
| D1 executable qualifier refusal | 3 | UR / N | D1 |
| Pre-existing quoted-heredoc parser refusal | 9 | UR / N | P3/M3 pinned Brush delimiter parsing |
| D1/D29 executable qualifier nested target | 33 | D / N | D1/D29 |
| D1/D29 active syntax refusal | 18 | UR / N | D1/D29 |
| Out-of-slice cp program owner | 3 | UC / D | P5 before cutover |
| D29 5 resolved expansion Bash glob reach | 27 | D / N | D29 |
| D29 8 structured credential root | 27 | N / D | P6/M3; regression when owner lands |
| Total | 285 | | |

D29 8 tracked row: structured Read of .docker, .kube, .cargo, .config and
.config/gh, plus Grep of .config/gh, is Go D / Rust N on all consumers. Six
current tracking inputs give 18 observations; three DeepSeek inputs duplicate
part of that partition (nine more). Total 27. Go IsSensitiveRoot
(native/filesystem/credentials.go:130–147) recognizes the roots; Rust lexical
credential classes recognize file suffixes, and a structured Read has no walk.
This is a pre-existing P6/M3 loss, not repaired here; add the regression when
that owner lands. The external N expectations record current behavior only.

The Go word owner's false permit for resolved Bash glob reach is explicitly
ruled by D29 5. The escaped-heredoc Go syntax limitation is stopped under D29 6.
Neither case directs an unapproved Go change. Other deferred differences,
including cp, remain unchanged. Permission-equivalent coverage distinctions
are retained. The Glob tool remains outside observed coverage.

## Baseline failures and final exact-byte ablations

The final six functional owner tests compile against an external 577218f
archive and all fail at runtime. Only the renamed constructor call gets
compatibility spelling; no repaired production/support owner is copied into
that baseline. `baseline-final-regressions.log` and `baseline-final-tests.json`
retain those failures. All ten current owner/oracle/metadata tests pass.

Each final ablation uses one external source copy. It runs m1_2 and m1_3,
requires the named assertion to fail at runtime (exit 101, no compiler error),
builds the examples and runs the full corpus plus all 1,875 variant observations,
restores exact original bytes by SHA-256, then repeats the assertions and both
reports. Every restored projection equals the final normal projection. Timing
is excluded from semantic equality. All 23 bindings match final checkout bytes.

| Mechanism | Named failing test | Corpus rows changed | Variant class / semantic changes | Oracle differences | Broken / restored |
| --- | --- | --- | ---: | ---: | --- |
| ansi | `lexer_matches_brush_word_quoting` | None | 111 / 126 | 35 | 101 / 0; exact bytes |
| arithmetic-depth | `lexer_exposes_nested_and_heredoc_context` | None | 0 / 0 | 0 | 101 / 0; exact bytes |
| backtick-depth | `lexer_exposes_nested_and_heredoc_context` | None | 0 / 0 | 0 | 101 / 0; exact bytes |
| binding-glob | `resolved_unquoted_bindings_keep_bash_glob_reach` | None | 27 / 27 | 0 | 101 / 0; exact bytes |
| brace-group | `ansi_member_preserves_brace_reach` | appdata[82], credentials[189] | 237 / 237 | 0 | 101 / 0; exact bytes |
| brace-text | `ansi_prefix_preserves_brace_reach` | appdata[82], credentials[189], options[8], options[9], options[10], options[11] | 312 / 312 | 0 | 101 / 0; exact bytes |
| closing | `executable_qualifiers_survive_double_quoted_apostrophes` | None | 60 / 63 | 0 | 101 / 0; exact bytes |
| command-depth | `lexer_exposes_nested_and_heredoc_context` | None | 0 / 0 | 165 | 101 / 0; exact bytes |
| comment | `literal_data_and_nested_code_keep_d1_masking` | None | 3 / 3 | 0 | 101 / 0; exact bytes |
| d1-context | `literal_data_and_nested_code_keep_d1_masking` | None | 27 / 42 | 0 | 101 / 0; exact bytes |
| double-active | `double_quotes_keep_inner_apostrophes_literal` | None | 39 / 45 | 0 | 101 / 0; exact bytes |
| double | `lexer_matches_brush_word_quoting` | appdata[83], search[17], interpreters[36] | 150 / 177 | 4401 | 101 / 0; exact bytes |
| escape | `lexer_exposes_nested_and_heredoc_context` | None | 0 / 0 | 66 | 101 / 0; exact bytes |
| gettext | `lexer_matches_brush_word_quoting` | None | 0 / 0 | 106 | 101 / 0; exact bytes |
| heredoc-body | `literal_data_and_nested_code_keep_d1_masking` | None | 3 / 18 | 897 | 101 / 0; exact bytes |
| heredoc-delimiter | `lexer_exposes_nested_and_heredoc_context` | None | 0 / 0 | 0 | 101 / 0; exact bytes |
| heredoc-endtag | `literal_data_and_nested_code_keep_d1_masking` | None | 3 / 3 | 203 | 101 / 0; exact bytes |
| parameter-depth | `lexer_exposes_nested_and_heredoc_context` | None | 0 / 0 | 0 | 101 / 0; exact bytes |
| quoted-glob | `resolved_unquoted_bindings_keep_bash_glob_reach` | None | 6 / 6 | 0 | 101 / 0; exact bytes |
| single | `lexer_matches_brush_word_quoting` | None | 24 / 24 | 8982 | 101 / 0; exact bytes |
| tilde | `target_tilde_uses_lexical_prefix_context` | shell[169] | 27 / 27 | 0 | 101 / 0; exact bytes |
| unterminated-len | `unterminated_quotes_are_blocked` | None | 0 / 0 | 0 | 101 / 0; exact bytes |
| unterminated-none | `unterminated_quotes_are_blocked` | None | 0 / 0 | 0 | 101 / 0; exact bytes |

ANSI ablation breaks the oracle on N1 (`rust-m1-2-regressions[0..5]` and Opus
b-ansi-quote-desync); Double ablation breaks it on M12-N1 (all five current rows
and Opus's corresponding flags/eval inputs). Broken oracle JSON retains each
id and byte difference. Both `unterminated-none` and `unterminated-len` fail
`unterminated_quotes_are_blocked`; their zero corpus/variant delta shows why the
explicit lexical contract is necessary. Some metadata mutations have no verdict
change in this finite set; their context assertion still detects the lost field.
All 28 new regression rows have a semantic counterexample in at least one final
ablation, recorded in `regression-ablation-coverage.json`.

Initial oracle diagnostics and the completed pretag ablations/gates are kept
as history. They are superseded by final source-bound measurements; no diagnostic
run or intermediate oracle failure is counted as a final pass. Span-debug
instrumentation was removed. The final end-tag boundary regression and ablation
close the additional owner omission found during cleanup.

## Mechanical gates and remaining acceptance

| Gate | Final result | Artifact suffix |
| --- | --- | --- |
| cargo fmt -- --check | Pass | fmt.log |
| cargo clippy --offline --locked --all-targets -- -D warnings | Pass | clippy.log |
| cargo deny --offline --locked check | Advisories/bans/licenses/sources pass; five existing duplicate warnings, no fetch | deny.log |
| cargo build --offline --locked --release --all-targets | Pass | release.log |
| cargo test --offline --locked --all-targets --no-fail-fast | 80 pass; only every_legacy_row_is_accounted_for fails, 121 rows / 347 observations | tests.log |
| make rust-check, CARGO_NET_OFFLINE=true | make 2 / Cargo 101; same sole strict failure | rust-check.log |
| Named Brush oracle | Pass, with all discrepancies/refusals recorded | oracle-final.log/json |
| Packaged binary | --version 0.6.0 exit 0; no args/checker exit 1, no check performed | binary-smoke.json |

The complete strict differential remains open at 121 conflicts. D28 tracked
cwd/PWD/CDPATH, home and filesystem identity work awaits M2 Phase B. Heredoc
parsing and shell/stdin execution stay P3; bindings stay P2. D29 8 root classes
stay P6/M3. Git grep -f, operandValue, program/search roles and Target.search's
help/version producer remain P5/P7 as assigned. D25 suffix-shape policy, App
Data touch/scan gates, stream/content rules and directory-collapse owners remain
pending. The stopped heredoc parser and shared tilde over-deny candidates remain
recorded rather than ported.

The all-target suite includes the unchanged dev/lifecycle/writer/JSONL/SSH tests.
It is not fresh installed loading or real consumer Gate A/B acceptance. Execution
owners, production performance and cutover remain unaccepted. The same two
coordinator-owned reviewers must now verify M1.3, with the lexer oracle as an
explicit attack surface. No alternate reviewer verdict is substituted.

Changed: one raw lexical owner replaces partial scanners; resolved unquoted
bindings retain Bash glob reach; test-oracle intent and source-bound evidence
are explicit. Retained: distinct executable/literal, brace/paren/tilde,
malformed-quote, nesting/escape/heredoc and probe-order contracts. Candidates
left: the named deferred owners, event-oracle provenance limit and required
coordinator verifications. Validation: final gates, full corpus/variant replay
and 23 restored ablations above. M1.3 stops here.
