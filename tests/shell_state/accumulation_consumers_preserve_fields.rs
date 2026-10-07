use crate::program_contract as contract;

#[test]
fn unconditional_appends_preserve_complete_values() {
    contract::partition(
        "exact",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}

#[test]
fn producer_candidates_reach_xargs_operands() {
    contract::partition(
        "pipeline",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}

#[test]
fn complete_value_consumers_keep_independent_field_roles() {
    contract::partition(
        "complete",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}

#[test]
fn positional_sequences_keep_all_operand_candidates() {
    contract::partition(
        "positionals",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}

#[test]
fn fixed_pattern_and_archive_roles_ignore_operand_count() {
    contract::partition(
        "fixed_roles",
        include_str!("../fixtures/rust-batch17c-fields.json"),
    );
}

#[test]
fn literal_ifs_controls_unquoted_field_splitting() {
    contract::partition("ifs", include_str!("../fixtures/rust-batch17c-fields.json"));
}
