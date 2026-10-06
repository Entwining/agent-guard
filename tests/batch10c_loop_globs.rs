#[path = "support/batch6.rs"]
mod contract;

#[test]
fn shell_glob_loop_items_keep_protected_intersection_at_sinks() {
    contract::partition(
        "protected",
        include_str!("fixtures/rust-batch10c-loop-globs.json"),
    );
}

#[test]
fn public_shell_glob_items_remain_public() {
    contract::partition(
        "public",
        include_str!("fixtures/rust-batch10c-loop-globs.json"),
    );
}

#[test]
fn shell_glob_loop_cd_matches_direct_glob_cd() {
    contract::partition(
        "cwd",
        include_str!("fixtures/rust-batch10c-loop-globs.json"),
    );
}
