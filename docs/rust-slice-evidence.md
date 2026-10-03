# Offline Rust R4g evidence

The named R4f review repros and the D23 SSH identity regressions pass after repair,
but the full acceptance check is blocked: the expanded requirement-owned
differential has 140 Brush and 145 Tree-sitter RETAIN conflicts. The report does
not claim Go conformance. Go remains authoritative; the Rust trial is on no hook
path. The packaged `agent-guard-rust-slice` remains version-only. The held-out set
and holdout scrutiny files were not read.

Measurements bind source HEAD `b2626ef5b19d5dcbf4f8f787d396ae1f8561ded8` to
every tracked Rust source, test, fixture, example and build input through SHA-256
in `r4g-measurement-binding.json`. A later documentation-only commit can be
identified by its parent and unchanged measured source hashes; the final delivery
binding records the actual HEAD and that relationship. Measurements use Darwin
27.0.0 arm64, rustc 1.98.1 and Cargo 1.98.1, with synthetic HOME and an external
target directory. Go sources and dependencies are unchanged from `cd51eb4`.

## Development and test contract

The unchanged manifest has 260 labelled rows: 239 counted evaluator cases, nine
metamorphic variants, nine harness-only lifecycle rows and three owned-writer
rows. Dev evaluation checks the 248 evaluator rows through both parser arms
(496 checks); the variants also have 18 base comparisons. The three-arm class
denominator is 239; each latency arm has 248 samples. Lifecycle receipts retain
ready PID, `wait`, `try_wait`, completion and reaping assertions in a separate
nine-row test. They do not measure a production checker or contribute evaluator
F, latency or protection counts.

The frozen manifest SHA-256 is
`385e395dbee8855c2985a7b6d2f92083b898e1614573ad4f7b9ac71b8597be10`.
No manifest rows were added or relabelled. The harness applies eight explicit
decision corrections without changing those bytes: S17-replacement-codex/pi
expect library A under D22/D12, and the six runtime-selected-read/runtime_config
rows require InterpreterChosenRead under D22/D17 instead of the old
UnresolvedTarget spelling. Wire remains quiet for the two non-Claude advice rows;
the six configuration witnesses still expose permitted protected synthetic reads.

Assertions now require tool_class and effect_or_failure, including absent advice
and per-kind F/U-R messages. Conditional nested-read denials use the evaluator's
typed ProtectedTarget/AppData/Nested record, not the harness's eventual physical
read or the class being tested. Static effect records and closed-operation
effects are separately reported. Gate A asks the product adapter whether the
rendered consumer wire permits the call; its bypass negative breaks that product
predicate and detects a synthetic protected read. Every F kind blocks with exit 2
on Claude, Codex and Pi.

W01-W03 use a test-only single writer. W03 now obtains PartialStagedFailure from
an observed `Write` error after five accepted bytes, records its I/O kind and
partial artifact, and verifies durable state, retained proposal, original target
bytes and retry. W02 is the intervening-owner conflict. Nondiscriminating sibling,
untracked-file and guard-off equality assertions were removed. This is not
arbitrary-writer, concurrent-writer or crash-recovery acceptance.

At the measured source, fmt and warnings-denied Clippy pass. `make rust-check`
fails at `every_legacy_row_is_accounted_for` (Cargo 101, make 2), before later
targets. A separate `cargo test --locked -- --skip
every_legacy_row_is_accounted_for --nocapture` passes the remaining 46 tests;
this filtered run does not replace the failed required check. The dev contract,
variants, lifecycle, writer, interface, review and SSH tests are in that run.
Cargo-deny separately passes advisories, bans, licenses and sources with existing
duplicate-version warnings. Release builds pass. Dependencies and Cargo.lock are
unchanged.

## Offline evaluation interface and recovery

