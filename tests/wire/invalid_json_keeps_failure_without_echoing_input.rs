use agent_guard_rust::{CheckError, CheckErrorKind};
use std::io::{self, Read};

#[test]
fn invalid_json_keeps_failure_without_echoing_input() {
    let syntax = serde_json::from_str::<serde_json::Value>("{]").unwrap_err();
    let truncated = serde_json::from_str::<serde_json::Value>("[").unwrap_err();
    let data = serde_json::from_str::<String>("123").unwrap_err();
    for error in [syntax, truncated, data] {
        let error = CheckError::from(error);
        assert_eq!(error.kind, CheckErrorKind::MalformedInput);
        assert_eq!(error.to_string(), "invalid event input");
    }
}

#[test]
fn input_io_failure_stays_distinct_and_does_not_echo_the_source() {
    struct FailedInput;
    impl Read for FailedInput {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic private diagnostic"))
        }
    }
    let error = serde_json::from_reader::<_, serde_json::Value>(FailedInput).unwrap_err();
    let error = CheckError::from(error);
    assert_eq!(error.kind, CheckErrorKind::InputFailure);
    assert_eq!(error.to_string(), "event input could not be read");
    assert!(!format!("{error:?}").contains("synthetic private diagnostic"));
}
