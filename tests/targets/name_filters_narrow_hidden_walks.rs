use crate::program_contract as contract;

const ROWS: &str = include_str!("../fixtures/rust-hidden-walk-name-bounds.json");

#[test]
fn project_walks_without_a_credential_name_are_allowed() {
    contract::partition("allowed", ROWS);
}

#[test]
fn bounds_naming_credential_files_refuse_as_credential_reads() {
    contract::partition("credential-name", ROWS);
}

#[test]
fn narrowed_walks_keep_their_root_protection() {
    contract::partition("protected-root", ROWS);
}

#[test]
fn recursive_consumers_walk_below_the_names_they_are_handed() {
    contract::partition("recursive-consumer", ROWS);
}
