const ROWS: &str = include_str!("../fixtures/rust-git-pathspec-magic.json");

#[test]
fn git_pathspec_magic_names_its_path() {
    crate::program_contract::partition("magic", ROWS);
}

#[test]
fn git_exclusion_pathspecs_read_nothing() {
    crate::program_contract::partition("exclude", ROWS);
}
