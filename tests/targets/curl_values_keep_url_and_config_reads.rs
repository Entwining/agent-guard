use crate::program_contract as contract;

#[test]
fn curl_values_keep_url_and_config_reads() {
    contract::partition(
        "curl-values",
        include_str!("../fixtures/rust-curl-values.json"),
    );
}
