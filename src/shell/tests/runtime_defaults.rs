use super::*;

#[test]
fn unknown_defaults_use_one_context_as_parameter_count_grows() {
    for width in [1, 2, 4, 8] {
        let mut output = Observation::default();
        let host = crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        };
        let mut evaluator = statements::Evaluator::new(
            Frontend {
                arm: Arm::Brush,
                zsh: false,
                host,
            },
            &mut output,
        );
        let mut scope = statements::Scope::new(host.home, "/synthetic/project");
        let mut raw = String::new();
        for index in 0..width {
            scope.assign(
                format!("p{index}"),
                vec![statements::BindingValue::RuntimeUnknown(Some(format!(
                    "$unknown{index}"
                )))],
            );
            raw.push_str(&format!("${{p{index}:-run}}"));
        }
        let values = evaluator
            .expand(
                &RawWord {
                    raw,
                    syntax: WordSyntax::Shell,
                    expansions: Vec::new(),
                },
                &mut scope,
                0,
            )
            .unwrap();
        assert_eq!(values.len(), 1, "parameters={width}");
        assert!(values[0].word.runtime_unknown);
        assert!(values[0].word.expands);
        assert!(values[0].unknown_splitting);
        assert_eq!(values[0].word.text, "run".repeat(width));
        assert_eq!(output.context_delta_entries, width);
        assert_eq!(output.word_candidates_max, 1);
        assert!(!output.gaps.contains(&CoverageGap::InspectionBudget));
    }
}

#[test]
fn opaque_default_representatives_do_not_split_into_known_runtime_fields() {
    let mut output = Observation::default();
    let host = crate::record::HostFacts {
        home: "/synthetic/home",
        user: None,
    };
    let mut evaluator = statements::Evaluator::new(
        Frontend {
            arm: Arm::Brush,
            zsh: false,
            host,
        },
        &mut output,
    );
    let mut scope = statements::Scope::new(host.home, "/synthetic/project");
    scope.assign(
        "p".into(),
        vec![statements::BindingValue::RuntimeUnknown(Some(
            "$unknown".into(),
        ))],
    );
    let values = evaluator
        .expand(
            &RawWord {
                raw: "${p:-public other}".into(),
                syntax: WordSyntax::Shell,
                expansions: Vec::new(),
            },
            &mut scope,
            0,
        )
        .unwrap();
    let opaque = values
        .iter()
        .find(|value| value.word.runtime_unknown)
        .unwrap();
    assert_eq!(opaque.split.len(), 1);
    assert_eq!(opaque.split[0].text, "public other");
    let empty = values
        .iter()
        .find(|value| !value.word.runtime_unknown)
        .unwrap();
    assert_eq!(
        empty
            .split
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        ["public", "other"]
    );
}
