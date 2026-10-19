use agent_guard_rust::{
    Advice, CheckError, CheckErrorKind, Coverage, CoverageGap, DenialRule, Disposition, Evaluation,
    Outcome, Reason, Recovery, RecoveryStep,
    adapters::{Consumer, render},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Lists every variant of an enum with its name. The generated match has no
/// wildcard, so adding a variant stops this test from compiling until the
/// variant is listed here and given a fixture row.
macro_rules! every_variant {
    ($kind:ident: $($variant:ident $({ $($field:ident: $value:expr),* })?),* $(,)?) => {{
        fn _exhaustive(value: &$kind) {
            match value {
                $($kind::$variant { .. } => {})*
            }
        }
        vec![$((stringify!($variant), $kind::$variant $({ $($field: $value),* })?)),*]
    }};
}

fn evaluation(outcome: Outcome) -> Result<Evaluation, CheckError> {
    Ok(Evaluation {
        outcome,
        coverage: Coverage::SupportedPreflight,
        effects: Vec::new(),
    })
}

fn refusal(cause: CoverageGap, disposition: Disposition) -> Result<Evaluation, CheckError> {
    evaluation(Outcome::CoverageInsufficient {
        cause,
        disposition,
        recovery: None,
    })
}

/// Every blocking result the adapter can render, keyed by fixture class and kind.
fn blocking_results() -> Vec<(&'static str, &'static str, Result<Evaluation, CheckError>)> {
    use CheckErrorKind as E;
    use CoverageGap as G;
    use DenialRule as D;
    let rules = every_variant!(D: ResourceChange, AppData, Broad, File, Store, CodeFile, HiddenSearch,
        Dump, Variable, Token, Keychain, StoredSecret, Trace, Upload, Ssh, GrepSsh);
    let gaps = every_variant!(G: InodeAlias, InputByteLimit, NestingLimit, AbsoluteCwdRequired,
        InvalidEncoding, UnknownProgram { program: String::new() }, UnresolvedTarget,
        UnsupportedShellSyntax, ExecutorDivergence, OutsideObservedTool { tool: String::new() },
        ExecutionOwnerUnavailable, IdentityBound, InspectionBudget, InterpreterChosenRead,
        UnsupportedDialectConstruct);
    let errors = every_variant!(E: RelativeCwd, InvalidEncoding, MalformedInput, InputFailure,
        GuardFault, ProbeFault, ResourceLimit, InspectionBudget, Deadline);
    let denial = |rule| {
        evaluation(Outcome::ProtectedDenial {
            reason: Reason {
                effect: String::new(),
                rule,
            },
            recovery: Recovery {
                next_step: RecoveryStep::RecheckOperation {
                    description: String::new(),
                },
                preserved_scope: Vec::new(),
                excluded_scope: Vec::new(),
                automatic_application_supported: false,
            },
        })
    };
    let rules = rules
        .into_iter()
        .map(|(kind, rule)| ("policy", kind, denial(rule)));
    let gaps = gaps.into_iter().map(|(kind, cause)| {
        let result = refusal(cause, Disposition::RejectUnsupportedSyntax);
        ("coverage", kind, result)
    });
    let errors = errors
        .into_iter()
        .map(|(kind, kind_value)| ("error", kind, Err(CheckError { kind: kind_value })));
    rules.chain(gaps).chain(errors).collect()
}

fn marker(stderr: &str) -> (&str, &str, &str) {
    let (marker, reason) = stderr
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
        .unwrap_or_else(|| panic!("no leading reason marker: {stderr:?}"));
    let (class, id) = marker
        .split_once(":agent-guard/")
        .unwrap_or_else(|| panic!("marker without the agent-guard producer: {marker:?}"));
    (class, id, reason)
}

fn valid_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/rust-reason-markers.json")).unwrap()
}

fn str_field<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap()
}

