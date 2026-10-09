use crate::program_contract as contract;

#[test]
fn runtime_unknown_prefix_preserves_appdata_tail_protection() {
    contract::partition(
        "unknown",
        include_str!("../fixtures/rust-batch10c-appdata.json"),
    );
}

#[test]
fn exactly_computed_prefix_and_ordinary_unknown_path_remain_precise() {
    contract::partition(
        "control",
        include_str!("../fixtures/rust-batch10c-appdata.json"),
    );
}
