use serde_json::{Value, json};
use std::{
    env, fs,
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
    let (mut name, mut input) = (String::new(), None);
    for line in first.lines().filter_map(|line| line.strip_prefix("data: ")) {
        let event: Value = serde_json::from_str(line)?;
        if let Some(tool) = event["content_block"]["name"].as_str() {
            name = tool.into();
        }
        if let Some(arguments) = event["item"]["arguments"].as_str() {
            input = Some(serde_json::from_str::<Value>(arguments)?);
        } else if let Some(partial) = event["delta"]["partial_json"].as_str() {
            input = Some(serde_json::from_str::<Value>(partial)?);
        }
    }
    let input = input.ok_or("model did not supply a tool call")?;
    let body = if runtime == "codex" {
        json!({"tool_name":"Bash","tool_input":{"command":input["cmd"]}})
    } else {
        json!({"tool_name":name,"tool_input":input})
    };
    let mut hook = Command::new(env::var("AGENT_GUARD_TEST_HELPER")?)
        .args(["--runtime", &runtime])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    hook.stdin
        .take()
        .ok_or("missing hook stdin")?
        .write_all(body.to_string().as_bytes())?;
    let result = hook.wait_with_output()?;
    let is_error = !result.status.success();
    let stderr = String::from_utf8(result.stderr)?;
    let text = match runtime.as_str() {
        _ if !is_error => execute(&name, &input)?,
        "codex" => format!("Command blocked by PreToolUse hook: {stderr}"),
        "pi" if stderr.trim().is_empty() => "[error:agent-guard/guard_unavailable] stand-in".into(),
        "claude" if result.status.code() != Some(2) => {
            "stand-in hook error: failed; blocking because onFailure is \"block\"".into()
        }
        _ => stderr,
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

/// Performs the call the way the client's tool would, reporting only what that
/// tool returns to the model.
fn execute(name: &str, input: &Value) -> Result<String, Box<dyn std::error::Error>> {
    let field = |key: &str| input[key].as_str().ok_or(format!("missing {key}"));
    let path = || field("file_path").or_else(|_| field("path"));
    let argv = match name {
        "Read" | "read" => return Ok(fs::read_to_string(path()?)?),
        "Write" | "write" => {
            fs::write(path()?, field("content")?)?;
            return Ok("written".into());
        }
        "Edit" | "edit" => {
            let path = path()?;
            let (old, new) = match name {
                "Edit" => (&input["old_string"], &input["new_string"]),
                _ => (&input["edits"][0]["oldText"], &input["edits"][0]["newText"]),
            };
            let content = fs::read_to_string(path)?;
            fs::write(
                path,
                content.replace(
                    old.as_str().ok_or("missing old text")?,
                    new.as_str().ok_or("missing new text")?,
                ),
            )?;
            return Ok("edited".into());
        }
        "Grep" | "grep" => vec!["/usr/bin/grep", "-r", field("pattern")?, path()?],
        _ => vec!["/bin/sh", "-c", field("command").or_else(|_| field("cmd"))?],
    };
    let output = Command::new(argv[0]).args(&argv[1..]).output()?;
    if !output.status.success() {
        return Err("synthetic command failed".into());
    }
    Ok(format!(
        "{}{}",
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?
    ))
}
