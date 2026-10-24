use agent_guard_rust::{Coverage, CoverageGap};

const ROWS: &str = include_str!("../fixtures/rust-client-subcommands.json");

#[test]
fn cargo_subcommands_keep_operand_roles() {
    crate::program_contract::partition("cargo", ROWS);
}

#[test]
fn produced_items_take_the_client_operand_role() {
    crate::program_contract::partition("items", ROWS);
}

#[test]
fn query_verbs_name_volumes_and_records() {
    crate::program_contract::partition("queries", ROWS);
}

/// The client stays an unknown program apart from the slots it names.
fn keeps_unknown_program_gap(name: &str) {
    crate::program_contract::partition_with(name, ROWS, |row, evaluation| {
        if let Some(program) = row["gap"].as_str() {
            assert!(
                matches!(&evaluation.coverage, Coverage::LimitedPreflight(gaps)
                    if gaps.contains(&CoverageGap::UnknownProgram { program: program.into() })),
                "{}: {:?}",
                row["id"],
                evaluation.coverage
            );
        }
    });
}

#[test]
fn rex_method_and_action_names_are_not_paths() {
    keeps_unknown_program_gap("rex-names");
}

#[test]
fn rex_scripts_payloads_and_unparsed_words_keep_read_roles() {
    keeps_unknown_program_gap("rex-reads");
}

#[test]
fn name_slots_keep_independent_shell_effects() {
    keeps_unknown_program_gap("name-slot-effects");
}

#[test]
fn sessidx_queries_and_cwd_filters_are_not_paths() {
    keeps_unknown_program_gap("sessidx-names");
}

#[test]
fn sessidx_transcripts_files_and_unparsed_words_keep_read_roles() {
    keeps_unknown_program_gap("sessidx-reads");
}
