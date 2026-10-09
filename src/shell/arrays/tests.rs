#[test]
fn unbounded_array_append_converges_by_resource_identity() {
    let count = |width, depth| {
        let fields = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut body = format!("A+=({fields});");
        for _ in 0..depth {
            body = format!("while test public; do {body} done;");
        }
        let output = crate::shell::observe(
            &format!("A=(prefix); {body} cat \"${{A[@]}}\""),
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert!(
            !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
            "width={width}, depth={depth}"
        );
        let words = output
            .script
            .commands
            .iter()
            .filter(|c| c.argv.first().is_some_and(|w| w == "cat"))
            .flat_map(|c| c.argv.iter().skip(1))
            .map(|w| w.text.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(words.contains("prefix"));
        for n in 0..width {
            assert!(words.contains(format!("public{n}").as_str()));
        }
        (output.statement_visits, output.array_words)
    };
    let widths = [2, 4, 8].map(|width| count(width, 2));
    let depths = [1, 2, 4, 8].map(|depth| count(4, depth));
    println!("array convergence widths={widths:?}, depths={depths:?}");
    for (small, large) in widths.iter().zip(widths.iter().skip(1)) {
        assert!(large.0 <= small.0 * 5 && large.1 <= small.1 * 5);
    }
    for (small, large) in depths.iter().zip(depths.iter().skip(1)) {
        assert!(large.0 <= small.0 * 4 && large.1 <= small.1 * 4);
    }
}

#[test]
fn equal_array_executor_readings_share_one_projection() {
    let count = |expansion| {
        let output = crate::shell::observe(
            &format!("A=(first second); cat {expansion}"),
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        output
            .script
            .commands
            .iter()
            .filter(|command| command.argv.first().is_some_and(|word| word == "cat"))
            .count()
    };
    assert_eq!(count("\"${A[@]}\""), 1);
    assert_eq!(count("\"${A[1]}\""), 2);
    let output = crate::shell::observe(
        "A=(first second); for f in \"${A[@]}\"; do cat \"$f\"; done",
        crate::shell::Arm::Brush,
        "/synthetic/home",
        "/synthetic/project",
        true,
    )
    .unwrap();
    assert_eq!(
        output
            .script
            .commands
            .iter()
            .filter(|command| command.program.is_none())
            .count(),
        1
    );
}

#[test]
fn array_repetition_projects_loop_candidates_once() {
    let count = |width| {
        let items = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "cd public && say() {{ printf '%s' public | cat; }} && A=({items}); for f in \"${{A[@]}}\"; do x=$(say | grep -oF \"$f\"); y=$(say | grep -oF \"$f\"); z=$(say | grep -oF \"$f\"); printf '%s' \"$f\"; done"
        );
        let output = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert!(
            !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
            "width={width}: {:?}",
            output.gaps
        );
        (
            output.statement_visits,
            output.source_entries,
            output.script.commands.len(),
        )
    };
    let counts = [4, 8, 16, 24, 32].map(count);
    println!("array repeated-loop work: {counts:?}");
    for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
        assert!(
            small.0 > 0 && large.0 <= small.0 * 3 && large.1 == small.1 && large.2 <= small.2 * 3,
            "{counts:?}"
        );
    }
}

#[test]
fn array_candidate_work_grows_polynomially() {
    let count = |width| {
        let items = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "A=(base); for f in {items}; do test -f public || A+=(\"$f\"); done; cat \"${{A[@]}}\""
        );
        let output = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert!(
            !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
            "width={width}: {:?}",
            output.gaps
        );
        let operands = output
            .script
            .commands
            .iter()
            .filter(|command| command.argv.first().is_some_and(|word| word == "cat"))
            .flat_map(|command| command.argv.iter().skip(1))
            .map(|word| word.text.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for n in 0..width {
            assert!(operands.contains(format!("public{n}").as_str()));
        }
        (
            output.array_words,
            output.candidate_pairs,
            output.statement_visits,
        )
    };
    let counts = [4, 8, 16].map(count);
    println!("array candidate work: {counts:?}");
    for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
        assert!(
            small.0 > 0
                && large.0 <= small.0 * 5
                && large.1 <= small.1 * 5
                && large.2 <= small.2 * 5,
            "{counts:?}"
        );
    }
}
