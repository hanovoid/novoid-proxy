use async_stream::stream;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::{json, Map, Value};
use std::convert::Infallible;
use uuid::Uuid;

fn safe_json_parse(val: Option<&Value>) -> Value {
    match val {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or_else(|_| json!({})),
        Some(other) if !other.is_null() => other.clone(),
        _ => json!({}),
    }
}

fn convert_openai_content_parts(content: Option<&Value>) -> Value {
    let Some(c) = content else {
        return Value::String(String::new());
    };
    if let Some(s) = c.as_str() {
        return Value::String(s.to_string());
    }
    let Some(arr) = c.as_array() else {
        return Value::String(String::new());
    };

    let mut blocks = Vec::new();
    for part in arr {
        match part.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(txt) = part.get("text").and_then(|v| v.as_str()) {
                    blocks.push(json!({ "type": "text", "text": txt }));
                }
            }
            Some("image_url") => {
                if let Some(url) = part
                    .get("image_url")
                    .and_then(|v| v.get("url"))
                    .and_then(|v| v.as_str())
                {
                    if let Some(rest) = url.strip_prefix("data:") {
                        if let Some((meta, data)) = rest.split_once(";base64,") {
                            blocks.push(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": meta,
                                    "data": data
                                }
                            }));
                            continue;
                        }
                    }
                    blocks.push(json!({
                        "type": "image",
                        "source": {
                            "type": "url",
                            "url": url
                        }
                    }));
                }
            }
            _ => {}
        }
    }

    if blocks.is_empty() {
        Value::String(String::new())
    } else {
        Value::Array(blocks)
    }
}

