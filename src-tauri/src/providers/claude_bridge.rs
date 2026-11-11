//! Claude Code 只说 Anthropic Messages。上游是 OpenAI Chat / Responses 时，
//! 本地听 `127.0.0.1:15722`，把 `/v1/messages` 翻成上游的协议再翻回来。
//!
//! 模型 id 去掉厂商前缀后以 `claude-` 开头的，认为上游提供 `/messages`，原样转发。
//! 其余走转换：母版协议是 OpenAI Responses 就翻成 `/responses`，其余翻成 `/chat/completions`。
//!
//! 转换只在 agentcenter 窗口开着时生效。密钥不写进这个文件，
//! Claude Code 用 `ANTHROPIC_AUTH_TOKEN` 带过来，这里原样交给上游。

use super::model::ProviderKind;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Once;
use std::time::Duration;

pub const BRIDGE_PORT: u16 = 15722;

pub fn bridge_base_url() -> String {
    format!("http://127.0.0.1:{BRIDGE_PORT}")
}

pub fn config_path() -> PathBuf {
    super::store::home_dir()
        .join(".agents")
        .join("claude-bridge.json")
}

/// 去掉 `vendor/` 这类前缀之后，是不是 Claude 原生模型名。
pub fn model_speaks_messages(id: &str) -> bool {
    id.rsplit(|c| c == '/' || c == ':')
        .next()
        .unwrap_or(id)
        .trim()
        .to_ascii_lowercase()
        .starts_with("claude-")
}

/// 这一家要不要经本地转换再交给 Claude Code。
pub fn needs_bridge(kind: ProviderKind, model_ids: &[String]) -> bool {
    match kind {
        ProviderKind::Anthropic => model_ids.iter().any(|id| !model_speaks_messages(id)),
        ProviderKind::OpenaiChat | ProviderKind::OpenaiResponses | ProviderKind::GoogleGenAi => {
            true
        }
    }
}

pub fn bridge_file_content(upstream: &str, kind: ProviderKind) -> String {
    let mut s = serde_json::to_string_pretty(&json!({
        "upstream": upstream,
        "kind": kind.as_str(),
    }))
    .unwrap_or_else(|_| "{}".to_string());
    s.push('\n');
    s
}

/// 窗口进程里听一次。端口已被占用就放弃，不报错。
pub fn ensure_started() {
    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::spawn(|| {
            if let Err(e) = serve() {
                eprintln!("Claude 协议转换没听上：{e}");
            }
        });
    });
}

fn serve() -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", BRIDGE_PORT))?;
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        std::thread::spawn(move || {
            let _ = handle_conn(stream);
        });
    }
    Ok(())
}

struct Incoming {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn handle_conn(mut stream: TcpStream) -> std::io::Result<()> {
    let _ = stream.set_nodelay(true);
    let Some(req) = read_request(&mut stream)? else {
        return Ok(());
    };
    if req.method != "POST" || !is_messages_path(&req.path) {
        return write_json(
            &mut stream,
            404,
            &json!({"type":"error","error":{"type":"not_found_error","message":"只接受 POST /v1/messages"}}),
        );
    }
    let payload: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(e) => {
            return write_json(
                &mut stream,
                400,
                &json!({"type":"error","error":{"type":"invalid_request_error","message":e.to_string()}}),
            );
        }
    };
    let cfg = match read_bridge_file() {
        Ok(c) => c,
        Err(msg) => {
            return write_json(
                &mut stream,
                503,
                &json!({"type":"error","error":{"type":"api_error","message":msg}}),
            );
        }
    };
    let model = payload.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let route = route_for(model, &cfg.kind);
    if route == Route::Unsupported {
        return write_json(
            &mut stream,
            400,
            &json!({
                "type":"error",
                "error":{
                    "type":"invalid_request_error",
                    "message":"Google 原生协议还不能转成 Claude 的 Messages。请改用 OpenAI Chat 或 Responses。"
                }
            }),
        );
    }
    let key = incoming_key(&req.headers);
    let url = upstream_url(&cfg.upstream, route.path());
    let stream_mode = payload.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let outbound = match route {
        Route::Messages => payload.clone(),
        Route::Chat => to_chat_body(&payload),
        Route::Responses => to_responses_body(&payload),
        Route::Unsupported => unreachable!(),
    };
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(600))
        .build();
    let send = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json")
        .set("Accept-Encoding", "identity")
        .set("Authorization", &format!("Bearer {key}"))
        .set("x-api-key", &key)
        .set("anthropic-version", "2023-06-01");
    let text = match serde_json::to_string(&outbound) {
        Ok(t) => t,
        Err(e) => {
            return write_json(
                &mut stream,
                500,
                &json!({"type":"error","error":{"type":"api_error","message":e.to_string()}}),
            );
        }
    };
    match send.send_string(&text) {
        Ok(resp) => forward_ok(&mut stream, resp, route, model, stream_mode),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .or_else(|| v.pointer("/message"))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or(body);
            write_json(
                &mut stream,
                code,
                &json!({"type":"error","error":{"type":"api_error","message":msg}}),
            )
        }
        Err(e) => write_json(
            &mut stream,
            502,
            &json!({"type":"error","error":{"type":"api_error","message":e.to_string()}}),
        ),
    }
}

