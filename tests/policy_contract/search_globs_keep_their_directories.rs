#[test]
fn search_globs_keep_their_directories() {
    crate::program_contract::partition(
        "search-glob-path",
        include_str!("../fixtures/rust-search-glob-paths.json"),
    );
}

#[test]
fn search_globs_select_hidden_names_they_match() {
    crate::program_contract::partition(
        "search-glob-hidden",
        include_str!("../fixtures/rust-search-glob-paths.json"),
    );
}
