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