`examples/evaluate.rs` is a development JSONL interface. Guard mode calls the
library, DiskProbe and consumer render; it never executes the submitted operation.
Required fields are id, consumer, arm, home, cwd and exactly one event object or
event_raw string. Consumer/tool/event fields supply the operation and event cwd;
the host supplies HOME and fallback cwd. Optional context accepts only
zsh_executor, a host executor fact, defaulting to Zsh except for Pi. Empty or
relative event/fallback cwd is MalformedInput F. Codex exec_command workdir wins
over envelope cwd. Unknown context fields are rejected. Context has no objective,
public_task, project oracle or proposed continuation; no real environment values
or credentials are collected by this example.

Guard output includes outcome/class, coverage, disposition/cause, reason, advice,
recovery, observed_effects and exact exit/stdout/stderr. F has no completed outcome
and uses BlockOnCheckError. Invalid request fields return a sanitized request_error,
not a permitting guard verdict. Transport I/O failure exits unsuccessfully.
evaluate_ns measures evaluation/rendering only, excluding JSON I/O and setup.
Parse-only mode measures fresh parser construction/parsing/disposal and reports
non-shell or structured-only requests outside parser timing. Both modes are
synchronous; no threads, async execution or cache were introduced.

Recovery uses only event/host facts, resolved cwd/HOME/scope and typed detector
records. It names the excluded HOME for broad searches and gives a concrete owner
action. The agent chooses its continuation and establishes task equivalence.
Manifest next operations remain explicit agent-continuation witnesses rechecked
by the harness; these counts do not prove completion of the whole-HOME objective.
Quoted qualifier text does not select executable-qualifier recovery. Policy
decides advice for every consumer; render delivers it only to Claude.

## Review regressions

The 87 named review cases produce 174 arm observations: 159 fail at `1f9fbb2`
and 15 controls already pass there; all 174 pass after repair. Already-passing
controls are retained without inventing a prior failure. The old source archive
and new test inputs are outside the checkout, with a separate baseline target.
An independent release JSONL batch has 172/172 passes (86 cases, both arms).
The per-finding counts below overlap when one input belongs to two findings.

| Finding | Named regression | Before failed / already passed | After passed |
| --- | --- | ---: | ---: |
| B1 | `review_wire` | 4 / 0 | 4 |
| M6 | `review_wire` | 2 / 0 | 2 |
| B2 | `review_shell` | 5 / 5 | 10 |
| B3 | `review_program` | 20 / 0 | 20 |
| B4 | `review_program` | 6 / 0 | 6 |
| M1 | `review_identity` | 12 / 0 | 12 |
| M2 | `review_program` | 6 / 0 | 6 |
| M3 | `review_program` | 4 / 0 | 4 |
| M4 | `review_program` | 16 / 0 | 16 |
| M5 | `review_metadata` | 2 / 0 | 2 |
| F1 | `review_program` | 6 / 0 | 6 |
| F2 | `review_identity` | 8 / 4 | 12 |
| F3 | `review_shell` | 6 / 0 | 6 |
| F4 | `review_shell` | 6 / 4 | 10 |
| F5 | `review_program` | 6 / 0 | 6 |
| F6 | `review_program` | 8 / 0 | 8 |
| F7 | `review_identity` | 6 / 2 | 8 |
| F8 | `review_program` | 6 / 0 | 6 |
| F9 | `review_shell` | 14 / 0 | 14 |
| F10 | `review_identity` | 2 / 0 | 2 |
| F11 | `review_identity` | 2 / 0 | 2 |
| F13 | `review_metadata` | 2 / 0 | 2 |
| F15 | `review_program` | 6 / 0 | 6 |
| F18 | `review_shell` | 3 / 1 | 4 |
| m1 | `review_metadata` | 2 / 0 | 2 |
| m2 | `review_metadata` | 2 / 0 | 2 |
| m3 | `review_metadata` | 4 / 0 | 4 |
| m4 | `review_metadata` | 2 / 0 | 2 |
| m5 | `review_metadata` | 4 / 0 | 4 |
| F12, F13 | reason_and_effect_partitions_reject_missing_evidence; outside_tool_class_rejects_a_different_tool | Both named tests fail at 1f9fbb2 | Both pass |
| F14 | conditional_denial_requires_the_nested_effect_record (old harness); reason_and_effect_partitions_reject_missing_evidence and dev_contract (current) | Old conditional D without its effect record is wrongly accepted; stronger test fails | Missing/Nested effect assertions pass; nested-provenance ablation fails |
| m7 | partial_state_requires_an_observed_write_error (old writer); owned_writer_states (current) | Old original test passes; stronger I/O assertion fails | Current state/I/O assertions pass |
| M6 lifecycle | lifecycle_receipts_have_a_separate_evidence_owner (old harness); lifecycle_harness_only (current) | All nine old receipts lack separate owner; named test fails | Nine separate receipts pass |
| F17 | identity_depth_bound_is_not_syntax_or_success | Old ablation only rewords exclusions; not a faulty baseline verdict | New exclusion-removal ablation fails 101; exact restoration passes 0 |
| m6 | review_metadata plus all U-R review cases; review_b1_m6_failures_block_from_consumer_wire | Old Debug U-R text and F wire assertions fail | Per-kind messages and all eight F kinds pass |
| F16 | measurement and final-delivery source/hash binding | Historical report did not bind its measured source to actual HEAD | Source/raw/binary hashes and final ancestry are checked; no fictitious runtime before-failure is claimed |

