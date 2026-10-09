use serde_json::{Value, json};
use std::{
    env,
    io::{Read, Write},
    net::TcpStream,
    process::{Command, Stdio},
    time::Duration,
};

fn post(url: &str, body: &Value) -> Result<String, Box<dyn std::error::Error>> {
    let address = url.strip_prefix("http://").ok_or("invalid model URL")?;
    let (host, path) = address.split_once('/').ok_or("missing request path")?;
    let mut stream = TcpStream::connect(host)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let encoded = body.to_string();
    write!(
        stream,
        "POST /{path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{encoded}",
        encoded.len()
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or("missing response body")?;
    if !headers.contains("200 OK") {
        return Err("model request failed".into());
    }
    if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut decoded = String::new();
        let mut rest = body;
        loop {
            let (size, next) = rest.split_once("\r\n").ok_or("missing chunk size")?;
            let size = usize::from_str_radix(size, 16)?;
            if size == 0 {
                break;
            }
            decoded.push_str(next.get(..size).ok_or("truncated chunk")?);
            rest = next.get(size + 2..).ok_or("missing chunk ending")?;
        }
        Ok(decoded)
    } else {
        Ok(body.into())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = env::args().nth(1).ok_or("missing runtime")?;
    if env::var("ANTHROPIC_API_KEY")? != "synthetic-not-a-credential" {
        return Err("synthetic environment missing".into());
    }
    let url = format!(
        "{}/v1/{}",
        env::var("AGENT_GUARD_TEST_MODEL_URL")?,
        if runtime == "codex" {
            "responses"
        } else {
            "messages"
        }
    );
    let mut request = if runtime == "codex" {
        json!({"model":"synthetic","input":[]})
    } else {
        json!({"model":"synthetic","stream":true,"messages":[{"role":"user","content":"Run"}]})
    };
    let first = post(&url, &request)?;
    let mut command = None;
    for line in first.lines().filter_map(|line| line.strip_prefix("data: ")) {
        let event: Value = serde_json::from_str(line)?;
        if runtime == "codex" {
            if let Some(args) = event["item"]["arguments"].as_str() {
                command = serde_json::from_str::<Value>(args)?["cmd"]
                    .as_str()
                    .map(String::from);
            }
        } else if let Some(partial) = event["delta"]["partial_json"].as_str() {
            command = serde_json::from_str::<Value>(partial)?["command"]
                .as_str()
                .map(String::from);
        }
    }
    let command = command.ok_or("model did not supply command")?;
    let mut hook = Command::new(env::var("AGENT_GUARD_TEST_HELPER")?)
        .args(["--hook", &runtime])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let body = json!({"tool_name":"Bash","tool_input":{"command":command}}).to_string();
    hook.stdin
        .take()
        .ok_or("missing hook stdin")?
        .write_all(body.as_bytes())?;
    let result = hook.wait_with_output()?;
    let is_error = !result.status.success();
    let text = if is_error {
        format!(
            "{}{}",
            if runtime == "codex" {
                "Command blocked by PreToolUse hook: "
            } else {
                ""
            },
            String::from_utf8(result.stderr)?
        )
    } else {
        let output = Command::new("/bin/sh").args(["-c", &command]).output()?;
        if !output.status.success() {
            return Err("synthetic command failed".into());
        }
        format!(
            "{}{}",
            String::from_utf8(output.stdout)?,
            String::from_utf8(output.stderr)?
        )
    };
    if runtime == "codex" {
        request["input"] = json!([{"type":"function_call_output","output":text}]);
    } else {
        request["messages"] = json!([{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool_harness","is_error":is_error,"content":text}]}]);
    }
    if !post(&url, &request)?.contains("done") {
        return Err("model did not complete".into());
    }
    Ok(())
}
