#[test]
fn single_choice_argv_copies_grow_linearly() {
    for prefix in ["cat", "F=public cat", "env F=public cat", "cat -n"] {
        let counts = [64, 128, 256].map(|width| {
            let source = format!("{prefix} {}", vec!["pub.txt"; width].join(" "));
            let output = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/project",
                true,
            )
            .unwrap();
            assert!(output.gaps.is_empty(), "{prefix}: {:?}", output.gaps);
            let copies = output.argv_prefix_word_copies;
            println!("{prefix}: width={width}, argv word copies={copies}");
            assert!(
                copies > 0 && copies <= 4 * (width + 4),
                "{prefix}: {copies}"
            );
            copies
        });
        for pair in counts.windows(2) {
            assert!(pair[1] <= pair[0] * 3, "{prefix}: {counts:?}");
        }
    }
}

#[test]
fn literal_argv_compatibility_checks_grow_linearly() {
    for width in [64, 128, 256] {
        let source = format!("cat {}", vec!["pub.txt"; width].join(" "));
        let output = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        let checks = output.argument_compatibility_checks;
        println!("width={width}, argv compatibility checks={checks}");
        assert!(checks <= 2 * (width + 1), "{checks}");
    }
}

#[test]
fn compatible_bound_argv_extensions_grow_linearly() {
    for width in [64, 128, 256] {
        let source = format!(
            "if true; then p=public; else p=other; fi; cat {}",
            vec!["\"$p\""; width].join(" ")
        );
        let output = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert!(output.gaps.is_empty(), "{:?}", output.gaps);
        let commands = output
            .script
            .commands
            .iter()
            .filter(|command| command.argv.first().is_some_and(|word| word == "cat"))
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 2);
        for (command, value) in commands.iter().zip(["public", "other"]) {
            assert_eq!(command.argv.len(), width + 1);
            assert!(command.argv[1..].iter().all(|word| word == value));
        }
        println!(
            "bound width={width}, copies={}, checks={}",
            output.argv_prefix_word_copies, output.argument_compatibility_checks
        );
        assert!(output.argv_prefix_word_copies <= 12 * (width + 4));
        assert!(output.argument_compatibility_checks <= 12 * (width + 4));
    }
}

#[test]
fn argument_compatibility_retains_every_prior_binding_value() {
    let word = |value: &str, role| {
        let mut word = crate::record::Word::literal(value.into());
        word.role = role;
        word.binding_candidates.insert("p".into(), value.into());
        word
    };
    let prefix = super::ArgumentAlternative::new(vec![
        word("public", crate::record::Role::Arg),
        word("other", crate::record::Role::Arg),
    ]);
    let mut checks = 0;
    assert!(
        !prefix.compatible(&[word("other", crate::record::Role::Arg)], &mut checks),
        "a choice must agree with both prior values, not only the last"
    );
    assert!(prefix.compatible(&[crate::record::Word::literal("plain".into())], &mut checks));
    let ignored = super::ArgumentAlternative::new(vec![
        word("public", crate::record::Role::Assign),
        word("public", crate::record::Role::Precommand),
    ]);
    assert!(ignored.compatible(&[word("other", crate::record::Role::Arg)], &mut checks));
}