fn forward_ok(
    stream: &mut TcpStream,
    resp: ureq::Response,
    route: Route,
    model: &str,
    stream_mode: bool,
) -> std::io::Result<()> {
    if route == Route::Messages {
        if stream_mode {
            pipe_chunked(stream, resp.into_reader())
        } else {
            let body = resp.into_string().unwrap_or_default();
            write_raw(stream, 200, "application/json", body.as_bytes())
        }
    } else if stream_mode {
        let reader = resp.into_reader();
        let mut out = AnthropicStream::new(model);
        write_sse_headers(stream)?;
        let translated = std::io::BufReader::new(reader);
        for line in std::io::BufRead::lines(translated) {
            let Ok(line) = line else { break };
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            let piece = match route {
                Route::Chat => out.push_chat(data),
                Route::Responses => out.push_responses(data),
                _ => String::new(),
            };
            if !piece.is_empty() {
                write_chunk(stream, piece.as_bytes())?;
            }
        }
        let tail = out.finish();
        if !tail.is_empty() {
            write_chunk(stream, tail.as_bytes())?;
        }
        write_chunk(stream, b"")
    } else {
        let body = resp.into_string().unwrap_or_default();
        let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let converted = match route {
            Route::Chat => chat_response_to_anthropic(&parsed, model),
            Route::Responses => responses_response_to_anthropic(&parsed, model),
            _ => parsed,
        };
        write_json(stream, 200, &converted)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Messages,
    Chat,
    Responses,
    Unsupported,
}

impl Route {
    fn path(self) -> &'static str {
        match self {
            Route::Messages => "messages",
            Route::Chat => "chat/completions",
            Route::Responses => "responses",
            Route::Unsupported => "messages",
        }
    }
}

fn route_for(model: &str, kind: &str) -> Route {
    if kind == "google-generative-ai" && !model_speaks_messages(model) {
        return Route::Unsupported;
    }
    if model_speaks_messages(model) {
        return Route::Messages;
    }
    if kind == "openai-responses" {
        Route::Responses
    } else {
        Route::Chat
    }
}

/// base 已经以 `/v1` 结尾就不再加一段。
pub fn upstream_url(base: &str, route: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/{route}")
    } else {
        format!("{base}/v1/{route}")
    }
}

#[derive(serde::Deserialize)]
struct BridgeFile {
    upstream: String,
    kind: String,
}

fn read_bridge_file() -> Result<BridgeFile, String> {
    let path = config_path();
    let text = std::fs::read_to_string(&path).map_err(|_| {
        "还没有转换上游。先在 agentcenter 里把这个 provider 应用到 Claude。".to_string()
    })?;
    serde_json::from_str(&text).map_err(|e| format!("转换配置读不了：{e}"))
}

