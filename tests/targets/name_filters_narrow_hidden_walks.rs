use crate::program_contract as contract;

const ROWS: &str = include_str!("../fixtures/rust-hidden-walk-name-bounds.json");

#[test]
fn source_name_filters_narrow_hidden_walks() {
    contract::partition("narrowed", ROWS);
}

#[test]
fn listings_without_content_consumers_stay_allowed() {
    contract::partition("listing", ROWS);
}

#[test]
fn filters_that_match_every_name_keep_the_hidden_walk() {
    contract::partition("unbounded", ROWS);
}

#[test]
fn bounds_reaching_credential_names_at_any_depth_refuse() {
    contract::partition("bound-reaches-credential", ROWS);
}

#[test]
fn bounds_naming_credential_files_refuse_as_credential_reads() {
    contract::partition("credential-name", ROWS);
}

#[test]
fn hidden_search_advice_commands_are_allowed() {
    contract::partition("advice", ROWS);
}

#[test]
fn file_operands_are_read_without_a_walk() {
    contract::partition("file-operand", ROWS);
}

#[test]
fn narrowed_walks_keep_their_root_protection() {
    contract::partition("protected-root", ROWS);
}

#[test]
fn recursive_consumers_walk_below_the_names_they_are_handed() {
    contract::partition("recursive-consumer", ROWS);
}