The review wire test covers all eight F kinds through three consumers, beyond the
two malformed-input repros. Program tests cover code/script/positional operands,
grep/rg options, xargs options, env/set/variable dumps and the setup search
programs. Shell tests cover parsed-tree substitutions in tests, arithmetic, case
and loops, loop-value union, <> reads, all detector statement boundaries and
continued lines. Metadata tests cover resolved cwd, schema precedence, policy
advice and oracle-free recovery. Identity tests cover globs/groups, resolved HOME,
non-UTF-8 link rejection and the Go SSH public lexical class.

## D23 SSH identity and probe boundary

Stat is used only for Go SSHScopeDenied metadata comparisons: HOME/.ssh, its
FollowLinks-resolved root, search root parents, candidates and their parents after
lexical checks. App Data remains lexical-first/readlink-only. Private spellings
and other protected material are decided before any probe. No file contents are
opened or read by this identity owner. Device/inode comparisons, public regular
file controls, hard-linked public controls and directory file types follow Go.
Normal absent/not-a-directory results retain Go's absence handling; operational
stat errors propagate as ProbeFault F and block with exit 2.

The old owner at `cca9d46` fails all ten D23 observations: the three public-looking
directory names, aliased resolved root and search parent, through both arms.
Current SSH tests pass .ssh/config.d, directory.pub and known_hosts.backup
directory denials, public regular/hard-linked controls, fault blocking, a real
symlinked SSH-root alias, resolved-root search parents, and a metadata parent
inode alias. Directory hard links cannot be created on this host, so the latter
uses the Probe metadata boundary, while regular hard links and root symlinks are
real synthetic filesystem objects. Removing either root or parent comparison
fails the identity regression; separate search-parent, file-type and fault
ablations also fail and recover. A release-interface D23 batch passes 36/36
observations (six cases, both arms, all three consumers).

The direct identity regression records this complete per-candidate probe list:

| Private/protected spelling | readlink calls | stat calls |
| --- | --- | --- |
| $H/.ssh/id_rsa | [] | [] |
| $H/.ssh/id_ed25519 | [] | [] |
| $H/.ssh/keys/id.pub | [] | [] |
| $H/.ssh/CONFIG | [] | [] |
| $H/.ssh/id.PUB | [] | [] |
| $H/.ssh/known_HOSTS | [] | [] |

