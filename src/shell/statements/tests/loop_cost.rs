#[test]
fn header_membership_work_grows_subquadratically() {
    let counts = [128, 256, 512].map(|width| {
        let items = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("for f in {items}; do printf public; done");
        let output =
            crate::shell::observe(&source, crate::shell::Arm::Brush, "/h", "/h/p", true).unwrap();
        assert!(output.header_comparisons > 0);
        output.header_comparisons
    });
    println!("header comparisons: {counts:?}");
    for pair in counts.windows(2) {
        assert!(pair[1] < pair[0] * 3, "{counts:?}");
    }
}
#[test]
fn unconditional_literal_append_keeps_exact_string() {
    for width in [4, 8, 16] {
        let items = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("M=prefix; for f in {items}; do M=\"$M $f\"; done; echo \"$M\"");
        let output =
            crate::shell::observe(&source, crate::shell::Arm::Brush, "/h", "/h/p", true).unwrap();
        let echo = output.script.commands.last().unwrap();
        assert_eq!(echo.argv[1].text, format!("prefix {items}"));
        assert!(!echo.argv[1].cardinality_unknown);
        assert!(!echo.argv[1].expands);
    }
}

#[test]
fn conditional_literal_accumulation_work_grows_polynomially() {
    for bound in [false, true] {
        let count = |width| {
            let items = (0..width)
                .map(|n| format!("public{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            let header = if bound {
                format!("REQ='{items}'; for f in $REQ")
            } else {
                format!("for f in {items}")
            };
            let source = format!(
                "M=prefix; {header}; do test -f public || M=\"$M $f\"; done; echo \"[$M]\""
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
                "bound={bound} width={width}: {:?}",
                output.gaps
            );
            (
                output.statement_visits,
                output.candidate_pairs,
                output.script.commands.len(),
            )
        };
        let counts = [4, 8, 16].map(count);
        println!("conditional accumulation bound={bound}: {counts:?}");
        for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
            assert!(
                small.0 > 0
                    && large.0 <= small.0 * 5
                    && large.1 <= small.1 * 5
                    && large.2 <= small.2 * 5,
                "bound={bound}: {counts:?}"
            );
        }
    }
}
#[test]
fn unknown_fragment_repetition_converges_before_branching() {
    let count = |width| {
        let items = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "for f in {items}; do Q=\"\"; while read id; do Q=\"${{Q}}&x=${{id}}\"; done < \"$f\"; for c in true false; do curl \"https://example.test/?c=${{c}}${{Q}}\" -o public.json; done; done"
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
        (output.statement_visits, output.script.commands.len())
    };
    let counts = [2, 4, 8].map(count);
    println!("unknown repetition work={counts:?}");
    for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
        assert!(
            small.0 > 0 && large.0 <= small.0 * 3 && large.1 <= small.1 * 3,
            "{counts:?}"
        );
    }
}
#[test]
fn repeated_unknown_hits_converge_before_the_record_budget() {
    for width in [2, 4] {
        let items = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "hit=\"\"; for p in {items}; do for c in $(printf public); do [ \"$(printf public)\" = public ] && hit=\"$hit ${{c:0:7}}:$p\"; done; done; echo \"$hit\""
        );
        let observation = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert!(
            !observation
                .gaps
                .contains(&crate::CoverageGap::InspectionBudget)
        );
        assert!(
            observation.source_entries <= width * 8,
            "width={width}, recursive sources={}",
            observation.source_entries
        );
    }
}
#[test]
fn loop_directory_work_grows_polynomially() {
    let cost = |width| {
        let items = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source =
            format!("for r in a b c d; do for b in {items}; do cd $r/$b && ls; done; done");
        let observation = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        println!(
            "width={width}, cwd={}, gaps={:?}",
            observation.cwd_candidates, observation.gaps
        );
        assert!(
            !observation
                .gaps
                .contains(&crate::CoverageGap::InspectionBudget)
        );
        observation.cwd_candidates
    };
    let small = cost(5);
    let large = cost(10);
    println!("cwd candidates {small} -> {large}");
    assert!(small > 0 && large <= small * 4);
}
