use crate::program_contract as contract;

#[test]
fn fd_option_values_are_not_search_roots() {
    contract::partition("control", include_str!("../fixtures/rust-batch10-fd.json"));
}

#[test]
fn fd_protected_roots_refuse() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch10-fd.json"),
    );
}