pub fn open_ai_to_anthropic_request(oai_body: &Value) -> Value {
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();

    let push_user_blocks = |msgs: &mut Vec<Value>, new_blocks: Value| {
        let normalized: Vec<Value> = match new_blocks {
            Value::Array(arr) => arr,
            Value::String(s) => vec![json!({ "type": "text", "text": s })],
            other => vec![json!({ "type": "text", "text": other.to_string() })],
        };

        if let Some(last) = msgs.last_mut() {
            if last.get("role").and_then(|v| v.as_str()) == Some("user") {
                let mut prev = match last.get("content").cloned() {
                    Some(Value::Array(arr)) => arr,
                    Some(Value::String(s)) => vec![json!({ "type": "text", "text": s })],
                    _ => Vec::new(),
                };
                prev.extend(normalized);
                last["content"] = Value::Array(prev);
                return;
            }
        }

        msgs.push(json!({
            "role": "user",
            "content": normalized
        }));
    };

    if let Some(raw_msgs) = oai_body.get("messages").and_then(|v| v.as_array()) {
        for msg in raw_msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
            match role {
                "system" | "developer" => {
                    let txt = match msg.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(arr)) => arr
                            .iter()
                            .filter_map(|c| c.get("text").and_then(|v| v.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        _ => String::new(),
                    };
                    if !txt.is_empty() {
                        system_parts.push(txt);
                    }
                }
                "tool" => {
                    let result_txt = match msg.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(arr)) => arr
                            .iter()
                            .filter_map(|c| c.get("text").and_then(|v| v.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        Some(other) => other.to_string(),
                        None => String::new(),
                    };
                    let tool_use_id = msg
                        .get("tool_call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    push_user_blocks(
                        &mut messages,
                        json!([{
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": result_txt
                        }]),
                    );
                }
                "assistant" => {
                    let mut blocks = Vec::new();
                    match convert_openai_content_parts(msg.get("content")) {
                        Value::String(s) if !s.is_empty() => {
                            blocks.push(json!({ "type": "text", "text": s }));
                        }
                        Value::Array(arr) => {
                            blocks.extend(arr);
                        }
                        _ => {}
                    }
                    if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc in tcs {
                            let fn_obj = tc.get("function");
                            let name = fn_obj
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let input = safe_json_parse(fn_obj.and_then(|f| f.get("arguments")));
                            let id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| format!("toolu_{}", Uuid::new_v4().simple()));
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input
                            }));
                        }
                    }
                    let content_val = if blocks.is_empty() {
                        Value::String(String::new())
                    } else {
                        Value::Array(blocks)
                    };
                    messages.push(json!({
                        "role": "assistant",
                        "content": content_val
                    }));
                }
                "user" => {
                    let converted = convert_openai_content_parts(msg.get("content"));
                    push_user_blocks(&mut messages, converted);
                }
                _ => {}
            }
        }
    }

    if messages.is_empty() {
        messages.push(json!({ "role": "user", "content": "Hello" }));
    }

    let max_tokens = oai_body
        .get("max_completion_tokens")
        .or_else(|| oai_body.get("max_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(8192);

    let mut anth = Map::new();
    anth.insert(
        "model".to_string(),
        oai_body
            .get("model")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    anth.insert("max_tokens".to_string(), Value::from(max_tokens));
    anth.insert("messages".to_string(), Value::Array(messages));
    anth.insert(
        "stream".to_string(),
        Value::Bool(
            oai_body
                .get("stream")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        ),
    );

    if !system_parts.is_empty() {
        anth.insert(
            "system".to_string(),
            Value::String(system_parts.join("\n\n")),
        );
    }
    if let Some(temp) = oai_body.get("temperature").filter(|v| v.is_number()) {
        anth.insert("temperature".to_string(), temp.clone());
    }
    if let Some(top_p) = oai_body.get("top_p").filter(|v| v.is_number()) {
        anth.insert("top_p".to_string(), top_p.clone());
    }
    if let Some(stop) = oai_body.get("stop") {
        if stop.is_array() {
            anth.insert("stop_sequences".to_string(), stop.clone());
        } else if let Some(s) = stop.as_str() {
            anth.insert("stop_sequences".to_string(), json!([s]));
        }
    }
    if let Some(re) = oai_body.get("reasoning_effort") {
        anth.insert("reasoning_effort".to_string(), re.clone());
    }
    if let Some(r) = oai_body.get("reasoning") {
        anth.insert("reasoning".to_string(), r.clone());
    }

    let tool_choice_none = oai_body
        .get("tool_choice")
        .and_then(|v| v.as_str())
        == Some("none");
    if !tool_choice_none {
        if let Some(tools_arr) = oai_body.get("tools").and_then(|v| v.as_array()) {
            let mapped_tools: Vec<Value> = tools_arr
                .iter()
                .filter_map(|t| {
                    let fn_obj = t.get("function").unwrap_or(t);
                    let name = fn_obj.get("name").and_then(|v| v.as_str())?;
                    let desc = fn_obj
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let schema = fn_obj
                        .get("parameters")
                        .or_else(|| fn_obj.get("input_schema"))
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                    Some(json!({
                        "name": name,
                        "description": desc,
                        "input_schema": schema
                    }))
                })
                .collect();

            if !mapped_tools.is_empty() {
                anth.insert("tools".to_string(), Value::Array(mapped_tools));
                if let Some(tc) = oai_body.get("tool_choice") {
                    if tc.as_str() == Some("auto") {
                        anth.insert("tool_choice".to_string(), json!({ "type": "auto" }));
                    } else if tc.as_str() == Some("required") {
                        anth.insert("tool_choice".to_string(), json!({ "type": "any" }));
                    } else if tc.get("type").and_then(|v| v.as_str()) == Some("function") {
                        if let Some(name) = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            anth.insert(
                                "tool_choice".to_string(),
                                json!({ "type": "tool", "name": name }),
                            );
                        }
                    }
                }
            }
        }
    }

    Value::Object(anth)
}

pub fn anthropic_to_open_ai_response(anth_json: &Value, requested_model: &str) -> Value {
    let blocks = anth_json
        .get("content")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let text: String = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("");

    let tool_calls: Vec<Value> = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
        .map(|b| {
            let args = serde_json::to_string(b.get("input").unwrap_or(&json!({})))
                .unwrap_or_else(|_| "{}".to_string());
            json!({
                "id": b.get("id").cloned().unwrap_or(Value::String(String::new())),
                "type": "function",
                "function": {
                    "name": b.get("name").cloned().unwrap_or(Value::String(String::new())),
                    "arguments": args
                }
            })
        })
        .collect();

    let finish_reason = match anth_json.get("stop_reason").and_then(|v| v.as_str()) {
        Some("tool_use") => "tool_calls",
        Some("max_tokens") => "length",
        _ => "stop",
    };

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    if text.is_empty() && !tool_calls.is_empty() {
        message.insert("content".to_string(), Value::Null);
    } else {
        message.insert("content".to_string(), Value::String(text));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let input_tokens = anth_json
        .get("usage")
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached_tokens = anth_json
        .get("usage")
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = anth_json
        .get("usage")
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let prompt_tokens = input_tokens + cached_tokens;

    let mut usage = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": prompt_tokens + output_tokens
    });
    if cached_tokens > 0 {
        usage["prompt_tokens_details"] = json!({ "cached_tokens": cached_tokens });
    }

    let id = anth_json
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("chatcmpl-{}", Uuid::new_v4().simple()));
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": requested_model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason
        }],
        "usage": usage
    })
}

