const ROWS: &str = include_str!("../fixtures/rust-option-values.json");

#[test]
fn option_values_that_name_input_files_are_read() {
    crate::program_contract::partition("read", ROWS);
}

#[test]
fn output_option_values_are_written_not_read() {
    crate::program_contract::partition("write", ROWS);
}

#[test]
fn directory_option_values_are_entered() {
    crate::program_contract::partition("directory", ROWS);
}

#[test]
fn data_option_values_are_not_paths() {
    crate::program_contract::partition("data", ROWS);
}

#[test]
fn at_file_names_follow_the_program_that_defines_them() {
    crate::program_contract::partition("at-file", ROWS);
}

#[test]
fn unique_long_option_prefixes_bind_their_option() {
    crate::program_contract::partition("abbreviation", ROWS);
}

#[test]
fn short_option_equals_signs_follow_the_parser() {
    crate::program_contract::partition("dialect", ROWS);
}

#[test]
fn letters_whose_arity_differs_take_only_glued_values() {
    crate::program_contract::partition("arity", ROWS);
}

#[test]
fn unlisted_options_of_a_complete_grammar_leave_later_words_readable() {
    crate::program_contract::partition("unlisted", ROWS);
}
