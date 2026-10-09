#[test]
fn bracket_classes_match_either_case() {
    crate::program_contract::partition(
        "case-folded-class",
        include_str!("../fixtures/rust-case-folded-classes.json"),
    );
}

#[test]
fn parameter_patterns_match_case_exactly() {
    crate::program_contract::partition(
        "case-exact-parameter",
        include_str!("../fixtures/rust-case-folded-classes.json"),
    );
}

#[test]
fn collating_spellings_keep_the_parameter_value() {
    crate::program_contract::partition(
        "collating-spelling",
        include_str!("../fixtures/rust-case-folded-classes.json"),
    );
}
