#[path = "support/batch6.rs"]
mod batch7;

#[test]
fn global_option_values_never_consume_an_option() {
    batch7::partition("option", include_str!("fixtures/rust-batch7-global.json"));
}

#[test]
fn global_options_end_at_double_dash() {
    batch7::partition("boundary", include_str!("fixtures/rust-batch7-global.json"));
}

#[test]
fn global_names_remain_labelled_for_adapters() {
    batch7::partition("name", include_str!("fixtures/rust-batch7-global.json"));
    batch7::partition("owned", include_str!("fixtures/rust-batch7-global.json"));
}
