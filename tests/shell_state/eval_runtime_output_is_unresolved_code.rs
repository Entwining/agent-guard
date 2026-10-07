#[test]
fn eval_runtime_output_is_unresolved_code() {
    crate::program_contract::partition("output", include_str!("../fixtures/rust-eval-output.json"));
}
