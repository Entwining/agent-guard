#[path = "../../examples/evaluate.rs"]
mod interface;
use crate::support;

use agent_guard_rust::{
    Event, Outcome,
    adapters::{self, Consumer},
    evaluate_with_arm,
    filesystem::DiskProbe,
    shell::Arm,
};
use interface::{Mode, guard, handle_requests, request};
use serde_json::{Value, json};
use std::io;

#[test]
fn jsonl_dev_contract_and_parser_boundary() {
    let fixture = support::Fixture::new();
    let rows = support::rows();
    let ids = [
        "S01-read-public-claude",
        "S01-read-appdata-claude",
        "S18-json-claude",
    ];
    let mut input = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let row = rows.iter().find(|r| r["id"] == *id).unwrap();
        let mut request = json!({"id":id,"consumer":row["consumer"],"arm":"brush","home":fixture.home,"cwd":fixture.expand(support::text(row,"cwd"))});
        let body = fixture.body(row);
        if index == 1 {
            request["event"] = serde_json::from_slice(&body).unwrap();
        } else {
            request["event_raw"] = json!(String::from_utf8(body).unwrap());
        }
        serde_json::to_writer(&mut input, &request).unwrap();
        input.push(b'\n');
    }
    let mut output = Vec::new();
    handle_requests(Mode::Guard, io::Cursor::new(&input), &mut output).unwrap();
    let actual: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(actual.len(), ids.len());
    for (id, response) in ids.iter().zip(&actual) {
        let row = rows.iter().find(|r| r["id"] == *id).unwrap();
        assert_eq!(response["id"], *id);
        support::assert_preflight_tuple(row, response);
        let mut probe = DiskProbe;
        let result = evaluate_with_arm(
            Event {
                bytes: &fixture.body(row),
                context: &fixture.context(row),
                probe: &mut probe,
            },
            Arm::Brush,
        );
        let wire = adapters::render(Consumer::Claude, &result);
        assert_eq!(response["exit"], wire.exit);
        assert_eq!(response["stdout"], wire.stdout);
        assert_eq!(response["stderr"], wire.stderr);
        assert!(response["evaluate_ns"].as_u64().is_some());
        match result {
            Ok(e) => match e.outcome {
                Outcome::ProtectedDenial { reason, recovery } => {
                    assert_eq!(response["outcome"], "ProtectedDenial");
                    assert_eq!(response["reason"], reason.effect);
                    assert_eq!(response["recovery"], adapters::recovery_value(&recovery));
                    assert!(response["reason"].as_str().unwrap().contains("App Data"));
                }
                Outcome::NoObjection => {
                    assert_eq!(response["outcome"], "NoObjection");
                    assert!(response["reason"].is_null() && response["recovery"].is_null());
                }
                _ => panic!("unexpected dev outcome"),
            },
            Err(error) => {
                assert_eq!(error.kind, agent_guard_rust::CheckErrorKind::MalformedInput);
                assert_eq!(response["coverage"]["error_kind"], "MalformedInput");
                assert!(response["outcome"].is_null() && response["recovery"].is_null());
                assert_eq!(response["disposition"], "BlockOnCheckError");
            }
        }
        assert_eq!(response["advice"], json!([]));
    }

    let shell = json!({"id":"parser","consumer":"claude","arm":"brush","home":fixture.home,"cwd":fixture.project,"event":{"tool_name":"Bash","tool_input":{"command":format!("cat '{}'",fixture.container)}}});
    for arm in ["brush", "structured"] {
        let mut request = shell.clone();
        request["arm"] = json!(arm);
        let mut output = Vec::new();
        handle_requests(
            Mode::ParseOnly,
            io::Cursor::new(request.to_string()),
            &mut output,
        )
        .unwrap();
        let response: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            response["parse_status"],
            if arm == "structured" {
                "outside_arm_coverage"
            } else {
                "parsed"
            }
        );
        assert_eq!(response["parse_ns"].is_null(), arm == "structured");
        assert!(response.get("class").is_none() && response.get("evaluate_ns").is_none());
        if arm != "structured" {
            request["event"]["tool_input"]["command"] = json!("if then");
            let mut output = Vec::new();
            handle_requests(
                Mode::ParseOnly,
                io::Cursor::new(request.to_string()),
                &mut output,
            )
            .unwrap();
            let response: Value = serde_json::from_slice(&output).unwrap();
            assert_eq!(response["parse_status"], "parse_failed");
            assert!(response["parse_ns"].as_u64().is_some());
        }
    }
    let mut host_request = shell.clone();
    host_request["context"] = json!({"user":"fixture-user"});
    host_request["event"]["tool_input"]["command"] = json!("ls ~fixture-user/Library/Containers");
    assert_eq!(guard(&request(&host_request).unwrap())["class"], "D");
    host_request["context"]["user"] = json!(false);
    assert!(request(&host_request).is_err());
    host_request["context"] = json!({});
    host_request["arm"] = json!("tree");
    assert!(request(&host_request).is_err());

    let mut invalid = b"{\n{}\n\xff\n".to_vec();
    invalid.extend(input.split_inclusive(|byte| *byte == b'\n').next().unwrap());
    let mut output = Vec::new();
    handle_requests(Mode::Guard, io::Cursor::new(invalid), &mut output).unwrap();
    let responses: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 4);
    for response in &responses[..3] {
        assert_eq!(response["request_error"]["kind"], "MalformedRequest");
        assert!(response.get("class").is_none() && response.get("evaluate_ns").is_none());
    }
    assert_eq!(responses[3]["id"], ids[0]);
    assert_eq!(responses[3]["class"], "N");
}
