#[test]
fn logical_directory_guard_work_grows_linearly() {
    let counts = [64, 128, 256].map(|width| {
        let source = std::iter::repeat_n("true", width)
            .collect::<Vec<_>>()
            .join(" && ");
        let output = crate::shell::observe(
            &source,
            crate::shell::Arm::Brush,
            "/synthetic/home",
            "/synthetic/project",
            true,
        )
        .unwrap();
        assert!(output.gaps.is_empty());
        output.logical_guard_visits
    });
    println!("logical guard visits: {counts:?}");
    for (width, count) in [64, 128, 256].into_iter().zip(counts) {
        assert!(
            count > 0 && count <= width * 4,
            "logical guard repeatedly walked the left tree: {counts:?}"
        );
    }
    for pair in counts.windows(2) {
        assert!(pair[1] <= pair[0] * 2 + 4, "{counts:?}");
    }
}
#[test]
fn failure_collection_has_polynomial_cost() {
    for zsh in [true, false] {
        let cost = |parts| {
            let source = std::iter::once("cd /synthetic/child")
                .chain(std::iter::repeat_n("echo public", parts))
                .collect::<Vec<_>>()
                .join(" && ");
            let observation = crate::shell::observe(
                &source,
                crate::shell::Arm::Brush,
                "/synthetic/home",
                "/synthetic/work",
                zsh,
            )
            .unwrap();
            assert!(observation.gaps.is_empty());
            observation.failure_copies
        };
        let small = cost(4);
        let large = cost(8);
        assert_eq!(cost(4), small, "each observation owns its counter");
        println!("zsh={zsh}: failure-path copies {small} -> {large}");
        assert!(small > 0);
        assert!(
            large <= small * 4,
            "assertion: failure-path copies {small} -> {large}"
        );
    }
}

#[test]
fn shell_frontend_removes_only_dot_segments() {
    let result = crate::shell::observe(
        "printf public",
        crate::shell::Arm::Brush,
        "/h",
        "/a/./link/../tail",
        true,
    )
    .unwrap();
    assert_eq!(result.script.commands[0].cwd, "/a/link/../tail");
}
