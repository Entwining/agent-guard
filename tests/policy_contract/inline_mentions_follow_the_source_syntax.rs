use crate::program_contract;

const ROWS: &str = include_str!("../fixtures/rust-inline-mentions.json");

#[test]
fn interpreter_code_names_are_not_mentions() {
    program_contract::partition("interpreter-code", ROWS);
}

#[test]
fn interpreter_string_content_and_slash_tokens_are_mentions() {
    program_contract::partition("interpreter-string", ROWS);
}

#[test]
fn shell_fragments_keep_every_dot_name() {
    program_contract::partition("shell-fragment", ROWS);
}

#[test]
fn piped_text_an_interpreter_runs_as_its_program_is_source() {
    program_contract::partition("piped-source", ROWS);
}

#[test]
fn piped_text_an_interpreter_reads_as_data_is_not_source() {
    program_contract::partition("piped-data", ROWS);
}
