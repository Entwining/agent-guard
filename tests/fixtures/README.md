Fixtures contain synthetic inputs and expected behavior. Migration discussions,
review provenance and source hashes belong in Git history and local records.
Adding a behavior partition does not require updating a corpus-size assertion.

`contract.jsonl` holds consumer expectations for verdicts, denial reasons and
advice. `rust-contract-classification.jsonl` selects the applicable expectation:
`RETAIN` uses the original consumer expectation; `CHANGE` supplies the typed
outcome, coverage, reason, advice and recovery contract. Contract IDs join by
`family[index]`. Filesystem-link IDs join by relative link path and validate
setup metadata, not a checker operation. Every input must be accounted for,
including rows classified outside a supported mechanism's scope.

`filesystem.json` declares empty surrogate files, directories and links inside
the fixture home. `rust-d22-scope.jsonl` supplies program-selection and unresolved
input labels used by the differential report. These labels do not establish
installed-hook or runtime acceptance.

## Synthetic paths and inputs

`$E` is a harness-owned temporary root; `$H=$E/home`, `$P=$H/project`, and
`$C=$H/Library/Containers/com.example.fixture`. `$R` denotes the synthetic home's
parent and `$U` the synthetic username. Expand complete marker tokens, including
quoted data, without replacing prefixes of `$HOME` or another shell variable.
The evaluation context and executed fixture helper use the same synthetic HOME.
Generic foreign-home paths such as `/Users/me` and `/Users/fixture` are inert
inputs; they do not refer to the invoker's home. Volume aliases are synthetic
inputs for the documented macOS path-resolution contract.

The `base-v1` setup contains owner-created UTF-8 surrogates: `input.txt` contains
`needle\n.env\nprocess.env.SHELL\n`, `notes.txt` contains `ORIGINAL_NOTES\n`,
`untracked-work.txt` contains `UNTRACKED_WORK\n`, and `out.txt` contains `KEEP\n`.
`fixture.txt` initially does not exist. Protected-name surrogates contain only
`SYNTHETIC_CANARY\n`, never credentials. Per-row files, links and faults extend
this setup. A session ID denotes a fixture session, never a live session.
Submitted shell text is inspected, not executed. Closed operation witnesses
execute only their harness-owned surrogate operations.

## Development behavior rows

`rust-slice-dev.jsonl` contains one consumer/scenario per line. The test evaluates
labelled supported rows through both parser arms and checks the library result,
consumer wire, probes, recovery and closed-operation witnesses.

| Field | Behavior contract |
| --- | --- |
| `id`, `consumer` | Unique case identity and `claude`, `codex`, `pi`, or `owned-writer`. |
| `tool`, `input`, `cwd` | Consumer-native input and synthetic cwd; malformed cases may provide `raw_event_bytes`. |
| `dialect_executor_assumption` | Trusted fixture executor context; tool names are not executor attestation. |
| `provenance_form` | Input mechanism, including the required-execution-owner boundary. |
| `task_objective`, `synthetic_setup` | Intended task and base profile with case-specific resources. |
| `outcome_class`, `expected_coverage` | Expected typed decision and coverage; a failed check has no completed Evaluation. |
| `reason_contract`, `advice_expectation` | Required semantic reason and concrete alternative, or absence of reason/advice. |
| `recovery_objective` | Rechecked same-consumer operation or owner action, result, excluded scope and automatic-application boundary. |
| `allowed_effects`, `prohibited_effects`, `observer` | Closed-witness requirements; their declarations are not recorded passes. |
| `conditional_outcome` | Unsupported active syntax refuses; independently identified protected effects must still deny. |
| `metamorphic_variant` | Base case and varied fact whose unchanged behavior is checked. |
| `fault_injection`, `writer_state` | Explicit synthetic fault or durable fixture-writer state. |
| `status`, `coverage_limit` | Label status and evidence scope, including unresolved cases. |

N permits quietly; A permits with applicable advice; D denies a protected effect.
U-C preserves a limited-preflight gap with quiet wire; U-R rejects an unsupported
syntax, inspection-budget or identity case; U-O requires a verified execution
owner. F blocks on operational failure. W describes the harness-owned writer,
not production write preservation. A project-scoped continuation does not
complete a whole-HOME task. A coverage gap is not proof of runtime confinement.

The recording Probe checks lexical protection before each probe. App Data
traversal uses safe-prefix readlink; stat/inode checks belong to SSH identity.
Closed witnesses observe surrogate effects and operation starts; empty stdout
does not prove that no protected access occurred. Protocol, hook loading,
lifecycle and actual client acceptance remain separate checks.

## Parser and mechanism corpora

`rust-parser-inputs.jsonl` contains only an ID and shell source or structured
event. The lexer/Brush differential consumes shell rows; the report example
consumes both forms. `lexer_refusal=true` identifies the malformed backtick-child
partition that Brush tokenizes but the lexer rejects. Tokenizer refusals,
lexer refusals and actual word comparisons are reported separately.

Other JSON files hold named mechanism partitions and expected class, effect,
path identity, gap, reason or advice. Numeric recursion, record, payload and
alias-hop bounds are behavioral boundaries: their exact-limit and excess cases
are independent fixtures. Large-output sizes exercise complete pipe drainage.
Assertions about those bounds differ from assertions about a corpus's current
size. Update expected behavior only for an intentional contract change, with a
regression that distinguishes it and a mechanism ablation.