These are direct candidate identity calls; a whole event may separately inspect
its public cwd. The SSH public basename class is case-sensitive as Go specifies;
case folding must not turn the last three spellings into probeable public names.

## Release arm comparison

| Arm | N | A | D | U-C | U-R | U-O | F | Zero starts / required blocks | Closed results | Limited public controls | Rechecked continuations |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| structured | 16 | 0 | 24 | 10 | 168 | 6 | 15 | 138/138 | 16 | 10 | 72 |
| brush | 70 | 3 | 96 | 28 | 21 | 6 | 15 | 138/138 | 88 | 13 | 110 |
| tree | 70 | 3 | 96 | 28 | 21 | 6 | 15 | 138/138 | 88 | 13 | 110 |

The 138 required-block denominator uses counted D/U-R/F/U-O requirements,
excluding variants, writers, lifecycle and U-C. Each parser arm still has six
U-C configuration witnesses that read a synthetic protected file; they expose an
open boundary and are not protection successes.

| Arm | Evaluation/render samples | p50 us | p95 us | p99 us | Maximum us |
| --- | ---: | ---: | ---: | ---: | ---: |
| structured | 248 | 262.750 | 463.875 | 785.792 | 2862.667 |
| brush | 248 | 347.875 | 2832.584 | 3940.875 | 4105.834 |
| tree | 248 | 369.334 | 2961.625 | 3946.500 | 4128.375 |

Nearest-rank quantiles include nine variants. Timing excludes fixture setup,
submitted-operation surrogates, continuation rechecks and lifecycle. It includes
real synthetic readlink/stat probes, evaluation and render on a fresh request.
Latency is higher than the R4d sample; this packet claims neither a performance
improvement nor production tail latency, memory acceptance or parser equivalence.
No concurrent construct was added.

The matched-event R4d delta is explicit: the two S17 non-Claude replacement rows
move library N to A in both parser arms while wire stays quiet exit 0. Fifteen
evaluator F rows per parser arm change wire exit 3 to 2. Structured-only has twelve
such wire changes; its three probe-EACCES rows move U-R to F after the independent
cwd check while retaining exit 2. The nine lifecycle
rows leave every evaluator denominator; the old structured lifecycle mix was
six F/three U-R, so subtracting nine F from old totals would be incorrect.

## Requirement-owned differential

All 1,289 overlay rows remain accounted for per arm: 1,265 operations projected
through three consumer adapters (3,795 observations), plus 24 link setup-metadata
checks. Scope is selected before expectation comparison from a frozen Go observer
effective-program inventory and an independent D22/D10/dev/setup program table.
Any modelled effective program makes the row in-slice; Rust parser/shape failure
does not remove it. Empty-program constructs are compared. Only entirely
unmodelled programs receive an outside-program bucket. Scope never reads expected
labels or Rust predicates. The 1,265-row scope inventory SHA-256 is
`a3e41fd04bf1daf575c7312f9a275d298203ac7493228f6517dbcbb1fccd18ac`.

963 operation rows are in-slice; 302 operations (906 consumer observations) are
outside. Link metadata is neither an operation nor an outside verdict denominator.
Historical Codex Read/Grep/Write/Edit projections are offline adapter checks, not
claims of enrolled runtime tools. A quiet Go permission match with Rust U-C does
not establish SupportedPreflight.

| Family | RETAIN match Brush/Tree | CHANGE match | RETAIN conflict Brush/Tree | Outside | Metadata | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| appdata | 115/115 | 4 | 15/15 | 8 | 0 | 142 |
| clients | 0/0 | 0 | 0/0 | 119 | 0 | 119 |
| credentials | 232/230 | 2 | 11/13 | 48 | 0 | 293 |
| options | 10/10 | 0 | 4/4 | 0 | 0 | 14 |
| programs | 38/38 | 0 | 9/9 | 20 | 0 | 67 |
| readers | 112/112 | 0 | 50/50 | 78 | 0 | 240 |
| search | 88/88 | 1 | 8/8 | 5 | 0 | 102 |
| cwd | 41/41 | 0 | 28/28 | 0 | 0 | 69 |
| shell | 137/134 | 8 | 12/15 | 24 | 0 | 181 |
| interpreters | 35/35 | 0 | 3/3 | 0 | 0 | 38 |
| filesystem_link | 0/0 | 0 | 0/0 | 0 | 24 | 24 |
| Total | 808/803 | 15 | 140/145 | 302 | 24 | 1289 |

