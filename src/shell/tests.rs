use super::*;
mod runtime_defaults;
#[test]
fn variable_and_named_directory_candidates_share_one_word_budget() {
    let mut source = String::from("case public in ");
    for n in 0..32 {
        source.push_str(&format!("a{n}) D=/public/{n}; hash -d Q=/public/{n};; "));
    }
    source.push_str("esac; cat ~Q/$D");
    let output = observe(&source, Arm::Brush, "/h", "/h/p", true).unwrap();
    assert!(output.gaps.contains(&CoverageGap::InspectionBudget));
    assert_eq!(output.word_candidates_max, 512);
}
#[test]
fn identical_sources_share_syntax_but_observe_each_context() {
    for width in [2, 4, 8, 16] {
        let mut output = Observation::default();
        let host = crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        };
        let mut evaluator = statements::Evaluator::new(
            Frontend {
                arm: Arm::Brush,
                zsh: true,
                host,
            },
            &mut output,
        );
        for index in 0..width {
            let mut scope = statements::Scope::new(host.home, &format!("/synthetic/dir{index}"));
            scope.assign(
                "file".into(),
                vec![statements::BindingValue::Known(format!("public{index}"))],
            );
            evaluator
                .source("cat \"$file\"", &mut scope, index % 2)
                .unwrap();
        }
        assert_eq!(output.parse_builds, 1, "width={width}");
        assert_eq!(output.source_entries, width);
        assert_eq!(output.script.commands.len(), width);
        for (index, command) in output.script.commands.iter().enumerate() {
            assert_eq!(command.argv[1].text, format!("public{index}"));
            assert_eq!(command.cwd, format!("/synthetic/dir{index}"));
            assert_eq!(command.nested, index % 2 == 1);
        }
    }
}

#[test]
fn alternative_environments_store_only_binding_deltas() {
    for width in [4, 16, 64, 256] {
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
        for index in 0..width {
            scope.assign(
                format!("ambient{index}"),
                vec![statements::BindingValue::Known("unchanged".into())],
            );
        }
        scope.assign(
            "file".into(),
            vec![
                statements::BindingValue::Known("public".into()),
                statements::BindingValue::RuntimeUnknown(Some("public".into())),
                statements::BindingValue::Known("protected".into()),
            ],
        );
        let values = evaluator
            .expand(
                &RawWord {
                    raw: "\"$file\"".into(),
                    syntax: WordSyntax::Shell,
                    expansions: Vec::new(),
                },
                &mut scope,
                0,
            )
            .unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].word.text, "public");
        assert!(values[0].word.runtime_unknown);
        assert_eq!(values[1].word.text, "protected");
        assert!(!values[1].word.runtime_unknown);
        assert_eq!(output.context_delta_entries, 2, "ambient width={width}");
    }
}

#[test]
fn syntax_cache_hits_keep_depth_deadline_and_failure_checks() {
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
    evaluator.source("printf public", &mut scope, 0).unwrap();
    assert_eq!(
        evaluator
            .source("printf public", &mut scope, MAX_NESTING + 1)
            .unwrap_err()
            .kind,
        CheckErrorKind::ResourceLimit
    );
    evaluator.deadline = Some(std::time::Instant::now());
    assert_eq!(
        evaluator
            .source("printf public", &mut scope, 0)
            .unwrap_err()
            .kind,
        CheckErrorKind::Deadline
    );
    evaluator.deadline = None;
    for _ in 0..2 {
        evaluator.source("if", &mut scope, 0).unwrap();
    }
    assert_eq!(output.parse_builds, 2);
    assert_eq!(output.parse_failures, 2);
    assert!(output.script.parse_failed);
    assert!(output.gaps.contains(&CoverageGap::UnsupportedShellSyntax));
}

#[test]
fn quoted_star_preserves_unknown_argument_metadata() {
    let observation = observe(
        "f() { cat \"$*\"; }; f $value public",
        Arm::Brush,
        "/synthetic/home",
        "/synthetic/project",
        true,
    )
    .unwrap();
    assert!(observation.script.commands.iter().any(|command| {
        command.argv.first().is_some_and(|word| word == "cat")
            && command.argv.get(1).is_some_and(|word| {
                word.ends_with("public") && word.runtime_unknown && word.vars.contains(&"1".into())
            })
    }));
}
#[test]
fn complete_alternative_argv() {
    assert!(
        !ACCEPTANCE_ARMS.is_empty(),
        "missing acceptance parser arms"
    );
    for &arm in ACCEPTANCE_ARMS {
        let obs = observe("p='public protected'; cat $p", arm, "/h", "/h/p", true).unwrap();
        assert!(
            obs.script
                .commands
                .iter()
                .any(|c| c.argv == ["cat", "public", "protected"])
        );
        assert!(
            obs.script
                .commands
                .iter()
                .any(|c| c.argv == ["cat", "public protected"])
        );
    }
}
