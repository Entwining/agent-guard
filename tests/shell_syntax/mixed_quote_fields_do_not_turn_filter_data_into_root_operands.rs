use crate::program_contract as contract;

#[test]
fn mixed_quote_fields_do_not_turn_filter_data_into_root_operands() {
    contract::partition(
        "public",
        include_str!("../fixtures/rust-batch10c-quoted-fields.json"),
    );
}

#[test]
fn mixed_quote_fields_preserve_independent_protected_operands() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch10c-quoted-fields.json"),
    );
}