fn is_messages_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    path == "/v1/messages" || path == "/messages"
}

fn incoming_key(headers: &[(String, String)]) -> String {
    for (k, v) in headers {
        if k == "x-api-key" {
            return v.clone();
        }
    }
    for (k, v) in headers {
        if k == "authorization" {
            return v
                .strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .unwrap_or(v)
                .to_string();
        }
    }
    String::new()
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Incoming>> {
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        if buf.len() > 2 * 1024 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "请求头过大",
            ));
        }
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_sep(&buf) {
            break pos;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers.push((key, val));
        }
    }
    if content_length > 20 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "请求体过大",
        ));
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_length);
    Ok(Some(Incoming {
        method,
        path,
        headers,
        body,
    }))
}

fn find_sep(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn write_json(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_string());
    write_raw(stream, status, "application/json", text.as_bytes())
}

fn write_raw(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = reason_phrase(status);
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn write_sse_headers(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
    )?;
    stream.flush()
}

fn pipe_chunked(stream: &mut TcpStream, mut reader: impl Read) -> std::io::Result<()> {
    write_sse_headers(stream)?;
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        write_chunk(stream, &buf[..n])?;
    }
    write_chunk(stream, b"")
}

fn write_chunk(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    if data.is_empty() {
        stream.write_all(b"0\r\n\r\n")?;
    } else {
        write!(stream, "{:X}\r\n", data.len())?;
        stream.write_all(data)?;
        stream.write_all(b"\r\n")?;
    }
    stream.flush()
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Anthropic Messages 请求 → OpenAI Chat Completions 请求。
pub fn to_chat_body(src: &Value) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = src.get("system") {
        let text = flatten_text(system);
        if !text.is_empty() {
            messages.push(json!({"role":"system","content":text}));
        }
    }
    if let Some(arr) = src.get("messages").and_then(|v| v.as_array()) {
        for msg in arr {
            push_chat_message(&mut messages, msg);
        }
    }
    let mut out = Map::new();
    if let Some(model) = src.get("model") {
        out.insert("model".into(), model.clone());
    }
    out.insert("messages".into(), Value::Array(messages));
    if let Some(max) = src.get("max_tokens") {
        out.insert("max_tokens".into(), max.clone());
    }
    for key in ["temperature", "top_p", "stream"] {
        if let Some(v) = src.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    if let Some(stop) = src.get("stop_sequences") {
        out.insert("stop".into(), stop.clone());
    }
    if let Some(tools) = src.get("tools").and_then(|v| v.as_array()) {
        let converted: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").cloned().unwrap_or(Value::String(String::new())),
                        "description": t.get("description").cloned().unwrap_or(Value::String(String::new())),
                        "parameters": t.get("input_schema").cloned().unwrap_or(json!({"type":"object","properties":{}}))
                    }
                })
            })
            .collect();
        out.insert("tools".into(), Value::Array(converted));
    }
    if let Some(choice) = src.get("tool_choice") {
        out.insert("tool_choice".into(), chat_tool_choice(choice));
    }
    Value::Object(out)
}