Consumer-level categories are 2,440/2,425 matching RETAIN, 45/45 matching CHANGE,
404/419 mechanically classified Rust_defect and 906/906 outside (Brush/Tree).
Eight advice conflicts affect Claude only, explaining why defect observations
are not three times conflict rows. All 15 CHANGE rows match class, coverage,
reason/advice, excluded recovery boundary and explicit continuation rechecks.
The frozen overlay remains byte-identical at
`af65478c60c1a997a01d1e0bd4bd00a174c6351a998eb1f62b800a5ccb4a27c4`;
the original Go corpus is byte-identical at
`1e223c6453d6883acc88af9967beab4251ba0fc6d636a1186482b6e4b524c695`.

Go-deny/Rust-quiet-permit means a nonempty original Go denial with Rust N/A/U-C,
including library A with quiet non-Claude delivery. Each arm has 333 such
in-slice observations: 330 Rust_defect and three intended D16 changes at
search[84]. Outside has another 282, for 615 total per arm. R4d had 1,017 Brush
and 1,008 Tree total; the changed scope and implementations prevent attributing
that delta to a single mechanism. These remaining 330 in-slice observations are
not accepted conformance.

Every outside bucket, including zero-escape buckets, is below. Counts are equal
in both arms; observations are three per operation row.

| Unmodelled effective program bucket | Rows | Observations | Go deny / Rust permit |
| --- | ---: | ---: | ---: |
| `[` | 1 | 3 | 0 |
| `aws` | 3 | 9 | 3 |
| `az` | 2 | 6 | 3 |
| `builtin` | 1 | 3 | 0 |
| `chmod` | 1 | 3 | 0 |
| `client` | 8 | 24 | 0 |
| `cmp` | 1 | 3 | 0 |
| `column` | 1 | 3 | 0 |
| `cp` | 9 | 27 | 0 |
| `curl` | 56 | 168 | 75 |
| `date` | 1 | 3 | 0 |
| `dd` | 6 | 18 | 9 |
| `docker` | 40 | 120 | 24 |
| `dotenvx` | 3 | 9 | 0 |
| `ed` | 1 | 3 | 0 |
| `ex` | 1 | 3 | 0 |
| `gcloud` | 2 | 6 | 3 |
| `gh` | 23 | 69 | 39 |
| `glab` | 2 | 6 | 6 |
| `gpg` | 2 | 6 | 3 |
| `hg` | 1 | 3 | 0 |
| `http` | 4 | 12 | 9 |
| `install` | 1 | 3 | 0 |
| `jq` | 6 | 18 | 0 |
| `kubectl` | 4 | 12 | 3 |
| `mv` | 2 | 6 | 0 |
| `mytool` | 2 | 6 | 0 |
| `nocorrect` | 2 | 6 | 0 |
| `npm` | 10 | 30 | 3 |
| `open` | 1 | 3 | 0 |
| `pnpm` | 3 | 9 | 0 |
| `pwd` | 1 | 3 | 0 |
| `rsync` | 8 | 24 | 3 |
| `scp` | 9 | 27 | 6 |
| `security` | 6 | 18 | 15 |
| `sed` | 1 | 3 | 0 |
| `sftp` | 5 | 15 | 9 |
| `sha256sum` | 1 | 3 | 0 |
| `some-command` | 2 | 6 | 0 |
| `ssh` | 21 | 63 | 39 |
| `ssh-add` | 1 | 3 | 0 |
| `ssh-keygen` | 4 | 12 | 3 |
| `stat` | 1 | 3 | 0 |
| `svn` | 1 | 3 | 0 |
| `tac` | 1 | 3 | 0 |
| `tee` | 4 | 12 | 0 |
| `test` | 1 | 3 | 0 |
| `tool` | 1 | 3 | 3 |
| `vim` | 1 | 3 | 0 |
| `wc` | 4 | 12 | 0 |
| `wget` | 24 | 72 | 21 |
| `yq` | 3 | 9 | 0 |
| `zgrep` | 1 | 3 | 0 |
| `zip` | 1 | 3 | 3 |
| Total | 302 | 906 | 282 |

