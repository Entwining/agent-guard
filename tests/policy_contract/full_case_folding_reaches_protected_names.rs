#[test]
fn full_case_folding_reaches_protected_names() {
    crate::program_contract::partition(
        "apfs-case-folding",
        include_str!("../fixtures/rust-apfs-case-folding.json"),
    );
}
