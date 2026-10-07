#[test]
fn loop_cwd_widening_preserves_protected_candidates() {
    crate::program_contract::partition(
        "cwd",
        include_str!("../fixtures/rust-loop-cwd-widening.json"),
    );
}
