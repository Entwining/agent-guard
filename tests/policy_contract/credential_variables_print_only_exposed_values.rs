const ROWS: &str = include_str!("../fixtures/rust-credential-variable-exposure.json");

#[test]
fn only_uppercase_names_are_credential_variables() {
    crate::program_contract::partition("script-variable", ROWS);
}

#[test]
fn length_presence_and_discarded_output_do_not_print_the_value() {
    crate::program_contract::partition("unexposed", ROWS);
}

#[test]
fn any_expansion_of_the_value_still_prints_it() {
    crate::program_contract::partition("exposed", ROWS);
}
