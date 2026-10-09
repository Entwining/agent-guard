use super::*;
#[test]
fn executable_flow_growth_tracks_branch_outputs() {
    let counts = [4, 8, 16].map(|size| {
        let arms = (0..size)
            .map(|index| format!("p{index}) printf '%s\\n' 'cat /p/public{index}';;"))
            .collect::<String>();
        let source = format!("case public in {arms} esac | cat | sh");
        let output = observation(&source, &mut Scope::new("/h", "/p"));
        for index in 0..size {
            assert!(output.script.commands.iter().any(|command| {
                command
                    .argv
                    .iter()
                    .any(|word| word.text == format!("/p/public{index}"))
            }));
        }
        assert!(output.gaps.is_empty(), "{:?}", output.gaps);
        println!(
            "size={size}, flow_nodes={}, flow_pairs={}",
            output.flow_nodes, output.flow_pairs
        );
        assert!(output.flow_nodes > 0 && output.flow_nodes <= size * 32);
        assert!(output.flow_pairs > 0 && output.flow_pairs <= size * 16);
        (output.flow_nodes, output.flow_pairs)
    });
    for pair in counts.windows(2) {
        assert!(
            pair[1].0 <= pair[0].0 * 3 && pair[1].1 <= pair[0].1 * 3,
            "{counts:?}"
        );
    }
}

#[test]
fn ordered_loop_output_growth_tracks_header_members() {
    let counts = [4, 8, 16].map(|size| {
        let members = (0..size)
            .map(|index| format!("'cat /p/public{index}\n'"))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("for code in {members}; do printf '%s' \"$code\"; done | sh");
        let output = observation(&source, &mut Scope::new("/h", "/p"));
        assert!(output.gaps.is_empty(), "{:?}", output.gaps);
        for index in 0..size {
            assert!(output.script.commands.iter().any(|command| {
                command
                    .argv
                    .iter()
                    .any(|word| word.text == format!("/p/public{index}"))
            }));
        }
        println!(
            "size={size}, nodes={}, visits={}, pairs={}, guards={}",
            output.flow_nodes, output.flow_visits, output.flow_pairs, output.flow_guard_pairs
        );
        assert!(output.flow_nodes <= size * 64);
        assert!(output.flow_visits <= size * 64);
        assert!(output.flow_pairs <= size * 64);
        assert!(output.flow_guard_pairs <= size * 128);
        (
            output.flow_nodes,
            output.flow_visits,
            output.flow_pairs,
            output.flow_guard_pairs,
        )
    });
    for pair in counts.windows(2) {
        assert!(
            pair[1].0 <= pair[0].0 * 3
                && pair[1].1 <= pair[0].1 * 3
                && pair[1].2 <= pair[0].2 * 3
                && pair[1].3 <= pair[0].3 * 3,
            "{counts:?}"
        );
    }
}

#[test]
fn sequential_stdout_choices_merge_equivalent_paths() {
    let counts = [8, 16, 32].map(|size| {
        let body = "if true; then printf x; else printf ''; fi; ".repeat(size);
        let source = format!("{{ printf 'echo '; {body} printf public; }} | sh");
        let output = observation(&source, &mut Scope::new("/h", "/p"));
        assert!(
            !output.gaps.contains(&CoverageGap::InspectionBudget),
            "{:?}",
            output.gaps
        );
        assert!(output.script.commands.iter().any(|command| {
            command
                .argv
                .iter()
                .any(|word| word.text == format!("{}public", "x".repeat(size)))
        }));
        println!(
            "size={size}, nodes={}, visits={}, pairs={}, guards={}",
            output.flow_nodes, output.flow_visits, output.flow_pairs, output.flow_guard_pairs
        );
        assert!(output.flow_nodes <= size * 32);
        assert!(output.flow_visits <= size * 64);
        assert!(output.flow_pairs <= size * size * 16);
        assert!(output.flow_guard_pairs <= size * size * 8);
        (
            output.flow_visits,
            output.flow_pairs,
            output.flow_guard_pairs,
        )
    });
    for pair in counts.windows(2) {
        assert!(
            pair[1].0 <= pair[0].0 * 3 && pair[1].1 <= pair[0].1 * 5 && pair[1].2 <= pair[0].2 * 5,
            "{counts:?}"
        );
    }
}

#[test]
fn loop_flow_converges_without_repeating_origin_tags() {
    let counts = [4, 8, 16].map(|size| {
            let branch = "[ -n \"$remote\" ] && echo \"$d -> $remote\"; ".repeat(size);
            let source = format!(
                "for d in /p/*/; do cd \"$d\" 2>/dev/null && remote=$(git config --get remote.origin.url); {branch} done | head -10"
            );
            let output = observation(&source, &mut Scope::new("/h", "/p"));
            assert!(!output.gaps.contains(&CoverageGap::InspectionBudget));
            println!("size={size}, visits={}, nodes={}, pairs={}, guards={}", output.statement_visits, output.flow_nodes, output.flow_pairs, output.flow_guard_pairs);
            assert!(output.statement_visits <= size * 8);
            assert!(output.flow_nodes <= size * 64);
            assert!(output.flow_pairs <= size * size * 32);
            assert!(output.flow_guard_pairs <= size * size * 32);
            (output.statement_visits, output.flow_nodes, output.flow_pairs, output.flow_guard_pairs)
        });
    for pair in counts.windows(2) {
        assert!(
            pair[1].0 <= pair[0].0 * 3
                && pair[1].1 <= pair[0].1 * 3
                && pair[1].2 <= pair[0].2 * 5
                && pair[1].3 <= pair[0].3 * 5,
            "{counts:?}"
        );
    }
}

#[test]
fn loop_continue_does_not_tag_unchanged_bindings() {
    let source =
        "for f in /p/*; do printf public | cat | cat || continue; printf public; done | cat";
    let output = observation(source, &mut Scope::new("/h", "/p"));
    assert!(!output.gaps.contains(&CoverageGap::InspectionBudget));
    assert!(output.statement_visits < 32, "{}", output.statement_visits);
}
