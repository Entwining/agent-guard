use super::*;

fn seeded(width: usize) -> (Scope, EntryCopies) {
    let mut scope = Scope::new("/synthetic/home", "/synthetic/project");
    for index in 0..width {
        scope.assign(
            format!("state{index}"),
            vec![BindingValue::Known("public".repeat(16))],
        );
    }
    let counter = EntryCopies::default();
    for binding in Rc::make_mut(&mut scope.bindings).values_mut() {
        binding.copies = counter.clone();
    }
    scope.enter_function();
    scope.local("state0");
    let state = scope.state();
    Rc::make_mut(&mut scope.returns).push(state);
    let state = scope.state();
    Rc::make_mut(&mut scope.loops).push(Rc::new(vec![state]));
    counter.0.set(0);
    (scope, counter)
}

fn observe(source: &str, scope: &mut Scope) -> Observation {
    let mut output = Observation::default();
    let host = crate::record::HostFacts {
        home: "/synthetic/home",
        user: None,
    };
    let mut evaluator = Evaluator::new(
        Frontend {
            arm: crate::shell::Arm::Brush,
            zsh: true,
            host,
        },
        &mut output,
    );
    evaluator.source(source, scope, 0).unwrap();
    assert!(output.gaps.is_empty(), "{:?}", output.gaps);
    assert!(!output.script.commands.is_empty());
    output
}

fn copies(width: usize, source: &str) -> usize {
    let (mut scope, counter) = seeded(width);
    observe(source, &mut scope);
    counter.0.get()
}

#[test]
fn saved_binding_states_do_not_copy_entries() {
    for samples in [4, 8, 16] {
        let (scope, counter) = seeded(samples * 16);
        let states = (0..samples).map(|_| scope.state()).collect::<Vec<_>>();
        let copies = states.clone();
        let restored = copies
            .into_iter()
            .map(|state| scope.with_state(state))
            .collect::<Vec<_>>();
        assert_eq!(counter.0.get(), 0, "saved snapshots copied bindings");
        assert!(
            restored
                .iter()
                .all(|next| next.bindings == scope.bindings && next.frames == scope.frames)
        );
    }
}

#[test]
fn unchanged_loop_widening_keeps_binding_storage() {
    let (before, counter) = seeded(32);
    let mut after = before.clone();
    widen_runtime_repetition(&before, &mut after);
    assert!(
        Rc::ptr_eq(&before.bindings, &after.bindings),
        "unchanged widening copied binding storage"
    );
    assert_eq!(counter.0.get(), 0);
}

#[test]
fn unrelated_writes_share_unchanged_values_and_preserve_parent_state() {
    let (scope, _) = seeded(4);
    let mut child = scope.branch();
    child.local("state1");
    child.assign("state1".into(), vec![BindingValue::Known("other".into())]);
    assert!(!Rc::ptr_eq(&scope.bindings, &child.bindings));
    assert!(
        Rc::ptr_eq(
            &scope.bindings["state2"].values,
            &child.bindings["state2"].values
        ),
        "unchanged binding payload was copied"
    );
    assert_eq!(
        scope.bindings["state1"].values[0].known().unwrap(),
        &"public".repeat(16)
    );
    assert!(!scope.frames[0].contains_key("state1"));
    let mut joined = scope.clone();
    assert!(!joined.join(&[scope.clone(), child.clone()]));
    assert!(
        Rc::ptr_eq(
            &joined.bindings["state2"].values,
            &scope.bindings["state2"].values
        ),
        "unchanged values were copied by a changed-state join"
    );
    assert_eq!(joined.bindings["state1"].values.len(), 2);
    assert!(
        joined.bindings["state1"]
            .values
            .iter()
            .any(|value| value.known().is_some_and(|value| value == "other"))
    );
    child.leave_function();
    assert_eq!(child.bindings["state1"], scope.bindings["state1"]);
}