fn push_chat_message(out: &mut Vec<Value>, msg: &Value) {
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let content = msg.get("content").cloned().unwrap_or(Value::Null);
    if role == "assistant" {
        let text = flatten_text(&content);
        let mut calls = Vec::new();
        if let Some(blocks) = content.as_array() {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                    let args = block.get("input").cloned().unwrap_or(json!({}));
                    calls.push(json!({
                        "id": block.get("id").cloned().unwrap_or(Value::String(String::new())),
                        "type": "function",
                        "function": {
                            "name": block.get("name").cloned().unwrap_or(Value::String(String::new())),
                            "arguments": serde_json::to_string(&args).unwrap_or_else(|_| "{}".into())
                        }
                    }));
                }
            }
        }
        let mut assistant = Map::new();
        assistant.insert("role".into(), json!("assistant"));
        if !text.is_empty() {
            assistant.insert("content".into(), Value::String(text));
        } else if calls.is_empty() {
            assistant.insert("content".into(), Value::String(String::new()));
        }
        if !calls.is_empty() {
            assistant.insert("tool_calls".into(), Value::Array(calls));
        }
        out.push(Value::Object(assistant));
        return;
    }
    if let Some(blocks) = content.as_array() {
        let mut text_parts = Vec::new();
        for block in blocks {
            let ty = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ty == "tool_result" {
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": block.get("tool_use_id").cloned().unwrap_or(Value::String(String::new())),
                    "content": flatten_text(block.get("content").unwrap_or(&Value::Null))
                }));
            } else if ty == "text" {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(t.to_string());
                }
            }
        }
        if !text_parts.is_empty() {
            out.push(json!({"role":"user","content":text_parts.join("")}));
        } else if blocks.is_empty() {
            out.push(json!({"role":"user","content":""}));
        }
        return;
    }
    out.push(json!({"role":"user","content":flatten_text(&content)}));
}

fn chat_tool_choice(choice: &Value) -> Value {
    match choice.get("type").and_then(|v| v.as_str()) {
        Some("any") => json!("required"),
        Some("tool") => json!({
            "type": "function",
            "function": {"name": choice.get("name").and_then(|v| v.as_str()).unwrap_or("")}
        }),
        Some("none") => json!("none"),
        _ => json!("auto"),
    }
}

fn flatten_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// OpenAI Chat 完整回复 → Anthropic Messages 回复。
pub fn chat_response_to_anthropic(src: &Value, fallback_model: &str) -> Value {
    let choice = src
        .pointer("/choices/0")
        .cloned()
        .unwrap_or(Value::Null);
    let message = choice.get("message").cloned().unwrap_or(Value::Null);
    let mut blocks = Vec::new();
    if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            blocks.push(json!({"type":"text","text":text}));
        }
    }
    if let Some(calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for call in calls {
            let args = call
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let input = serde_json::from_str::<Value>(args).unwrap_or(json!({"raw": args}));
            blocks.push(json!({
                "type": "tool_use",
                "id": call.get("id").cloned().unwrap_or(json!("call")),
                "name": call.pointer("/function/name").cloned().unwrap_or(json!("")),
                "input": input
            }));
        }
    }
    let finish = choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("stop");
    anthropic_message(
        src.get("id").and_then(|v| v.as_str()).unwrap_or("msg_bridge"),
        src.get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_model),
        blocks,
        stop_reason(finish),
        src.pointer("/usage/prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        src.pointer("/usage/completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    )
}

fn anthropic_message(
    id: &str,
    model: &str,
    blocks: Vec<Value>,
    stop_reason: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Value {
    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": blocks,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens}
    })
}

fn stop_reason(finish: &str) -> &'static str {
    match finish {
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        _ => "end_turn",
    }
}

/// Anthropic Messages 请求 → OpenAI Responses 请求。
pub fn to_responses_body(src: &Value) -> Value {
    let chat = to_chat_body(src);
    let mut input = Vec::new();
    let mut instructions = None;
    if let Some(msgs) = chat.get("messages").and_then(|v| v.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            if role == "system" {
                instructions = msg.get("content").cloned();
                continue;
            }
            if role == "tool" {
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": msg.get("tool_call_id").cloned().unwrap_or(json!("")),
                    "output": msg.get("content").cloned().unwrap_or(json!(""))
                }));
                continue;
            }
            if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        input.push(json!({"role":"assistant","content":text}));
                    }
                }
                for call in calls {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.get("id").cloned().unwrap_or(json!("")),
                        "name": call.pointer("/function/name").cloned().unwrap_or(json!("")),
                        "arguments": call.pointer("/function/arguments").cloned().unwrap_or(json!("{}"))
                    }));
                }
                continue;
            }
            input.push(json!({
                "role": role,
                "content": msg.get("content").cloned().unwrap_or(json!(""))
            }));
        }
    }
    let mut out = Map::new();
    if let Some(model) = src.get("model") {
        out.insert("model".into(), model.clone());
    }
    out.insert("input".into(), Value::Array(input));
    if let Some(ins) = instructions {
        out.insert("instructions".into(), ins);
    }
    if let Some(max) = src.get("max_tokens") {
        out.insert("max_output_tokens".into(), max.clone());
    }
    if let Some(stream) = src.get("stream") {
        out.insert("stream".into(), stream.clone());
    }
    if let Some(tools) = chat.get("tools").and_then(|v| v.as_array()) {
        let converted: Vec<Value> = tools
            .iter()
            .filter_map(|t| t.get("function").cloned())
            .map(|f| {
                json!({
                    "type": "function",
                    "name": f.get("name").cloned().unwrap_or(json!("")),
                    "description": f.get("description").cloned().unwrap_or(json!("")),
                    "parameters": f.get("parameters").cloned().unwrap_or(json!({"type":"object"}))
                })
            })
            .collect();
        out.insert("tools".into(), Value::Array(converted));
    }
    Value::Object(out)
}

