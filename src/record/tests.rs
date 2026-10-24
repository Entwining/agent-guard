#[test]
fn rewritten_word_clears_cwd_projection_and_preserves_raw_provenance() {
    let mut old = super::Word::literal("/old".into());
    old.raw = "$(pwd)/old".into();
    old.pwd = true;
    old.cwd_ranges = std::iter::once(0..4).collect();
    let new = old.with_text("/new".into());
    assert_eq!((new.text.as_str(), new.value.as_str()), ("/new", "/new"));
    assert!(!new.pwd && new.cwd_ranges.is_empty());
    assert_eq!(new.raw, old.raw);
}
#[test]
fn projected_cwd_ranges_describe_current_word_bytes() {
    let packet: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/rust-m2-cwd.json")).unwrap();
    let mut projected = 0;
    for row in packet["rows"].as_array().unwrap() {
        let result = crate::shell::observe(
            row["source"].as_str().unwrap(),
            crate::shell::Arm::Brush,
            "/h",
            row["cwd"].as_str().unwrap(),
            true,
        )
        .unwrap();
        for command in result.script.commands {
            for word in command.argv {
                for range in word.cwd_ranges {
                    assert_eq!(
                        word.text.get(range.clone()),
                        Some(command.cwd.as_str()),
                        "{}",
                        row["id"]
                    );
                    assert_eq!(
                        word.value.get(range),
                        Some(command.cwd.as_str()),
                        "{}",
                        row["id"]
                    );
                    projected += 1;
                }
            }
        }
    }
    assert!(projected > 0);
}
#[test]
fn field_width_and_unfixed_bytes_decide_option_position_separately() {
    let arguments = |source: &str| {
        let result =
            crate::shell::observe(source, crate::shell::Arm::Brush, "/h", "/h/project", true)
                .unwrap();
        result
            .script
            .commands
            .iter()
            .filter(|command| command.argv.first().is_some_and(|word| word == "cat"))
            .flat_map(|command| command.argv[1..].to_vec())
            .collect::<Vec<_>>()
    };
    let repeated = "M=\"\"; for f in a b; do test -f public || M=\"$M $f\"; done; ";
    // Each split field of a repetition keeps its own bytes; only the count is open.
    let fields = arguments(&format!("{repeated}cat $M"));
    assert!(fields.iter().any(|word| word == "a"));
    for word in fields.iter().filter(|word| !word.text.is_empty()) {
        assert!(
            word.field_count_unknown && word.exact().is_some(),
            "{word:?}"
        );
        assert!(!word.option_unknown(), "{word:?}");
    }
    // A quoted repetition joins an open number of fields into one word.
    for word in arguments(&format!("{repeated}cat \"$M\"")) {
        assert!(word.text.is_empty() || word.exact().is_none(), "{word:?}");
    }
    // Unfixed text after a literal prefix keeps that prefix.
    for word in arguments("read V; cat \"x$V\"") {
        assert!(word.exact().is_none() && !word.option_unknown(), "{word:?}");
    }
    for word in arguments("read V; cat \"$V\"") {
        assert!(word.option_unknown(), "{word:?}");
    }
}
