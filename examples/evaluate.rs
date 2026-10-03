//! Caller-materialized, offline evaluation; this example is never a hook entry.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use agent_guard_rust::{
    CheckError, CheckErrorKind, Context, Coverage, CoverageGap, Disposition, Event, Outcome,
    adapters::{self, Consumer, Operation},
    evaluate_with_arm,
    filesystem::DiskProbe,
    shell::Arm,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    io::{self, BufRead, Write},
    process::ExitCode,
    time::Instant,
};

#[derive(Clone, Copy)]
enum Mode {
    Guard,
    ParseOnly,
}

struct Request {
    id: String,
    arm: Arm,
    context: Context,
    bytes: Vec<u8>,
}

fn string(value: &Value, key: &str) -> Result<String, &'static str> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or("required string field is missing or invalid")
}

fn boolean(value: &Value, key: &str, default: bool) -> Result<bool, &'static str> {
    value
        .get(key)
        .map(|v| v.as_bool().ok_or("context boolean is invalid"))
        .transpose()
        .map(|v| v.unwrap_or(default))
}

fn request(value: &Value) -> Result<Request, &'static str> {
    let id = string(value, "id")?;
    let consumer = match string(value, "consumer")?.as_str() {
        "claude" => Consumer::Claude,
        "codex" => Consumer::Codex,
        "pi" => Consumer::Pi,
        _ => return Err("unknown consumer"),
    };
    let arm = match string(value, "arm")?.as_str() {
        "structured" => Arm::StructuredOnly,
        "brush" => Arm::Brush,
        "tree" => Arm::TreeSitter,
        _ => return Err("unknown arm"),
    };
    let home = string(value, "home")?;
    let cwd = string(value, "cwd")?;
    let bytes = match (value.get("event"), value.get("event_raw")) {
        (Some(event), None) if event.is_object() => {
            serde_json::to_vec(event).map_err(|_| "event object could not be encoded")?
        }
        (None, Some(raw)) => raw
            .as_str()
            .ok_or("event_raw must be a string")?
            .as_bytes()
            .to_vec(),
        _ => return Err("provide exactly one event object or event_raw string"),
    };
    let metadata = value.get("context").unwrap_or(&Value::Null);
    if !metadata.is_null() && !metadata.is_object() {
        return Err("context must be an object");
    }
    if metadata
        .as_object()
        .is_some_and(|fields| fields.keys().any(|key| key != "zsh_executor"))
    {
        return Err("context field has no production source");
    }
    Ok(Request {
        id,
        arm,
        context: Context {
            consumer,
            home,
            cwd,
            zsh_executor: boolean(metadata, "zsh_executor", consumer != Consumer::Pi)?,
            require_execution_owner: false,
            shell_observation_entries: Cell::new(0),
        },
        bytes,
    })
}

fn gap_name(gap: &CoverageGap) -> String {
    match gap {
        CoverageGap::UnknownProgram { program } => format!("UnknownProgram:{program}"),
        CoverageGap::OutsideObservedTool { tool } => format!("OutsideObservedTool:{tool}"),
        other => format!("{other:?}"),
    }
}

fn guard(request: &Request) -> Value {
    let mut probe = DiskProbe;
    let started = Instant::now();
    let result = evaluate_with_arm(
        Event {
            bytes: &request.bytes,
            context: &request.context,
            probe: &mut probe,
        },
        request.arm,
    );
    let wire = adapters::render(request.context.consumer, &result);
    let evaluate_ns = started.elapsed().as_nanos();
    let mut response = json!({
        "id":request.id, "outcome":null, "class":null, "coverage":null,
        "disposition":null, "cause":null, "reason":null, "advice":[], "recovery":null,
        "exit":wire.exit, "stdout":wire.stdout, "stderr":wire.stderr,
        "evaluate_ns":evaluate_ns
        ,"observed_effects":[]
    });
    match result {
        Err(error) => {
            response["class"] = json!("F");
            response["coverage"] =
                json!({"state":"NotCompleted","error_kind":format!("{:?}",error.kind)});
            response["disposition"] = json!("BlockOnCheckError");
            response["reason"] = json!(error.to_string());
        }
        Ok(evaluation) => {
            response["observed_effects"] = adapters::effects_value(&evaluation.effects);
            response["coverage"] = match evaluation.coverage {
                Coverage::SupportedPreflight => json!({"state":"SupportedPreflight"}),
                Coverage::LimitedPreflight(gaps) => {
                    json!({"state":"LimitedPreflight","gaps":gaps.iter().map(gap_name).collect::<Vec<_>>()})
                }
                Coverage::OutsideObservedToolCoverage { tool } => {
                    json!({"state":"OutsideObservedToolCoverage","tool_class":tool})
                }
            };
            match evaluation.outcome {
                Outcome::NoObjection => {
                    response["outcome"] = json!("NoObjection");
                    response["class"] = json!("N");
                }
                Outcome::SoftAdvice(advice) => {
                    response["outcome"] = json!("SoftAdvice");
                    response["class"] = json!("A");
                    response["advice"] =
                        json!(advice.iter().map(|a| &a.message).collect::<Vec<_>>());
                }
                Outcome::ProtectedDenial { reason, recovery } => {
                    response["outcome"] = json!("ProtectedDenial");
                    response["class"] = json!("D");
                    response["reason"] = json!(reason.effect);
                    response["recovery"] = adapters::recovery_value(&recovery);
                }
                Outcome::CoverageInsufficient {
                    cause,
                    disposition,
                    recovery,
                } => {
                    response["outcome"] = json!("CoverageInsufficient");
                    response["class"] = json!(match disposition {
                        Disposition::ContinueLimitedPreflight => "UC",
                        Disposition::RejectUnsupportedSyntax => "UR",
                        Disposition::RequireVerifiedExecutionOwner => "UO",
                    });
                    response["cause"] = json!(gap_name(&cause));
                    response["disposition"] = json!(format!("{disposition:?}"));
                    response["recovery"] = recovery
                        .as_ref()
                        .map(adapters::recovery_value)
                        .unwrap_or(Value::Null);
                }
            }
        }
    }
    response
}