pub fn responses_response_to_anthropic(src: &Value, fallback_model: &str) -> Value {
    let mut blocks = Vec::new();
    let mut text = String::new();
    if let Some(items) = src.get("output").and_then(|v| v.as_array()) {
        for item in items {
            let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ty == "message" {
                if let Some(parts) = item.get("content").and_then(|v| v.as_array()) {
                    for part in parts {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
            } else if ty == "function_call" {
                let args = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let input = serde_json::from_str::<Value>(args).unwrap_or(json!({"raw": args}));
                blocks.push(json!({
                    "type": "tool_use",
                    "id": item.get("call_id").cloned().unwrap_or(json!("call")),
                    "name": item.get("name").cloned().unwrap_or(json!("")),
                    "input": input
                }));
            }
        }
    }
    if text.is_empty() {
        if let Some(t) = src.get("output_text").and_then(|v| v.as_str()) {
            text = t.to_string();
        }
    }
    if !text.is_empty() {
        blocks.insert(0, json!({"type":"text","text":text}));
    }
    let stop = if blocks.iter().any(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
    {
        "tool_use"
    } else {
        "end_turn"
    };
    anthropic_message(
        src.get("id").and_then(|v| v.as_str()).unwrap_or("msg_bridge"),
        src.get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_model),
        blocks,
        stop,
        src.pointer("/usage/input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        src.pointer("/usage/output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    )
}

struct ToolBuf {
    id: String,
    name: String,
    args: String,
}

struct AnthropicStream {
    model: String,
    started: bool,
    text_open: bool,
    index: u32,
    tools: BTreeMap<u64, ToolBuf>,
    closed: bool,
    input_tokens: u64,
    output_tokens: u64,
}

impl AnthropicStream {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            started: false,
            text_open: false,
            index: 0,
            tools: BTreeMap::new(),
            closed: false,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    fn push_chat(&mut self, data: &str) -> String {
        if data == "[DONE]" {
            return self.finish();
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return String::new();
        };
        if let Some(n) = v.pointer("/usage/prompt_tokens").and_then(|x| x.as_u64()) {
            self.input_tokens = n;
        }
        if let Some(n) = v
            .pointer("/usage/completion_tokens")
            .and_then(|x| x.as_u64())
        {
            self.output_tokens = n;
        }
        let Some(choice) = v.pointer("/choices/0") else {
            return String::new();
        };
        let mut out = String::new();
        if let Some(delta) = choice.get("delta") {
            if let Some(text) = delta.get("content").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    out.push_str(&self.ensure_text());
                    out.push_str(&sse(
                        "content_block_delta",
                        &json!({
                            "type":"content_block_delta",
                            "index":0,
                            "delta":{"type":"text_delta","text":text}
                        }),
                    ));
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                for call in calls {
                    let idx = call.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                    let buf = self.tools.entry(idx).or_insert_with(|| ToolBuf {
                        id: String::new(),
                        name: String::new(),
                        args: String::new(),
                    });
                    if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                        buf.id = id.to_string();
                    }
                    if let Some(name) = call.pointer("/function/name").and_then(|v| v.as_str()) {
                        buf.name.push_str(name);
                    }
                    if let Some(args) = call
                        .pointer("/function/arguments")
                        .and_then(|v| v.as_str())
                    {
                        buf.args.push_str(args);
                    }
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            out.push_str(&self.close(stop_reason(reason)));
        }
        out
    }

    fn push_responses(&mut self, data: &str) -> String {
        if data == "[DONE]" {
            return self.finish();
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return String::new();
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty == "response.output_text.delta" {
            let text = v.get("delta").and_then(|t| t.as_str()).unwrap_or("");
            if text.is_empty() {
                return String::new();
            }
            let mut out = self.ensure_text();
            out.push_str(&sse(
                "content_block_delta",
                &json!({
                    "type":"content_block_delta",
                    "index":0,
                    "delta":{"type":"text_delta","text":text}
                }),
            ));
            return out;
        }
        if ty == "response.completed" || ty == "response.incomplete" {
            if let Some(resp) = v.get("response") {
                if let Some(n) = resp.pointer("/usage/input_tokens").and_then(|x| x.as_u64()) {
                    self.input_tokens = n;
                }
                if let Some(n) = resp.pointer("/usage/output_tokens").and_then(|x| x.as_u64()) {
                    self.output_tokens = n;
                }
                if let Some(items) = resp.get("output").and_then(|o| o.as_array()) {
                    for item in items {
                        if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                            let idx = self.tools.len() as u64;
                            self.tools.insert(
                                idx,
                                ToolBuf {
                                    id: item
                                        .get("call_id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("call")
                                        .to_string(),
                                    name: item
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    args: item
                                        .get("arguments")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("{}")
                                        .to_string(),
                                },
                            );
                        }
                    }
                }
            }
            let reason = if self.tools.is_empty() {
                "end_turn"
            } else {
                "tool_use"
            };
            return self.close(reason);
        }
        String::new()
    }

    fn finish(&mut self) -> String {
        if self.closed {
            return String::new();
        }
        let reason = if self.tools.is_empty() {
            "end_turn"
        } else {
            "tool_use"
        };
        self.close(reason)
    }

    fn ensure_text(&mut self) -> String {
        let mut out = String::new();
        if !self.started {
            out.push_str(&self.message_start());
        }
        if !self.text_open {
            out.push_str(&sse(
                "content_block_start",
                &json!({
                    "type":"content_block_start",
                    "index": self.index,
                    "content_block":{"type":"text","text":""}
                }),
            ));
            self.text_open = true;
        }
        out
    }

    fn message_start(&mut self) -> String {
        self.started = true;
        sse(
            "message_start",
            &json!({
                "type":"message_start",
                "message":{
                    "id":"msg_bridge",
                    "type":"message",
                    "role":"assistant",
                    "content":[],
                    "model": self.model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage":{"input_tokens": self.input_tokens, "output_tokens": 0}
                }
            }),
        )
    }

    fn close(&mut self, stop: &str) -> String {
        if self.closed {
            return String::new();
        }
        let mut out = String::new();
        if !self.started {
            out.push_str(&self.message_start());
        }
        if self.text_open {
            out.push_str(&sse(
                "content_block_stop",
                &json!({"type":"content_block_stop","index":0}),
            ));
            self.text_open = false;
            self.index += 1;
        }
        let tools: Vec<ToolBuf> = std::mem::take(&mut self.tools).into_values().collect();
        for tool in tools {
            let idx = self.index;
            self.index += 1;
            out.push_str(&sse(
                "content_block_start",
                &json!({
                    "type":"content_block_start",
                    "index": idx,
                    "content_block":{
                        "type":"tool_use",
                        "id": tool.id,
                        "name": tool.name,
                        "input": {}
                    }
                }),
            ));
            out.push_str(&sse(
                "content_block_delta",
                &json!({
                    "type":"content_block_delta",
                    "index": idx,
                    "delta":{"type":"input_json_delta","partial_json": tool.args}
                }),
            ));
            out.push_str(&sse(
                "content_block_stop",
                &json!({"type":"content_block_stop","index": idx}),
            ));
        }
        out.push_str(&sse(
            "message_delta",
            &json!({
                "type":"message_delta",
                "delta":{"stop_reason": stop, "stop_sequence": null},
                "usage":{"output_tokens": self.output_tokens}
            }),
        ));
        out.push_str(&sse(
            "message_stop",
            &json!({"type":"message_stop"}),
        ));
        self.closed = true;
        out
    }
}

fn sse(event: &str, data: &Value) -> String {
    format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(data).unwrap_or_else(|_| "{}".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_style_ids() {
        assert!(model_speaks_messages("claude-sonnet-5-5"));
        assert!(model_speaks_messages("claude-haiku-4-5-20251001"));
        assert!(!model_speaks_messages("deepseek/deepseek-v4.1-flash"));
        assert!(!model_speaks_messages("gpt-5.5"));
        assert!(!model_speaks_messages("deepseek/deepseek-v4-flash-fast"));
        assert!(model_speaks_messages("anthropic/claude-opus-5"));
    }

    #[test]
    fn bridge_only_when_a_model_cannot_speak_messages() {
        assert!(!needs_bridge(
            ProviderKind::Anthropic,
            &["claude-sonnet-5-5".into()]
        ));
        assert!(needs_bridge(
            ProviderKind::Anthropic,
            &["deepseek/deepseek-v4.1-flash".into()]
        ));
        assert!(needs_bridge(ProviderKind::OpenaiChat, &[]));
        assert!(needs_bridge(ProviderKind::OpenaiResponses, &[]));
    }

    #[test]
    fn command_base_does_not_double_v1() {
        assert_eq!(
            upstream_url("https://api.commandcode.ai/provider/v1", "messages"),
            "https://api.commandcode.ai/provider/v1/messages"
        );
        assert_eq!(
            upstream_url("https://api.commandcode.ai/provider/v1/", "chat/completions"),
            "https://api.commandcode.ai/provider/v1/chat/completions"
        );
        assert_eq!(
            upstream_url("https://api.example.com", "messages"),
            "https://api.example.com/v1/messages"
        );
    }

    #[test]
    fn chat_request_carries_system_and_tools() {
        let src = json!({
            "model": "deepseek/deepseek-v4.1-flash",
            "max_tokens": 32,
            "system": "be brief",
            "messages": [{"role":"user","content":"hi"}],
            "tools": [{
                "name": "echo",
                "description": "Echo",
                "input_schema": {"type":"object","properties":{"text":{"type":"string"}}}
            }]
        });
        let out = to_chat_body(&src);
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["messages"][0]["content"], "be brief");
        assert_eq!(out["messages"][1]["content"], "hi");
        assert_eq!(out["tools"][0]["function"]["name"], "echo");
        assert_eq!(out["max_tokens"], 32);
    }

    #[test]
    fn chat_response_becomes_anthropic_message() {
        let src = json!({
            "id": "chatcmpl_1",
            "model": "m",
            "choices": [{"finish_reason":"stop","message":{"role":"assistant","content":"pong"}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 1}
        });
        let out = chat_response_to_anthropic(&src, "m");
        assert_eq!(out["content"][0]["text"], "pong");
        assert_eq!(out["stop_reason"], "end_turn");
        assert_eq!(out["usage"]["input_tokens"], 3);
    }

    #[test]
    fn chat_sse_text_is_anthropic_events() {
        let mut s = AnthropicStream::new("m");
        let piece = s.push_chat(r#"{"choices":[{"delta":{"content":"hi"},"finish_reason":null}]}"#);
        assert!(piece.contains("text_delta"));
        assert!(piece.contains("hi"));
        let end = s.push_chat(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        assert!(end.contains("message_stop"));
        assert!(end.contains("end_turn"));
    }
}
