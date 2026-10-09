use super::*;
#[test]
fn binding_join_replaces_the_complete_exit_state() {
    let mut outer = Scope::new("/h", "/p");
    let exit = outer.clone();
    outer.assign("D".into(), vec![BindingValue::Known("public".into())]);
    assert!(!outer.join(&[exit]));
    assert!(!outer.contexts().contains_key("D"));
}

#[test]
fn function_replay_has_its_own_work_bound() {
    let data = fixture();
    let result = observation(
        data["sources"]["function_runs"].as_str().unwrap(),
        &mut seeded_scope(),
    );
    assert!(result.script.commands.len() < 512);
    assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
}

#[test]
fn recursive_function_does_not_expand_to_the_depth_bound() {
    let data = fixture();
    let result = observation(
        data["sources"]["recursive"].as_str().unwrap(),
        &mut Scope::new("/h", "/p"),
    );
    assert!(result.script.commands.len() <= 3);
    assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
}

#[test]
fn word_context_product_has_a_finite_bound() {
    let data = fixture();
    let raw = RawWord {
        raw: data["sources"]["contexts"].as_str().unwrap().into(),
        syntax: WordSyntax::Shell,
        expansions: Vec::new(),
    };
    let mut scope = seeded_scope();
    let mut output = Observation::default();
    let frontend = Frontend {
        arm: Arm::Brush,
        zsh: true,
        host: HostFacts {
            home: "/h",
            user: None,
        },
    };
    let expanded = Evaluator::new(frontend, &mut output)
        .expand(&raw, &mut scope, 0)
        .unwrap();
    assert_eq!(expanded.len(), 512);
    assert!(output.gaps.contains(&CoverageGap::InspectionBudget));
}

#[test]
fn command_argv_product_has_a_finite_bound() {
    let data = fixture();
    let result = observation(
        data["sources"]["argv"].as_str().unwrap(),
        &mut seeded_scope(),
    );
    assert_eq!(result.script.commands.len(), 512);
    assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
}

#[test]
fn statement_work_bound_applies_without_emitted_commands() {
    let data = fixture();
    let source = data["sources"]["statement"].as_str().unwrap().repeat(600);
    let result = observation(&source, &mut Scope::new("/h", "/p"));
    assert!(result.script.commands.is_empty());
    assert!(result.gaps.contains(&CoverageGap::InspectionBudget));
}

#[test]
fn binding_values_and_exit_snapshots_are_bounded() {
    let mut outer = Scope::new("/h", "/p");
    Rc::make_mut(&mut outer.loops).push(Rc::default());
    let branches = (0..513)
        .map(|n| {
            let mut branch = outer.clone();
            branch.assign("A".into(), vec![BindingValue::Known(n.to_string())]);
            let state = branch.state();
            Rc::make_mut(&mut branch.returns).push(state.clone());
            Rc::make_mut(&mut Rc::make_mut(&mut branch.loops)[0]).push(state);
            branch
        })
        .collect::<Vec<_>>();
    assert!(outer.join(&branches));
    assert_eq!(outer.bindings["A"].values.len(), 512);
    assert_eq!(outer.returns.len(), 512);
    assert_eq!(outer.loops[0].len(), 512);
}