Tree has five additional in-slice conflict rows: credentials[199] wrongly turns
env plus a redirected child into a dump; credentials[200] denies for that wrong
owner; shell[76-78] return blocking U-R on complex heredocs where Brush preserves
the protected operand and returns D. Three outside rows also differ by arm
(appdata[60], shell[73-74], Brush U-C versus Tree U-R). Their scope stays outside;
the inventories preserve both observations. The five in-slice differences add
15 defect observations but do not change the Go-deny/Rust-permit total.

## Causal ablations

All 60 entries below have a compiled named-test failure (101), exact-byte
SHA-256 restoration, and the same named test passing (0). The receipt includes
the original/restored byte hashes; the aggregate also hashes each raw failure and
pass log. No entry relies on a compile failure or safety exemption. Removing the
recovery boundary deletes its exclusion, rather than changing wording. Private
probe negatives are intercepted at the RecordingProbe boundary before any real
protected access. Ablations were performed at their reviewable implementation
steps; their restoration hashes need not equal later legitimate source changes.

| Mechanism ablation ID | Observed failing test | Break / restored |
| --- | --- | --- |
| f-wire | `review_b1_m6_failures_block_from_consumer_wire` | 101 / 0; exact bytes |
| product-gate | `gate_a_zero_starts_and_bypass_negative` | 101 / 0; exact bytes |
| brush-tree-words | `review_shell` | 101 / 0; exact bytes |
| loop-union | `review_shell` | 101 / 0; exact bytes |
| read-write-brush | `review_shell` | 101 / 0; exact bytes |
| read-write-tree | `review_shell` | 101 / 0; exact bytes |
| statement-boundaries | `review_shell` | 101 / 0; exact bytes |
| continued-line-tree | `review_shell` | 101 / 0; exact bytes |
| shell-code-flags | `review_program` | 101 / 0; exact bytes |
| interpreter-code-flags | `review_program` | 101 / 0; exact bytes |
| grep-option-table | `review_program` | 101 / 0; exact bytes |
| wrapper-options | `review_program` | 101 / 0; exact bytes |
| secret-variables | `review_program` | 101 / 0; exact bytes |
| shell-glob-reach | `review_identity` | 101 / 0; exact bytes |
| glob-group-union | `review_identity` | 101 / 0; exact bytes |
| ssh-public-class | `review_identity` | 101 / 0; exact bytes |
| non-utf8-bound | `review_identity` | 101 / 0; exact bytes |
| resolved-home | `review_identity` | 101 / 0; exact bytes |
| listing-roles | `review_program` | 101 / 0; exact bytes |
| tar-roles | `review_program` | 101 / 0; exact bytes |
| git-file-roles | `command_boundaries_preserve_protected_operands` | 101 / 0; exact bytes |
| cd-state | `review_program` | 101 / 0; exact bytes |
| no-oracle-recovery | `review_metadata` | 101 / 0; exact bytes |
| recovery-boundary-removal | `identity_depth_bound_is_not_syntax_or_success` | 101 / 0; exact bytes |
| detector-record-recovery | `review_metadata` | 101 / 0; exact bytes |
| event-cwd-validation | `review_metadata` | 101 / 0; exact bytes |
| exec-workdir-precedence | `review_metadata` | 101 / 0; exact bytes |
| consumer-independent-advice | `review_metadata` | 101 / 0; exact bytes |
| resolved-cwd | `review_metadata` | 101 / 0; exact bytes |
| home-reason | `review_metadata` | 101 / 0; exact bytes |
| ur-kind-message | `review_metadata` | 101 / 0; exact bytes |
| effect-records | `dev_contract` | 101 / 0; exact bytes |
| nested-provenance | `dev_contract` | 101 / 0; exact bytes |
| tool-class-assertion | `outside_tool_class_rejects_a_different_tool` | 101 / 0; exact bytes |
| effect-contract-assertion | `reason_and_effect_partitions_reject_missing_evidence` | 101 / 0; exact bytes |
| lifecycle-denominator | `lifecycle_harness_only` | 101 / 0; exact bytes |
| writer-write-fault | `owned_writer_states` | 101 / 0; exact bytes |
| raw-parent-paths | `readlink_parents_and_firmlink_spelling` | 101 / 0; exact bytes |
| prefix-normalization | `readlink_parents_and_firmlink_spelling` | 101 / 0; exact bytes |
| firmlink-case | `readlink_parents_and_firmlink_spelling` | 101 / 0; exact bytes |
| resolved-ssh-root | `readlink_parents_and_firmlink_spelling` | 101 / 0; exact bytes |
| du-values | `listing_option_values_and_children` | 101 / 0; exact bytes |
| fd-directory-options | `listing_option_values_and_children` | 101 / 0; exact bytes |
| listing-children | `listing_option_values_and_children` | 101 / 0; exact bytes |
| versioned-interpreters | `grep_ag_and_versioned_interpreter_roles` | 101 / 0; exact bytes |
| ag-hidden | `grep_ag_and_versioned_interpreter_roles` | 101 / 0; exact bytes |
| recursive-grep-value | `grep_ag_and_versioned_interpreter_roles` | 101 / 0; exact bytes |
| broad-owner-reason | `grep_ag_and_versioned_interpreter_roles` | 101 / 0; exact bytes |
| wrapper-cwd-targets | `readlink_parents_and_firmlink_spelling` | 101 / 0; exact bytes |
| ssh-root-inode | `resolved_ssh_root_and_parent_inode_identity` | 101 / 0; exact bytes |
| ssh-parent-inode | `resolved_ssh_root_and_parent_inode_identity` | 101 / 0; exact bytes |
| ssh-public-file-type | `ssh_public_file_type_and_probe_fault` | 101 / 0; exact bytes |
| ssh-stat-fault | `ssh_public_file_type_and_probe_fault` | 101 / 0; exact bytes |
| ssh-lexical-stop | `private_key_spelling_stops_before_all_probes` | 101 / 0; exact bytes |
| ssh-broad-priority | `broad_root_recovery_preserves_excluded_scope` | 101 / 0; exact bytes |
| ssh-protected-root-scope | `protected_ssh_root_does_not_deny_unrelated_public_targets` | 101 / 0; exact bytes |
| requirement-owned-scope | `listing_option_values_and_children` | 101 / 0; exact bytes |
| ssh-search-parent-inode | `search_compares_resolved_ssh_root_parents` | 101 / 0; exact bytes |
| interpreter-env-option-role | `interpreter_environment_options_use_paths_without_extracting_contents` | 101 / 0; exact bytes |
| ssh-public-spelling-case | `private_key_spelling_stops_before_all_probes` | 101 / 0; exact bytes |

