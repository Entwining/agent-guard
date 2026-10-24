use crate::program_contract as contract;

#[test]
fn inherited_presence_reaches_defaults_and_children() {
    contract::partition(
        "inherited-presence",
        include_str!("../fixtures/rust-inherited-presence.json"),
    );
}
