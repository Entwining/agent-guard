#[path = "support/fixture_paths.rs"]
mod fixture_paths;
#[path = "../tests/support/harness.rs"]
mod support;

fn main() {
    for row in support::rows()
        .iter()
        .filter(|r| support::is_evaluator_row(r))
    {
        println!("{}", support::run(row));
    }
}
