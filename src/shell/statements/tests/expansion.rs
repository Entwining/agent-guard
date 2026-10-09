use super::*;
fn expand_candidates(
    values: Vec<BindingValue>,
    expansions: Vec<crate::shell::RawExpansion>,
    depth: usize,
) -> (Result<Vec<Expanded>, CheckError>, Observation) {
    let mut scope = Scope::new("/h", "/p");
    scope.assign("v".into(), values);
    let raw = RawWord {
        raw: "$v".into(),
        syntax: WordSyntax::Shell,
        expansions,
    };
    let mut output = Observation::default();
    let result = {
        let mut evaluator = Evaluator::new(
            Frontend {
                arm: Arm::Brush,
                zsh: true,
                host: HostFacts {
                    home: "/h",
                    user: None,
                },
            },
            &mut output,
        );
        crate::shell::expand_scoped(&raw, &mut scope, &mut evaluator, depth)
    };
    (result, output)
}

#[test]
fn empty_runtime_candidate_survives_without_erasing_known_alternatives() {
    let (sole, _) = expand_candidates(
        vec![BindingValue::RuntimeUnknown(Some(String::new()))],
        Vec::new(),
        0,
    );
    let sole = sole.unwrap();
    assert_eq!(sole.len(), 1);
    assert!(sole[0].word.runtime_unknown);
    assert!(sole[0].word.text.is_empty());
    let (mixed, _) = expand_candidates(
        vec![
            BindingValue::RuntimeUnknown(Some(String::new())),
            BindingValue::Known("public".into()),
        ],
        Vec::new(),
        0,
    );
    assert!(
        mixed
            .unwrap()
            .iter()
            .any(|expanded| expanded.word.text == "public")
    );
}

#[test]
fn runtime_absence_and_undetermined_binding_have_distinct_coverage() {
    for (value, unsupported) in [
        (BindingValue::RuntimeUnknown(None), false),
        (BindingValue::Undetermined, true),
    ] {
        let (expanded, output) = expand_candidates(vec![value], Vec::new(), 0);
        assert!(!expanded.unwrap().is_empty());
        assert_eq!(
            output.gaps.contains(&CoverageGap::UnsupportedShellSyntax),
            unsupported
        );
    }
}

#[test]
fn independently_detected_code_obeys_the_source_recursion_frontier() {
    let mut output = Observation::default();
    let mut scope = Scope::new("/h", "/p");
    let mut evaluator = Evaluator::new(
        Frontend {
            arm: Arm::Brush,
            zsh: true,
            host: HostFacts {
                home: "/h",
                user: None,
            },
        },
        &mut output,
    );
    assert_eq!(
        evaluator
            .source("if then; cat =(true)", &mut scope, 64)
            .unwrap_err()
            .kind,
        crate::CheckErrorKind::ResourceLimit
    );
}

#[test]
fn forwarded_code_and_variable_bodies_obey_the_recursive_frontier() {
    for expansion in [
        crate::shell::RawExpansion::Code("true".into()),
        crate::shell::RawExpansion::Variable("v".into()),
    ] {
        let (result, _) = expand_candidates(
            vec![BindingValue::Known("true".into())],
            vec![expansion],
            64,
        );
        assert!(
            matches!(
                result,
                Err(CheckError {
                    kind: crate::CheckErrorKind::ResourceLimit
                })
            ),
            "forwarded body crossed its recursion frontier"
        );
    }
    for source in ["cat =(cat .env)", "v='cat .env'; echo ${(e)v}"] {
        let output = observation(source, &mut Scope::new("/h", "/p"));
        let protected: Vec<_> = output
            .script
            .commands
            .iter()
            .filter(|c| {
                c.argv.iter().map(|w| w.text.as_str()).collect::<Vec<_>>() == ["cat", ".env"]
            })
            .collect();
        assert!(!protected.is_empty(), "forwarded child body was lost");
        assert!(
            protected.iter().all(|command| command.nested),
            "forwarded body acquired a top-level copy"
        );
    }
}

#[test]
fn assigned_pwd_does_not_duplicate_non_tilde_words() {
    let mut scope = Scope::new("/h", "/p");
    scope.assign("PWD".into(), vec![BindingValue::Known("/assigned".into())]);
    let raw = RawWord {
        raw: "public".into(),
        syntax: WordSyntax::Shell,
        expansions: Vec::new(),
    };
    let mut output = Observation::default();
    let mut evaluator = Evaluator::new(
        Frontend {
            arm: Arm::Brush,
            zsh: true,
            host: HostFacts {
                home: "/h",
                user: None,
            },
        },
        &mut output,
    );
    assert_eq!(
        crate::shell::expand_scoped(&raw, &mut scope, &mut evaluator, 0)
            .unwrap()
            .len(),
        1
    );
    let output = observation(
        "PWD=/assigned; printf '%s' a b c d e f g h i j; printf '%s' ~+",
        &mut Scope::new("/h", "/p"),
    );
    assert!(!output.gaps.contains(&CoverageGap::InspectionBudget));
    assert!(
        output
            .script
            .commands
            .iter()
            .any(|c| c.argv.iter().any(|w| w.text == "/assigned"))
    );
    assert!(
        output
            .script
            .commands
            .iter()
            .any(|c| c.argv.iter().any(|w| w.text == "/p"))
    );
}
