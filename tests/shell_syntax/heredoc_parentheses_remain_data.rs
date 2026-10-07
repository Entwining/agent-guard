#[test]
fn heredoc_parentheses_remain_data() {
    crate::program_contract::partition(
        "heredoc",
        include_str!("../fixtures/rust-heredoc-parentheses.json"),
    );
}