## Remaining failures and decision conflicts

The expanded differential intentionally fails on every in-slice disagreement;
no implementation-shaped exclusion, baseline-defect exception or Rust-derived
manifest label suppresses it. Representative open failures are lexical user
tilde expansion (appdata[85]), unresolved parameter fragments (appdata[132]),
ANSI-C/brace/parameter expansion, command-local/exported bindings, complex cwd
and physical path state, streams/heredocs/static code producers, further wrapper,
Git/tar/reader option roles, credential directory/glob classes and workflow advice
beyond the replacement rule. The per-row inventories retain complete source,
scope, three observations, wire, reasons, recovery and mismatch flags.

Go IsAppdata is HOME-scoped, as is Rust: these rows do not justify global Library
protection. Go ExpandHome uses host USER (the Go corpus sets fixture-user);
Rust has not ported that user-tilde owner. ROOT is an unresolved shell parameter,
not a host fact supplied by the Rust example; its exact unresolved-fragment
semantics remain a shell/target gap. shell[175] currently returns ProbeFault F
for a long git commit message inferred as a path, while Go permits; it is not an
input ResourceLimit result or a resolved decision exception.

Two decision/overlay classes remain visible rather than silently changing labels:
credentials[172] has empty cwd and now returns MalformedInput F as D22 requires,
while RETAIN asks for Go D. D17's any-process protected-cwd ruling also requires D
for frozen Go-permit rows such as programs[28], programs[36] and programs[39].
They still appear in the mechanical Rust_defect count until the coordinator
adjudicates their overlay. Other Git role disagreements, including programs[29]
and programs[44], are not treated as protected-cwd exceptions without that evidence.
D23 supersedes D6's SSH identity deferral and now includes the allowed metadata
root/parent comparisons; no unresolved D23 permission question remains.

