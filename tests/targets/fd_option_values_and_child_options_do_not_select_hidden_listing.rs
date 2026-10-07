use crate::program_contract as contract;

#[test]
fn fd_option_values_and_child_options_do_not_select_hidden_listing() {
    contract::partition("control", include_str!("../fixtures/rust-batch10-fd.json"));
}

#[test]
fn fd_explicit_hidden_content_consumers_still_refuse() {
    contract::partition(
        "protected",
        include_str!("../fixtures/rust-batch10-fd.json"),
    );
}
