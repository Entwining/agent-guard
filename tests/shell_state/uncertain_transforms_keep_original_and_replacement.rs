use crate::program_contract as contract;

#[test]
fn uncertain_transforms_keep_original_and_replacement() {
    contract::partition(
        "uncertain-transforms",
        include_str!("../fixtures/rust-uncertain-transforms.json"),
    );
}