pub fn pipe_anthropic_stream_to_openai(
    upstream_res: reqwest::Response,
    requested_model: String,
) -> impl Stream<Item = Result<Bytes, Infallible>> + Send {
    stream! {
        yield Ok(Bytes::from_static(b": keepalive\n\n"));
        let mut byte_stream = upstream_res.bytes_stream();
        let mut buffer = Vec::<u8>::new();
        let id = format!("chatcmpl-{}", Uuid::new_v4().simple());
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut tool_call_idx: i64 = -1;
        let mut current_is_tool = false;
        let mut finish_reason = "stop".to_string();
        let mut usage_info: Option<Value> = None;

        while let Some(chunk_res) = byte_stream.next().await {
            let Ok(chunk) = chunk_res else {
                break;
            };
            buffer.extend_from_slice(&chunk);

            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line_bytes);
                let line = line.trim_end_matches(&['\r', '\n'][..]);
                if !line.starts_with("data:") {
                    continue;
                }
                let raw = line[5..].trim();
                if raw.is_empty() || raw == "[DONE]" {
                    continue;
                }
                let Ok(evt) = serde_json::from_str::<Value>(raw) else {
                    continue;
                };

                match evt.get("type").and_then(|v| v.as_str()) {
                    Some("content_block_start") => {
                        if evt.get("content_block").and_then(|c| c.get("type")).and_then(|v| v.as_str()) == Some("tool_use") {
                            tool_call_idx += 1;
                            current_is_tool = true;
                            let cb = &evt["content_block"];
                            let chunk = json!({
                                "id": id,
                                "object": "chat.completion.chunk",
                                "created": created,
                                "model": requested_model,
                                "choices": [{
                                    "index": 0,
                                    "delta": {
                                        "role": "assistant",
                                        "tool_calls": [{
                                            "index": tool_call_idx,
                                            "id": cb.get("id").cloned().unwrap_or(Value::Null),
                                            "type": "function",
                                            "function": {
                                                "name": cb.get("name").cloned().unwrap_or(Value::String(String::new())),
                                                "arguments": ""
                                            }
                                        }]
                                    },
                                    "finish_reason": Value::Null
                                }]
                            });
                            if let Ok(s) = serde_json::to_string(&chunk) {
                                yield Ok(Bytes::from(format!("data: {s}\n\n")));
                            }
                        } else {
                            current_is_tool = false;
                        }
                    }
                    Some("content_block_delta") => {
                        if evt.get("delta").and_then(|d| d.get("type")).and_then(|v| v.as_str()) == Some("text_delta") {
                            if let Some(txt) = evt.get("delta").and_then(|d| d.get("text")).and_then(|v| v.as_str()) {
                                let chunk = json!({
                                    "id": id,
                                    "object": "chat.completion.chunk",
                                    "created": created,
                                    "model": requested_model,
                                    "choices": [{
                                        "index": 0,
                                        "delta": { "content": txt },
                                        "finish_reason": Value::Null
                                    }]
                                });
                                if let Ok(s) = serde_json::to_string(&chunk) {
                                    yield Ok(Bytes::from(format!("data: {s}\n\n")));
                                }
                            }
                        } else if current_is_tool
                            && evt.get("delta").and_then(|d| d.get("type")).and_then(|v| v.as_str()) == Some("input_json_delta")
                        {
                            if let Some(pj) = evt.get("delta").and_then(|d| d.get("partial_json")).and_then(|v| v.as_str()) {
                                let chunk = json!({
                                    "id": id,
                                    "object": "chat.completion.chunk",
                                    "created": created,
                                    "model": requested_model,
                                    "choices": [{
                                        "index": 0,
                                        "delta": {
                                            "tool_calls": [{
                                                "index": tool_call_idx,
                                                "function": { "arguments": pj }
                                            }]
                                        },
                                        "finish_reason": Value::Null
                                    }]
                                });
                                if let Ok(s) = serde_json::to_string(&chunk) {
                                    yield Ok(Bytes::from(format!("data: {s}\n\n")));
                                }
                            }
                        }
                    }
                    Some("content_block_stop") => {
                        current_is_tool = false;
                    }
                    Some("message_delta") => {
                        match evt.get("delta").and_then(|d| d.get("stop_reason")).and_then(|v| v.as_str()) {
                            Some("tool_use") => finish_reason = "tool_calls".to_string(),
                            Some("max_tokens") => finish_reason = "length".to_string(),
                            Some(_) => finish_reason = "stop".to_string(),
                            None => {}
                        }
                        if let Some(u) = evt.get("usage") {
                            let inp = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                                + u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            let out = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            usage_info = Some(json!({
                                "prompt_tokens": inp,
                                "completion_tokens": out,
                                "total_tokens": inp + out
                            }));
                        }
                    }
                    Some("message_stop") => {
                        let mut end_chunk = json!({
                            "id": id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": requested_model,
                            "choices": [{
                                "index": 0,
                                "delta": {},
                                "finish_reason": finish_reason
                            }]
                        });
                        if let Some(ref u) = usage_info {
                            end_chunk["usage"] = u.clone();
                        }
                        if let Ok(s) = serde_json::to_string(&end_chunk) {
                            yield Ok(Bytes::from(format!("data: {s}\n\n")));
                        }
                    }
                    _ => {}
                }
            }
        }

        yield Ok(Bytes::from_static(b"data: [DONE]\n\n"));
    }
}
