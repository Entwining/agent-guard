use agent_guard_rust::shell::{Arm, observe};

#[test]
fn nested_matrix_command_cost_is_polynomial() {
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
        let observed = observe(
            &source,
            Arm::Brush,
            "/synthetic/home",
            "/synthetic/work",
            true,
        )
        .unwrap();
        let parses = observed.parse_successes + observed.parse_failures;
        println!(
            "levels={levels}, parses={parses}, commands={}, gaps={:?}",
            observed.script.commands.len(),
            observed.gaps
        );
        observed.script.commands.len()
    };
    let small = count(2);
    let large = count(4);
    assert!(small > 0);
    assert!(
        large <= small * 4,
        "assertion: nested matrix commands {small} -> {large}"
    );
}