#[test]
fn branch_and_join_share_unchanged_vectors_and_maps() {
    let (mut scope, _) = seeded(4);
    scope.directory.alternatives = Rc::new(vec![cwd::CwdPath::Logical("/synthetic/other".into())]);
    scope.directory.failures = Some(Rc::new(vec![cwd::CwdPath::Logical(
        "/synthetic/failure".into(),
    )]));
    scope.pipeline_input = Some(FlowBuilder::default().bytes("public\n".into(), Guard::new()));
    let before = scope.branch();
    assert!(
        Rc::ptr_eq(&scope.frames, &before.frames),
        "frame stack was copied"
    );
    assert!(
        Rc::ptr_eq(&scope.returns, &before.returns),
        "return snapshots were copied"
    );
    assert!(
        Rc::ptr_eq(&scope.loops, &before.loops),
        "loop snapshots were copied"
    );
    assert!(
        Rc::ptr_eq(
            &scope.directory.alternatives,
            &before.directory.alternatives
        ),
        "cwd candidates were copied"
    );
    assert!(
        Rc::ptr_eq(
            scope.directory.failures.as_ref().unwrap(),
            before.directory.failures.as_ref().unwrap()
        ),
        "cwd failures were copied"
    );
    assert!(
        Flow::ptr_eq(
            scope.pipeline_input.as_ref().unwrap(),
            before.pipeline_input.as_ref().unwrap()
        ),
        "pipeline input was copied"
    );
    observe(
        "if true; then printf public; else printf public; fi",
        &mut scope,
    );
    assert!(
        Rc::ptr_eq(&scope.bindings, &before.bindings),
        "unchanged binding join rebuilt the map"
    );
    assert!(
        Rc::ptr_eq(&scope.frames, &before.frames),
        "unchanged frame join rebuilt the stack"
    );
    assert!(
        Rc::ptr_eq(
            &scope.directory.alternatives,
            &before.directory.alternatives
        ),
        "unchanged cwd join rebuilt candidates"
    );
    assert!(
        Flow::ptr_eq(
            scope.pipeline_input.as_ref().unwrap(),
            before.pipeline_input.as_ref().unwrap()
        ),
        "unchanged pipeline join rebuilt input"
    );
}

#[test]
fn shared_multivalue_bindings_still_receive_join_normalization() {
    let mut scope = Scope::new("/synthetic/home", "/synthetic/project");
    scope.assign(
        "p".into(),
        vec![
            BindingValue::Known("public".into()),
            BindingValue::RepeatedFields(Box::new(LiteralRepetition {
                prefix: "public".into(),
                alternatives: vec!["other".into()],
                suffix: String::new(),
                may_be_empty: false,
            })),
        ],
    );
    let branch = scope.clone();
    assert!(!scope.join(&[branch.clone(), branch]));
    let values = &scope.bindings["p"].values;
    assert_eq!(
        values.len(),
        1,
        "shared multivalue join skipped normalization"
    );
    assert!(
        matches!(&values[0], BindingValue::RepeatedFields(value) if value.may_be_empty && value.alternatives == ["other"])
    );
}

#[test]
fn isolated_sources_share_function_tables_and_bodies() {
    let mut scope = Scope::new("/synthetic/home", "/synthetic/project");
    let mut output = Observation::default();
    let host = crate::record::HostFacts {
        home: "/synthetic/home",
        user: None,
    };
    let mut evaluator = Evaluator::new(
        Frontend {
            arm: crate::shell::Arm::Brush,
            zsh: true,
            host,
        },
        &mut output,
    );
    evaluator
        .source("inspect() { printf public; }", &mut scope, 0)
        .unwrap();
    let functions = evaluator.functions.clone();
    let copied = evaluator.functions["inspect"].clone();
    assert!(
        Rc::ptr_eq(&copied.body, &functions["inspect"].body),
        "function body was copied"
    );
    evaluator
        .isolated_source("printf public", &mut scope, 0)
        .unwrap();
    assert!(
        Rc::ptr_eq(&evaluator.functions, &functions),
        "isolated source copied function table"
    );
    assert!(output.gaps.is_empty());
}

#[test]
fn branch_snapshots_share_unchanged_binding_entries() {
    let counts = [4, 8, 16].map(|branches| {
        let arms = (0..branches)
            .map(|n| format!("public{n}) printf public;;"))
            .collect::<Vec<_>>()
            .join(" ");
        let count = copies(branches * 16, &format!("case public in {arms} esac"));
        println!(
            "branches={branches}, entries={}, copies={count}",
            branches * 16
        );
        (branches, count)
    });
    for &(branches, count) in &counts {
        assert!(
            count <= branches * 80,
            "branch snapshot copied {count} entries"
        );
    }
    for pair in counts.windows(2) {
        assert!(pair[1].1 <= pair[0].1 * 3 + 16, "{counts:?}");
    }
}

#[test]
fn loop_snapshots_share_unchanged_binding_entries() {
    let counts = [1, 2, 4].map(|depth| {
        let source = format!(
            "{}printf public; {}",
            "while false; do ".repeat(depth),
            "done; ".repeat(depth)
        );
        let count = copies(depth * 32, &source);
        println!("depth={depth}, entries={}, copies={count}", depth * 32);
        (depth, count)
    });
    for &(depth, count) in &counts {
        assert!(count <= depth * 160, "loop snapshot copied {count} entries");
    }
    for pair in counts.windows(2) {
        assert!(pair[1].1 <= pair[0].1 * 3 + 16, "{counts:?}");
    }
}
