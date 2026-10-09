#[test]
fn nul_command_bytes_follow_go_lexer() {
    crate::program_contract::partition("nul", include_str!("../fixtures/rust-nul-command.json"));
}
