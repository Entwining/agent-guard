#[test]
fn find_leading_options_preserve_roots() {
    crate::program_contract::partition(
        "leading",
        include_str!("../fixtures/rust-find-leading.json"),
    );
}
