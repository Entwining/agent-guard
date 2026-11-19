#[test]
fn environment_templates_keep_the_example_exemption() {
    crate::program_contract::partition(
        "environment-template",
        include_str!("../fixtures/rust-environment-templates.json"),
    );
}
