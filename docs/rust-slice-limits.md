# Offline Rust slice limits

The P1 trial accepts at most 65,536 bytes in the serialized consumer event and
64 simultaneously nested shell substitution/group delimiters. Larger values
permit more work before rejection. Excess input fails with `ResourceLimit`
before parser or filesystem-probe entry; it is never an empty successful check.
These limits concern offline preflight, not installed hooks or execution isolation.

The values were selected before implementing the evaluator or examining holdout.
The release calibration example generated only a public `cat` operand with
whitespace padding or nested public substitutions. Five independent child runs
at each of nine byte sizes (256 through 1,048,576) and nine nesting depths
(1 through 256) produced 90 successful observations with both pinned parsers.
Each child had a one-second deadline with termination and waiting on timeout.

At 65,536 command bytes, the maximum JSON/brush/tree-sitter times were
21/743/2,192 microseconds and child wall time was 7.855 milliseconds. At
64 substitutions, the maximum parser times were 55/98 microseconds and wall
time was 2.412 milliseconds. All five samples at 1,048,576 bytes and depth 256
also passed. The smaller supported bounds leave room for recursive observation,
identity probes and rendering within the proposed three-second total budget.
The first 256-byte child took 604.317 milliseconds, included without filtering.
This small public sweep does not establish production tail latency or memory use.

The raw sweep is `~/.cache/guard-fixtures/fixture-seat/r4b-calibration.jsonl`, generated
by `r4b-calibration.py` in the same task directory using `examples/calibrate.rs`.
Input-byte boundary recipes materialize the complete serialized event, including
its envelope, at the bound and one byte above it. Nesting recipes materialize
64 and 65 substitutions. Both paired cases must assert parser/probe entry counts
and the semantic public result or the exact failure. Subsequent development
work may not tune these values against held-out results.