Six U-C dynamic-configuration witnesses per parser arm remain an explicit
protection gap. Full shell/program equivalence, installed protocol and hook
loading, real consumer Gate A, execution Gate B, verified execution owners,
arbitrary-writer preservation, performance acceptance and Go cutover remain open.
The packaged binary and production Go/hook paths are unchanged. No installation,
registration, permission/TCC change, real-secret/App Data experiment, remote write,
release or holdout inspection was performed.

## Evidence provenance and frozen limits

Raw artifacts are under `~/.cache/guard-fixtures/fixture-seat/`. r4g-measure.py and
r4g-measurement-binding.json bind all three 248-row arm logs and both 1,289-row
differentials. r4g-summary.json/r4g-summarize.py reproduce the tables and validate
every source/raw hash; r4g-mismatch-inventory-brush/tree.jsonl retain all conflicts,
outside rows and intended changes. r4g-deltas.json retains every matched class/wire
delta from R4d. The final-delivery binding records actual HEAD, ancestry, unchanged
measurement inputs, documentation hash and protected-plan preservation.

r4g-baseline-named-tests.log contains the 1f9fbb2 failures and already-passing
controls; r4g-writer-strength-before.log, r4g-conditional-strength-before.log and
r4g-lifecycle-strength-before.log retain old-harness discrimination failures.
r4g-other-tests-final.log retains
the current passes, lifecycle receipts and private-candidate probe lists.
r4g-before/after-{requests,responses,regression}.jsonl and r4g-cli-after.log retain
the independent 86-case JSONL checks; r4g-ssh-before.log and r4g-ssh-cli outputs
retain D23 before/after evidence. r4g-ablation-summary.jsonl and
r4g-ablate-<id>-fail/pass.log retain the 60 fail/restoration/pass pairs.
r4g-rust-check-final.log retains the failed required gate;
r4g-cargo-deny-final.log and r4g-release-build.log retain separate successful
dependency/release checks. r4g-initial-9cb1e3f-* preserves the earlier source
measurement; all prior R4b/R4d artifacts remain historical, not current passes.

The 65,536-byte event and 64-delimiter limits remain frozen in 021ad5d6.
docs/rust-slice-limits.md preserves the public-only 90-observation sweep, exact event-envelope
boundary, twelve S20 labels and conservative selection. No new calibration,
threshold change or production performance claim is made in R4g.
