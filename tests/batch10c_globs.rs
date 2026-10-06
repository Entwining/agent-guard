#[path = "support/batch6.rs"]
mod contract;

#[test]
fn visible_shell_matches_and_literal_search_roots_skip_hidden_credentials() {
    contract::partition("visible", include_str!("fixtures/rust-batch10c-globs.json"));
}

#[test]
fn explicit_hidden_patterns_and_literal_git_pathspecs_keep_protection() {
    contract::partition("control", include_str!("fixtures/rust-batch10c-globs.json"));
}
