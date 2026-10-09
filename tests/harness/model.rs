use super::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
    pub raw: Value,
}
#[derive(Default, Deserialize)]
struct ModelRequest {
    model: Option<String>,
    stream: Option<bool>,
    input: Option<Vec<Option<ResponseInput>>>,
    messages: Option<Vec<Option<MessageInput>>>,
}
#[derive(Deserialize)]
struct ResponseInput {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    output: Value,
}
#[derive(Deserialize)]
struct MessageInput {
    #[serde(default)]
    content: Value,
}
#[derive(Deserialize)]
struct ToolPart {
    #[serde(rename = "type")]
    kind: Option<String>,
    is_error: Option<bool>,
    #[serde(default)]
    content: Value,
}
#[derive(Deserialize)]
struct TextPart {
    text: Option<String>,
}
#[derive(Default)]
pub struct ScriptedModel {
    pub runtime: String,
    pub command: String,
    pub result: Option<ToolResult>,
    pub requests: usize,
    pub error: String,
}
impl ScriptedModel {
    pub fn begin(&mut self, runtime: &str, command: &str) {
        self.runtime = runtime.into();
        self.command = command.into();
        self.result = None;
        self.requests = 0;
        self.error.clear();
    }
    pub fn response(&mut self, method: &str, path: &str, body: &[u8]) -> Result<(String, String)> {
        if method != "POST" {
            return Ok(("application/json".into(), "{\"data\":[]}".into()));
        }
        self.requests += 1;
        let request = serde_json::from_slice::<Option<ModelRequest>>(body)?.unwrap_or_default();
        let model_name = request.model.as_deref().unwrap_or("");
        if self.runtime == "codex" {
            if let Some(input) = &request.input {
                for item in input.iter().flatten() {
                    if item.kind.as_deref() == Some("function_call_output") {
                        self.result = Some(ToolResult {
                            text: serde_json::from_value::<Option<String>>(item.output.clone())?
                                .unwrap_or_default(),
                            is_error: false,
                            raw: item.output.clone(),
                        });
                    }
                }
            }
            let item = if self.result.is_none() {
                json!({"type":"function_call","id":"fc_harness","call_id":"call_harness","name":"exec_command","arguments":json!({"cmd":self.command,"yield_time_ms":1000,"max_output_tokens":1000}).to_string(),"status":"completed"})
            } else {
                json!({"type":"message","id":"msg_harness","role":"assistant","status":"completed","content":[{"type":"output_text","text":"done","annotations":[]}]})
            };
            let response = json!({"id":format!("resp_harness_{}",self.requests),"object":"response","created_at":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),"status":"completed","model":model_name,"output":[item],"usage":{"input_tokens":10,"output_tokens":10,"total_tokens":20,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}});
            let mut started = response.clone();
            started["status"] = json!("in_progress");
            started["output"] = json!([]);
            let mut events = vec![
                json!({"type":"response.created","response":started}),
                json!({"type":"response.output_item.added","output_index":0,"item":item}),
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
                json!({"type":"response.completed","response":response}),
            ];
            for (i, event) in events.iter_mut().enumerate() {
                event["sequence_number"] = json!(i);
            }
            return Ok(("text/event-stream".into(), events_text(&events)));
        }
        if !path.contains("/messages") {
            return Ok(("application/json".into(), "{\"input_tokens\":10}".into()));
        }
        if let Some(Some(last)) = request
            .messages
            .as_ref()
            .and_then(|messages| messages.last())
            && let Some(parts) = last.content.as_array()
        {
            for raw in parts {
                let Ok(Some(part)) = serde_json::from_value::<Option<ToolPart>>(raw.clone()) else {
                    continue;
                };
                if part.kind.as_deref() != Some("tool_result") {
                    continue;
                }
                let text = match serde_json::from_value::<Option<String>>(part.content.clone()) {
                    Ok(text) => text.unwrap_or_default(),
                    Err(_) => {
                        serde_json::from_value::<Option<Vec<Option<TextPart>>>>(part.content)?
                            .unwrap_or_default()
                            .into_iter()
                            .flatten()
                            .filter_map(|block| block.text)
                            .collect()
                    }
                };
                self.result = Some(ToolResult {
                    text,
                    is_error: part.is_error.unwrap_or(false),
                    raw: raw.clone(),
                });
            }
        }
        let name = if self.runtime == "claude" {
            "Bash"
        } else {
            "bash"
        };
        let (content, stop) = if self.result.is_none() {
            (
                json!({"type":"tool_use","id":"tool_harness","name":name,"input":{"command":self.command}}),
                "tool_use",
            )
        } else {
            (json!({"type":"text","text":"done"}), "end_turn")
        };
        let message = json!({"id":"msg_harness","type":"message","role":"assistant","model":model_name,"content":[content],"stop_reason":stop,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":10}});
        if request.stream != Some(true) {
            return Ok(("application/json".into(), format!("{message}\n")));
        }
        let mut start = message;
        start["content"] = json!([]);
        start["stop_reason"] = Value::Null;
        let (block, delta) = if self.result.is_none() {
            (
                json!({"type":"tool_use","id":"tool_harness","name":name,"input":{}}),
                json!({"type":"input_json_delta","partial_json":content["input"].to_string()}),
            )
        } else {
            (
                json!({"type":"text","text":""}),
                json!({"type":"text_delta","text":"done"}),
            )
        };
        Ok((
            "text/event-stream".into(),
            events_text(&[
                json!({"type":"message_start","message":start}),
                json!({"type":"content_block_start","index":0,"content_block":block}),
                json!({"type":"content_block_delta","index":0,"delta":delta}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":{"output_tokens":10}}),
                json!({"type":"message_stop"}),
            ]),
        ))
    }
}
fn events_text(events: &[Value]) -> String {
    events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap_or("")
            )
        })
        .collect()
}

