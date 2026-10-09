use crate::program_contract as contract;

#[test]
fn filters_are_data_and_later_operands_remain_readable() {
    contract::partition(
        "control",
        include_str!("../fixtures/rust-batch10-filters.json"),
    );
}

#[test]
fn filter_files_and_protected_input_operands_refuse() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch10-filters.json"),
    );
}
