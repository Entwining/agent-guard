use super::*;

#[test]
fn unknown_defaults_use_one_context_as_parameter_count_grows() {
    for width in [1, 2, 4, 8] {
        let mut output = Observation::default();
        let host = crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
            config_dirs: &[],
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
                vec![statements::BindingValue::RuntimeUnknown(Some(
                    format!("$unknown{index}").into(),
                ))],
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
fn presence_assumptions_add_one_context_each_as_parameter_count_grows() {
    for width in [1, 2, 4, 8, 16] {
        // Unbound names, and names a conditional assignment may leave inherited.
        for conditional in [false, true] {
            let mut output = Observation::default();
            let host = crate::record::HostFacts {
                home: "/synthetic/home",
                user: None,
                config_dirs: &[],
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
                if conditional {
                    scope.assign(
                        format!("p{index}"),
                        vec![
                            statements::BindingValue::Known("x".into()),
                            statements::BindingValue::Inherited,
                        ],
                    );
                }
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
            let texts = values
                .iter()
                .map(|value| value.word.text.as_str())
                .collect::<Vec<_>>();
            assert!(texts.contains(&"run".repeat(width).as_str()), "{texts:?}");
            if conditional {
                assert!(texts.contains(&"x".repeat(width).as_str()), "{texts:?}");
            }
            let readings = if conditional { 3 } else { 2 };
            assert!(
                output.context_delta_entries <= readings * width,
                "parameters={width} conditional={conditional} entries={}",
                output.context_delta_entries
            );
            assert!(!output.gaps.contains(&CoverageGap::InspectionBudget));
        }
    }
}

#[test]
fn opaque_default_representatives_do_not_split_into_known_runtime_fields() {
    let mut output = Observation::default();
    let host = crate::record::HostFacts {
        home: "/synthetic/home",
        user: None,
        config_dirs: &[],
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
            String::from("$unknown").into(),
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
    let fields = values
        .iter()
        .map(|value| {
            value
                .split
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(fields.contains(&vec!["public other"]), "{fields:?}");
    assert!(fields.contains(&vec!["public", "other"]), "{fields:?}");
    // The empty branch is an assumption about the runtime value.
    assert!(
        values
            .iter()
            .flat_map(|value| &value.split)
            .all(|word| word.exact().is_none())
    );
}