#[test]
fn every_blocking_reason_leads_with_its_fixture_marker() {
    let fixture = fixture();
    let rows: BTreeMap<(&str, &str), &str> = fixture["rendered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                (str_field(row, "class"), str_field(row, "kind")),
                str_field(row, "id"),
            )
        })
        .collect();
    let results = blocking_results();
    assert_eq!(
        rows.len(),
        fixture["rendered"].as_array().unwrap().len(),
        "a kind is listed twice"
    );
    assert_eq!(
        rows.keys().copied().collect::<BTreeSet<_>>(),
        results
            .iter()
            .map(|(class, kind, _)| (*class, *kind))
            .collect(),
        "each variant needs exactly one fixture row"
    );
    // Kinds may share an ID only when they block with the same reason and recovery.
    let mut reasons: BTreeMap<(u8, &str, &str), String> = BTreeMap::new();
    for (class, kind, result) in &results {
        let id = rows[&(*class, *kind)];
        assert!(valid_id(id), "{id}");
        for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
            let wire = render(consumer, result);
            assert_eq!(wire.exit, 2, "{consumer:?} {kind}");
            assert!(wire.stdout.is_empty(), "{consumer:?} {kind}");
            let (got_class, got_id, reason) = marker(&wire.stderr);
            assert_eq!((got_class, got_id), (*class, id), "{consumer:?} {kind}");
            assert_eq!(
                reason.starts_with("DENIED: "),
                consumer == Consumer::Claude,
                "{consumer:?} {kind}"
            );
            let shared = reasons
                .entry((consumer as u8, class, id))
                .or_insert_with(|| wire.stderr.clone());
            assert_eq!(*shared, wire.stderr, "{class}:{id} names different reasons");
        }
    }
}

#[test]
fn proceeding_outcomes_carry_no_marker() {
    for consumer in [Consumer::Claude, Consumer::Codex, Consumer::Pi] {
        for result in [
            evaluation(Outcome::NoObjection),
            evaluation(Outcome::SoftAdvice(vec![Advice::RgInclude])),
            refusal(
                CoverageGap::UnresolvedTarget,
                Disposition::ContinueLimitedPreflight,
            ),
        ] {
            let wire = render(consumer, &result);
            assert_eq!(wire.exit, 0, "{consumer:?}");
            assert!(wire.stderr.is_empty(), "{consumer:?}");
            assert!(!wire.stdout.contains(":agent-guard/"), "{consumer:?}");
        }
    }
}

/// `entry` is false for kinds the native entry never renders: non-blocking
/// gaps proceed, `evaluate_native` turns relative cwd, invalid encoding and
/// resource limits into coverage refusals, policy turns an inspection-budget
/// error into a refusal, the entry never requires an execution owner, and no
/// entry path produces the remaining error kinds.
#[test]
fn setup_table_lists_exactly_the_markers_the_entry_emits() {
    let fixture = fixture();
    let mut emitted: BTreeSet<(String, String)> = fixture["rendered"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["entry"] == true)
        .map(|row| (str_field(row, "id").into(), str_field(row, "class").into()))
        .collect();
    for list in ["checker", "shell_entry", "pi_extension"] {
        for row in fixture[list].as_array().unwrap() {
            let id = str_field(row, "id");
            assert!(valid_id(id), "{id}");
            assert!(
                emitted.insert((id.to_owned(), "error".into())),
                "error:{id} is listed twice"
            );
        }
    }
    let setup = include_str!("../../docs/setup.md");
    let section = setup
        .split_once("\n## Reason markers\n")
        .and_then(|(_, rest)| rest.split("\n## ").next())
        .unwrap();
    let mut documented = BTreeSet::new();
    for line in section.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let id = cells[1].trim_matches('`').to_owned();
        assert!(
            documented.insert((id, cells[2].to_owned())),
            "duplicate table row {line}"
        );
    }
    assert_eq!(documented, emitted);
}