pub struct ModelServer {
    pub model: Arc<Mutex<ScriptedModel>>,
    pub url: String,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<()>>>,
}
impl ModelServer {
    pub fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let model = Arc::new(Mutex::new(ScriptedModel::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let state = Arc::clone(&model);
        let stop = Arc::clone(&stopping);
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false)?;
                        let response = (|| -> Result<(String, String)> {
                            let (method, path, body) = read_request(&mut stream)?;
                            state
                                .lock()
                                .map_err(|_| "model lock poisoned")?
                                .response(&method, &path, &body)
                        })();
                        let (status, content_type, body) = match response {
                            Ok((kind, body)) => ("200 OK", kind, body),
                            Err(e) => {
                                state.lock().map_err(|_| "model lock poisoned")?.error =
                                    e.to_string();
                                (
                                    "400 Bad Request",
                                    "text/plain".into(),
                                    "invalid model request\n".into(),
                                )
                            }
                        };
                        stream.set_write_timeout(Some(Duration::from_secs(20)))?;
                        if let Err(e) = write!(
                            stream,
                            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        ) {
                            state.lock().map_err(|_| "model lock poisoned")?.error = e.to_string();
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Ok(())
        });
        Ok(Self {
            model,
            url,
            stopping,
            worker: Some(worker),
        })
    }
    pub fn finish(&mut self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| "model server panicked")??;
        }
        Ok(())
    }
}
impl Drop for ModelServer {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
fn line(stream: &mut TcpStream, max: usize, deadline: Instant) -> Result<String> {
    let mut bytes = Vec::new();
    let mut byte = [0];
    loop {
        read_before(stream, &mut byte, deadline)?;
        bytes.push(byte[0]);
        if bytes.len() > max {
            return Err("HTTP header exceeds limit".into());
        }
        if bytes.ends_with(b"\r\n") {
            bytes.truncate(bytes.len() - 2);
            return Ok(String::from_utf8(bytes)?);
        }
    }
}
fn read_before(stream: &mut TcpStream, bytes: &mut [u8], deadline: Instant) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("HTTP request timed out")?;
        stream.set_read_timeout(Some(remaining))?;
        let read = stream.read(&mut bytes[offset..])?;
        if read == 0 {
            return Err("unexpected EOF".into());
        }
        offset += read;
    }
    Ok(())
}
fn read_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>)> {
    let started = Instant::now();
    let header_deadline = started + Duration::from_secs(5);
    let body_deadline = started + Duration::from_secs(20);
    let first = line(stream, 1 << 20, header_deadline)?;
    let mut fields = first.split_whitespace();
    let method = fields.next().ok_or("missing HTTP method")?.to_owned();
    let path = fields.next().ok_or("missing HTTP path")?.to_owned();
    let mut length = 0usize;
    let mut chunked = false;
    let mut expect = false;
    let mut header_bytes = first.len();
    loop {
        let header = line(stream, 1 << 20, header_deadline)?;
        header_bytes += header.len();
        if header_bytes > 1 << 20 {
            return Err("HTTP header exceeds limit".into());
        }
        if header.is_empty() {
            break;
        }
        let (key, value) = header.split_once(':').ok_or("invalid HTTP header")?;
        match key.to_ascii_lowercase().as_str() {
            "content-length" => length = value.trim().parse()?,
            "transfer-encoding" => chunked = value.trim().eq_ignore_ascii_case("chunked"),
            "expect" => expect = value.trim().eq_ignore_ascii_case("100-continue"),
            _ => {}
        }
    }
    if expect {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let mut bytes = Vec::new();
    if chunked {
        loop {
            let size = line(stream, 1 << 20, body_deadline)?;
            let size = usize::from_str_radix(size.split(';').next().ok_or("invalid chunk")?, 16)?;
            if size == 0 {
                while !line(stream, 1 << 20, body_deadline)?.is_empty() {}
                break;
            }
            if bytes.len().saturating_add(size) > 16 << 20 {
                return Err("model request exceeds limit".into());
            }
            let start = bytes.len();
            bytes.resize(start + size, 0);
            read_before(stream, &mut bytes[start..], body_deadline)?;
            if !line(stream, 2, body_deadline)?.is_empty() {
                return Err("invalid chunk ending".into());
            }
        }
    } else {
        if length > 16 << 20 {
            return Err("model request exceeds limit".into());
        }
        bytes.resize(length, 0);
        read_before(stream, &mut bytes, body_deadline)?;
    }
    Ok((method, path, bytes))
}