fn parse_only(request: &Request) -> Value {
    let mut response = json!({"id":request.id,"parse_status":"outside_arm_coverage","parse_ns":null,"error_kind":null});
    if request.arm == Arm::StructuredOnly {
        return response;
    }
    let source = match adapters::decode(
        request.context.consumer,
        &request.bytes,
        &request.context.cwd,
    ) {
        Ok(event) => match event.operation {
            Operation::Shell(source) => source,
            _ => return response,
        },
        Err(error) => {
            response["parse_status"] = json!("input_error");
            response["error_kind"] = json!(format!("{:?}", error.kind));
            return response;
        }
    };
    let started = Instant::now();
    let parsed = match request.arm {
        Arm::Brush => Ok(brush_parser::Parser::builder()
            .build(io::Cursor::new(source.as_bytes()))
            .parse_program()
            .is_ok()),
        Arm::TreeSitter => {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_bash::LANGUAGE.into())
                .map_err(|_| CheckError {
                    kind: CheckErrorKind::GuardFault,
                })
                .and_then(|()| {
                    parser
                        .parse(&source, None)
                        .map(|tree| !tree.root_node().has_error())
                        .ok_or(CheckError {
                            kind: CheckErrorKind::GuardFault,
                        })
                })
        }
        Arm::StructuredOnly => unreachable!(),
    };
    response["parse_ns"] = json!(started.elapsed().as_nanos());
    response["parse_status"] = json!(match parsed {
        Ok(true) => "parsed",
        Ok(false) => "parse_failed",
        Err(error) => {
            response["error_kind"] = json!(format!("{:?}", error.kind));
            "parser_error"
        }
    });
    response
}

fn handle_requests(mode: Mode, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let value = serde_json::from_slice::<Value>(&line);
        let response = match value {
            Ok(value) => match request(&value) {
                Ok(request) => match mode {
                    Mode::Guard => guard(&request),
                    Mode::ParseOnly => parse_only(&request),
                },
                Err(message) => {
                    json!({"id":value.get("id").and_then(Value::as_str),"request_error":{"kind":"MalformedRequest","message":message}})
                }
            },
            Err(_) => {
                json!({"id":null,"request_error":{"kind":"MalformedRequest","message":"invalid request JSON"}})
            }
        };
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = match args.as_slice() {
        [] => Mode::Guard,
        [flag, mode] if flag == "--mode" && mode == "guard" => Mode::Guard,
        [flag, mode] if flag == "--mode" && mode == "parse-only" => Mode::ParseOnly,
        _ => {
            eprintln!("usage: evaluate [--mode guard|parse-only]");
            return ExitCode::FAILURE;
        }
    };
    if handle_requests(mode, io::stdin().lock(), io::stdout().lock()).is_err() {
        eprintln!("evaluation interface I/O failed");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use super::*;

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
                    assert_eq!(error.kind, CheckErrorKind::MalformedInput);
                    assert_eq!(response["coverage"]["error_kind"], "MalformedInput");
                    assert!(response["outcome"].is_null() && response["recovery"].is_null());
                    assert_eq!(response["disposition"], "BlockOnCheckError");
                }
            }
            assert_eq!(response["advice"], json!([]));
        }

        let shell = json!({"id":"parser","consumer":"claude","arm":"brush","home":fixture.home,"cwd":fixture.project,"event":{"tool_name":"Bash","tool_input":{"command":format!("cat '{}'",fixture.container)}}});
        for arm in ["brush", "tree", "structured"] {
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
}
