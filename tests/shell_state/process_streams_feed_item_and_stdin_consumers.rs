const ROWS: &str = include_str!("../fixtures/rust-process-items.json");
#[test]
fn process_streams_feed_xargs_items() {
    crate::program_contract::partition("xargs", ROWS);
}
#[test]
fn process_streams_feed_list_file_items() {
    crate::program_contract::partition("list", ROWS);
}
#[test]
fn process_stream_data_does_not_read_named_paths() {
    crate::program_contract::partition("data", ROWS);
}