#[test]
fn unknown_loop_candidates_converge_before_nested_branching() {
    let count = |width, depth| {
        let fields = (0..width)
            .map(|n| format!("public{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut source = format!("inspect() {{ cat \"$1\"; }}; for path in {fields}; do ");
        for level in 0..depth {
            source.push_str(&format!("for k{level} in $(printf public); do "));
        }
        source.push_str("out=$(inspect \"$path\"); cat \"$path\"; ");
        source.push_str(&"done; ".repeat(depth + 1));
        let output = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            false,
        )
        .unwrap();
        assert!(
            !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
            "width={width} depth={depth}: {:?}",
            output.gaps
        );
        for index in 0..width {
            assert!(
                output.script.commands.iter().any(|command| command
                    .argv
                    .first()
                    .is_some_and(|w| w == "cat")
                    && command.argv[1] == format!("public{index}").as_str())
            );
        }
        (
            output.statement_visits,
            output.script.commands.len(),
            output.parse_builds,
        )
    };
    let widths = [2, 4, 8, 16].map(|width| count(width, 2));
    let depths = [1, 2, 4, 8].map(|depth| count(4, depth));
    println!("unknown loop candidates widths={widths:?}, depths={depths:?}");
    for (small, large) in widths
        .iter()
        .zip(widths.iter().skip(1))
        .chain(depths.iter().zip(depths.iter().skip(1)))
    {
        assert!(large.0 <= small.0 * 3 && large.1 <= small.1 * 3 && large.2 == small.2);
    }
}

#[test]
fn substitution_body_is_observed_once_per_candidate_union() {
    for width in [2, 4, 8] {
        let mut output = super::Observation::default();
        let host = crate::record::HostFacts {
            home: "/synthetic/home",
            user: None,
        };
        let mut evaluator = super::Evaluator::new(
            super::Frontend {
                arm: crate::shell::Arm::Brush,
                zsh: true,
                host,
            },
            &mut output,
        );
        let mut scope = super::Scope::new(host.home, "/synthetic/project");
        for name in ["a", "b"] {
            scope.assign(
                name.into(),
                (0..width)
                    .map(|n| super::BindingValue::Known(format!("public{n}")))
                    .collect(),
            );
        }
        evaluator
            .expand(
                &super::RawWord {
                    raw: "${a}$(printf public)${b}".into(),
                    syntax: super::WordSyntax::Shell,
                    expansions: Vec::new(),
                },
                &mut scope,
                0,
            )
            .unwrap();
        assert_eq!(output.source_entries, 1, "width={width}");
        assert_eq!(output.parse_successes, 1, "width={width}");
        assert!(output.gaps.is_empty(), "{:?}", output.gaps);
    }
}
#[test]
fn nested_substitution_work_grows_polynomially() {
    let count = |levels| {
        let names = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
        let mut source = String::from("P=1; F=2; ");
        for name in names.iter().take(levels) {
            source.push_str(&format!("for {name} in \"\" \"$P\" \"$F\"; do "));
        }
        source.push_str("o=$(sh x.sh");
        for name in names.iter().take(levels) {
            source.push_str(&format!(" \"${name}\""));
        }
        source.push_str(");");
        source.push_str(&" done;".repeat(levels));
        let output = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/work",
            true,
        )
        .unwrap();
        assert!(
            output
                .script
                .commands
                .iter()
                .any(|c| c.argv.iter().any(|w| w.text == "x.sh")),
            "nested script operand was lost"
        );
        assert!(output.source_entries > 0);
        assert!(
            output.source_entries <= levels + 1,
            "nested source body reparsed for loop combinations: levels={levels}, entries={}",
            output.source_entries
        );
        (
            output.source_entries,
            output.parse_successes + output.parse_failures,
            output.candidate_pairs,
            output.script.commands.len(),
        )
    };
    let counts = [2, 4, 8].map(count);
    println!("nested substitution work={counts:?}");
    for (small, large) in counts.iter().zip(counts.iter().skip(1)) {
        assert!(
            large.0 <= small.0 * 4
                && large.1 <= small.1 * 4
                && large.2 <= small.2 * 8
                && large.3 <= small.3 * 4,
            "nested substitution work: {counts:?}"
        );
    }
}

#[test]
fn repeated_binding_work_grows_quadratically() {
    let observe = |size| {
        let items = (0..size)
            .map(|n| format!("v{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let source =
            format!("for id in {items}; do curl https://example.test/$id -o file_$id -w $id; done");
        let output =
            crate::shell::observe(&source, crate::shell::Arm::Brush, "/h", "/h/p", true).unwrap();
        assert!(output.gaps.is_empty(), "{:?}", output.gaps);
        output.candidate_pairs
    };
    let small = observe(8);
    let large = observe(16);
    println!("candidate pairs: {small} -> {large}");
    assert!(
        small > 0 && large <= small * 5,
        "candidate pair growth: {small} -> {large}"
    );
}
#[test]
fn glob_directory_work_stays_bounded() {
    let count = |size, isolated, branch| {
        let mut source = String::new();
        for n in 0..size {
            let body = match branch {
                0 => format!("cd \"$d{n}\"; ls"),
                1 => format!("if printf public; then cd \"$d{n}\"; fi; ls"),
                _ => format!("printf public && cd \"$d{n}\"; ls"),
            };
            let body = if isolated { format!("({body})") } else { body };
            source.push_str(&format!("for d{n} in public*/; do {body}; done;"));
        }
        let output =
            crate::shell::observe(&source, crate::shell::Arm::Brush, "/h", "/h/p", true).unwrap();
        if isolated {
            assert!(output.gaps.is_empty(), "{:?}", output.gaps);
        } else {
            assert!(
                output.gaps.contains(&crate::CoverageGap::IdentityBound),
                "{:?}",
                output.gaps
            );
            assert!(
                !output.gaps.contains(&crate::CoverageGap::InspectionBudget),
                "{:?}",
                output.gaps
            );
        }
        output.candidate_pairs
    };
    for (isolated, branch) in [(false, 0), (true, 0), (false, 1), (false, 2)] {
        let small = count(2, isolated, branch);
        let large = count(4, isolated, branch);
        println!(
            "isolated={isolated}, branch={branch}: glob directory candidate pairs: {small} -> {large}"
        );
        assert!(
            small > 0 && large <= small * 4,
            "glob directory work: {small} -> {large}"
        );
    }
}
