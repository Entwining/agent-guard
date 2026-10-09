#[test]
fn bracket_classes_match_either_case() {
    crate::program_contract::partition(
        "case-folded-class",
        include_str!("../fixtures/rust-case-folded-classes.json"),
    );
}

#[test]
fn bash_collating_classes_name_their_character() {
    crate::program_contract::partition(
        "collating-class",
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
