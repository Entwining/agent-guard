use crate::program_contract as contract;
use agent_guard_rust::Outcome;

const ROWS: &str = include_str!("../fixtures/rust-curl-trace-credentials.json");

#[test]
fn traces_of_anonymous_requests_are_allowed() {
    contract::partition("anonymous", ROWS);
}

#[test]
fn traces_of_requests_with_credentials_refuse() {
    contract::partition_with("credentials", ROWS, |row, evaluation| {
        let Outcome::ProtectedDenial { reason, .. } = &evaluation.outcome else {
            panic!("{}: {:?}", row["id"], evaluation.outcome);
        };
        assert_eq!(reason.rule.id(), "curl_trace", "{}", row["id"]);
        assert_eq!(reason.trigger.as_deref(), Some("curl"), "{}", row["id"]);
    });
}
