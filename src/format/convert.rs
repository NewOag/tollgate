//! Bidirectional request/response body translation between the OpenAI
//! Chat Completions API and the OpenAI Responses API.
//!
//! # Chat → Responses (request)
//! Translates a `/v1/chat/completions` request body so it can be sent to
//! a `/v1/responses` upstream endpoint. Message `content` strings are
//! wrapped in a `{"type":"input_text","text":"..."}` block; multi-part
//! content arrays are preserved with appropriate type renames.
//!
//! # Responses → Chat (request) — inverse direction
//! Translates a `/v1/responses` request body to `/v1/chat/completions`.
//! `input` may be a bare string (becomes a single `user` message) or an
//! array of message objects.
//!
//! # Response body conversion (non-streaming)
//! Both directions convert the full JSON response body, mapping the
//! relevant fields (text, usage, stop reason) to the target schema.
//!
//! # SSE stream conversion
//! Both directions convert SSE event payloads on the fly. The raw bytes
//! relayed to the client are the *converted* bytes; the response_body
//! stored in the database therefore reflects what the client actually saw.

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ─── Chat Completions schema fragments ───────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatContentPart {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ChatContent>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<Value>,
    #[serde(flatten)]
    pub extra: Value,
}

// ─── Responses API schema fragments ──────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    String(String),
    Messages(Vec<ResponsesMessage>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponsesMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ResponsesContentPart>>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponsesContentPart {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: ResponsesInput,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: Value,
}

// ─── Chat → Responses ─────────────────────────────────────────────────────────

/// Translates an OpenAI Chat Completions request body to the Responses API
/// request format. Returns the translated bytes, or the original bytes
/// unchanged if parsing fails (so the upstream gets something rather than
/// nothing, and can return its own error).
pub fn chat_request_to_responses(body: &[u8]) -> Bytes {
    let Ok(req) = serde_json::from_slice::<ChatRequest>(body) else {
        return Bytes::copy_from_slice(body);
    };

    let input: Vec<Value> = req
        .messages
        .iter()
        .map(|msg| {
            let content_parts: Vec<Value> = match &msg.content {
                None => vec![],
                Some(ChatContent::Text(s)) => {
                    let part_type = if msg.role == "assistant" { "output_text" } else { "input_text" };
                    vec![json!({"type": part_type, "text": s})]
                }
                Some(ChatContent::Parts(parts)) => parts
                    .iter()
                    .map(|p| {
                        if p.kind == "text" {
                            let part_type = if msg.role == "assistant" { "output_text" } else { "input_text" };
                            json!({"type": part_type, "text": p.text})
                        } else {
                            // image_url and other parts pass through as-is
                            json!({"type": p.kind, "text": p.text})
                        }
                    })
                    .collect(),
            };
            json!({
                "role": msg.role,
                "content": content_parts,
            })
        })
        .collect();

    let mut out = json!({
        "model": req.model,
        "input": input,
        "stream": req.stream,
    });

    // Forward unknown extra fields (tools, temperature, max_tokens, etc.)
    if let Value::Object(extra) = req.extra {
        if let Value::Object(obj) = &mut out {
            for (k, v) in extra {
                // skip fields we've already mapped
                if !matches!(k.as_str(), "model" | "messages" | "stream" | "stream_options") {
                    obj.insert(k, v);
                }
            }
        }
    }

    Bytes::from(out.to_string())
}

/// Translates a Responses API request body to the Chat Completions format.
/// Returns the translated bytes, or the original bytes unchanged if parsing
/// fails.
pub fn responses_request_to_chat(body: &[u8]) -> Bytes {
    let Ok(req) = serde_json::from_slice::<ResponsesRequest>(body) else {
        return Bytes::copy_from_slice(body);
    };

    let messages: Vec<Value> = match &req.input {
        ResponsesInput::String(s) => {
            vec![json!({"role": "user", "content": s})]
        }
        ResponsesInput::Messages(msgs) => msgs
            .iter()
            .map(|msg| {
                // Collapse content parts back to a plain string when there's
                // exactly one text part, otherwise keep as an array.
                let content: Value = match &msg.content {
                    None => Value::Null,
                    Some(parts) if parts.len() == 1 && (parts[0].kind == "input_text" || parts[0].kind == "output_text") => {
                        Value::String(parts[0].text.clone())
                    }
                    Some(parts) => {
                        let arr: Vec<Value> = parts
                            .iter()
                            .map(|p| {
                                let kind = if p.kind == "input_text" || p.kind == "output_text" { "text" } else { &p.kind };
                                json!({"type": kind, "text": p.text})
                            })
                            .collect();
                        Value::Array(arr)
                    }
                };
                json!({"role": msg.role, "content": content})
            })
            .collect(),
    };

    let mut out = json!({
        "model": req.model,
        "messages": messages,
        "stream": req.stream,
    });

    if let Value::Object(extra) = req.extra {
        if let Value::Object(obj) = &mut out {
            for (k, v) in extra {
                if !matches!(k.as_str(), "model" | "input" | "stream") {
                    obj.insert(k, v);
                }
            }
        }
    }

    Bytes::from(out.to_string())
}

// ─── Non-streaming response conversion ───────────────────────────────────────

/// Converts a complete Responses API response body to a Chat Completions
/// response body. Returns the original bytes on any parse failure.
pub fn responses_response_to_chat(body: &[u8]) -> Bytes {
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        return Bytes::copy_from_slice(body);
    };

    let id = val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let model = val.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let created = val.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0);

    // Extract text from output[].content[].text
    let text = extract_text_from_responses_output(&val);
    let finish_reason = val
        .get("incomplete_details")
        .map(|_| "length")
        .unwrap_or_else(|| val.get("status").and_then(|v| v.as_str()).unwrap_or("stop"));
    let finish_reason = if finish_reason == "completed" { "stop" } else { finish_reason };

    let usage = val.get("usage").cloned().unwrap_or(Value::Null);
    let chat_usage = if usage.is_null() {
        Value::Null
    } else {
        json!({
            "prompt_tokens": usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            "completion_tokens": usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            "total_tokens": usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        })
    };

    let out = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": text,
            },
            "finish_reason": finish_reason,
        }],
        "usage": chat_usage,
    });

    Bytes::from(out.to_string())
}

/// Converts a Chat Completions response body to a Responses API response
/// body. Returns the original bytes on any parse failure.
pub fn chat_response_to_responses(body: &[u8]) -> Bytes {
    let Ok(val) = serde_json::from_slice::<Value>(body) else {
        return Bytes::copy_from_slice(body);
    };

    let id = val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let model = val.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let created = val.get("created").and_then(|v| v.as_i64()).unwrap_or(0);

    let text = val
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let finish_reason = val
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(|f| f.as_str())
        .unwrap_or("stop")
        .to_string();

    let status = if finish_reason == "stop" { "completed" } else { &finish_reason };

    let usage_val = val.get("usage").cloned().unwrap_or(Value::Null);
    let resp_usage = if usage_val.is_null() {
        Value::Null
    } else {
        json!({
            "input_tokens": usage_val.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            "output_tokens": usage_val.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            "total_tokens": usage_val.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        })
    };

    let out = json!({
        "id": id,
        "object": "response",
        "created_at": created,
        "model": model,
        "status": status,
        "output": [{
            "type": "message",
            "id": "",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text}],
            "status": status,
        }],
        "usage": resp_usage,
    });

    Bytes::from(out.to_string())
}

fn extract_text_from_responses_output(val: &Value) -> String {
    val.get("output")
        .and_then(|o| o.as_array())
        .and_then(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if item.get("type").and_then(|t| t.as_str()) == Some("message") {
                        item.get("content")
                            .and_then(|c| c.as_array())
                            .map(|parts| {
                                parts
                                    .iter()
                                    .filter_map(|p| {
                                        if p.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                            p.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join("")
                            })
                    } else {
                        None
                    }
                })
                .next()
        })
        .unwrap_or_default()
}

// ─── SSE stream conversion ────────────────────────────────────────────────────

/// Converts a single SSE event payload (the JSON after "data: ") from the
/// Responses API streaming format to the Chat Completions streaming format.
/// Returns `None` when the event should be suppressed (e.g., the terminal
/// `[DONE]` for Responses which has no equivalent in Chat SSE, or events
/// that don't map to Chat events). An empty `Some("")` means pass-through.
pub fn responses_sse_to_chat(payload: &str) -> Option<String> {
    if payload.trim_start() == "[DONE]" {
        return Some("[DONE]".to_string());
    }
    let Ok(val) = serde_json::from_str::<Value>(payload) else {
        return None;
    };
    let kind = val.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match kind {
        "response.created" | "response.in_progress" => {
            // Map to an empty delta chunk so the client knows the stream started
            let model = val.get("response").and_then(|r| r.get("model")).and_then(|m| m.as_str()).unwrap_or("");
            let id = val.get("response").and_then(|r| r.get("id")).and_then(|i| i.as_str()).unwrap_or("");
            let chunk = json!({
                "id": id,
                "object": "chat.completion.chunk",
                "model": model,
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}],
            });
            Some(chunk.to_string())
        }
        "response.output_text.delta" => {
            let delta = val.get("delta").and_then(|d| d.as_str()).unwrap_or("");
            let item_id = val.get("item_id").and_then(|i| i.as_str()).unwrap_or("");
            let chunk = json!({
                "id": item_id,
                "object": "chat.completion.chunk",
                "model": "",
                "choices": [{"index": 0, "delta": {"content": delta}, "finish_reason": null}],
            });
            Some(chunk.to_string())
        }
        "response.completed" | "response.done" => {
            let response = val.get("response").cloned().unwrap_or(Value::Null);
            let usage = response.get("usage").cloned().unwrap_or(Value::Null);
            let id = response.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let model = response.get("model").and_then(|m| m.as_str()).unwrap_or("");
            let chat_usage = if usage.is_null() {
                Value::Null
            } else {
                json!({
                    "prompt_tokens": usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                    "completion_tokens": usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                    "total_tokens": usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                })
            };
            // Final delta with finish_reason
            let chunk = json!({
                "id": id,
                "object": "chat.completion.chunk",
                "model": model,
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                "usage": chat_usage,
            });
            Some(chunk.to_string())
        }
        "response.output_text.done" | "response.output_item.done" | "response.content_part.done"
        | "response.content_part.added" | "response.output_item.added" => {
            // Internal bookkeeping events — suppress from chat stream
            None
        }
        "response.failed" | "response.incomplete" => {
            // Pass through as a terminal error chunk
            let chunk = json!({
                "id": "",
                "object": "chat.completion.chunk",
                "model": "",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            });
            Some(chunk.to_string())
        }
        _ => None,
    }
}

/// Converts a single SSE event payload from the Chat Completions streaming
/// format to the Responses API streaming format.
pub fn chat_sse_to_responses(payload: &str) -> Option<String> {
    if payload.trim_start() == "[DONE]" {
        return Some("[DONE]".to_string());
    }
    let Ok(val) = serde_json::from_str::<Value>(payload) else {
        return None;
    };

    let id = val.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
    let model = val.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string();
    let choices = val.get("choices").and_then(|c| c.as_array());

    let Some(choices) = choices else {
        return None;
    };
    let Some(choice) = choices.first() else {
        return None;
    };

    let delta = choice.get("delta").cloned().unwrap_or(Value::Null);
    let finish_reason = choice.get("finish_reason").and_then(|f| f.as_str());

    // Role delta — emit response.created
    if delta.get("role").is_some() {
        let event = json!({
            "type": "response.created",
            "response": {
                "id": id,
                "object": "response",
                "model": model,
                "status": "in_progress",
            }
        });
        return Some(event.to_string());
    }

    // Content delta
    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            let event = json!({
                "type": "response.output_text.delta",
                "item_id": id,
                "output_index": 0,
                "content_index": 0,
                "delta": content,
            });
            return Some(event.to_string());
        }
    }

    // Final chunk with finish_reason
    if finish_reason.is_some() {
        let usage = val.get("usage").cloned().unwrap_or(Value::Null);
        let resp_usage = if usage.is_null() {
            json!(null)
        } else {
            json!({
                "input_tokens": usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                "output_tokens": usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                "total_tokens": usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            })
        };
        let event = json!({
            "type": "response.completed",
            "response": {
                "id": id,
                "object": "response",
                "model": model,
                "status": "completed",
                "usage": resp_usage,
            }
        });
        return Some(event.to_string());
    }

    None
}

// ─── Path rewriting ───────────────────────────────────────────────────────────

/// Rewrites a Chat Completions path to the Responses API equivalent.
/// `/v1/chat/completions` → `/v1/responses`; other paths pass through.
pub fn chat_path_to_responses(path: &str) -> &str {
    if path == "/v1/chat/completions" {
        "/v1/responses"
    } else {
        path
    }
}

/// Rewrites a Responses API path to the Chat Completions equivalent.
/// `/v1/responses` → `/v1/chat/completions`; other paths pass through.
pub fn responses_path_to_chat(path: &str) -> &str {
    if path == "/v1/responses" {
        "/v1/chat/completions"
    } else {
        path
    }
}

// ─── SseConvertBuffer ─────────────────────────────────────────────────────────

use super::sse::SseBuffer;

/// Direction of request/response body translation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BodyTransform {
    /// No translation needed.
    None,
    /// Client sent Chat Completions; upstream expects Responses API.
    ChatToResponses,
    /// Client sent Responses API; upstream expects Chat Completions.
    ResponsesToChat,
}

/// Wraps an [`SseBuffer`] and converts each complete SSE event payload
/// to the target protocol format before re-encoding it as `data: …\n\n`
/// bytes that can be relayed directly to the client.
pub struct SseConvertBuffer {
    sse: SseBuffer,
}

impl SseConvertBuffer {
    pub fn new() -> Self {
        SseConvertBuffer { sse: SseBuffer::default() }
    }

    /// Feeds `chunk` into the internal SSE buffer, converts each complete
    /// event payload according to `transform`, and returns the translated
    /// `data: …\n\n` bytes to relay to the client.
    pub fn feed_and_convert(&mut self, chunk: &[u8], transform: BodyTransform) -> Bytes {
        let payloads = self.sse.feed(chunk);
        if payloads.is_empty() {
            return Bytes::new();
        }
        let mut out = Vec::new();
        for payload in payloads {
            let converted = match transform {
                BodyTransform::ChatToResponses => chat_sse_to_responses(&payload),
                BodyTransform::ResponsesToChat => responses_sse_to_chat(&payload),
                BodyTransform::None => Some(payload),
            };
            if let Some(translated) = converted {
                out.extend_from_slice(b"data: ");
                out.extend_from_slice(translated.as_bytes());
                out.extend_from_slice(b"\n\n");
            }
        }
        Bytes::from(out)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_to_responses_basic() {
        let body = br#"{"model":"gpt-4o","messages":[{"role":"user","content":"hello"}],"stream":false}"#;
        let out = chat_request_to_responses(body);
        let val: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(val["model"], "gpt-4o");
        assert_eq!(val["stream"], false);
        let input = &val["input"];
        assert!(input.is_array());
        let first = &input[0];
        assert_eq!(first["role"], "user");
        assert_eq!(first["content"][0]["type"], "input_text");
        assert_eq!(first["content"][0]["text"], "hello");
    }

    #[test]
    fn chat_request_to_responses_assistant_message() {
        let body = br#"{"model":"gpt-4o","messages":[{"role":"assistant","content":"hi"}],"stream":false}"#;
        let out = chat_request_to_responses(body);
        let val: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(val["input"][0]["content"][0]["type"], "output_text");
    }

    #[test]
    fn responses_request_to_chat_string_input() {
        let body = br#"{"model":"gpt-4o","input":"hello","stream":false}"#;
        let out = responses_request_to_chat(body);
        let val: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(val["messages"][0]["role"], "user");
        assert_eq!(val["messages"][0]["content"], "hello");
    }

    #[test]
    fn responses_request_to_chat_message_input() {
        let body = br#"{"model":"gpt-4o","input":[{"role":"user","content":[{"type":"input_text","text":"hi"}]}],"stream":false}"#;
        let out = responses_request_to_chat(body);
        let val: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(val["messages"][0]["role"], "user");
        assert_eq!(val["messages"][0]["content"], "hi");
    }

    #[test]
    fn responses_response_to_chat_basic() {
        let body = json!({
            "id": "resp_1",
            "model": "gpt-4o",
            "status": "completed",
            "output": [{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],
            "usage": {"input_tokens":5,"output_tokens":3,"total_tokens":8},
        });
        let out = responses_response_to_chat(body.to_string().as_bytes());
        let val: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(val["choices"][0]["message"]["content"], "hello");
        assert_eq!(val["usage"]["prompt_tokens"], 5);
        assert_eq!(val["usage"]["completion_tokens"], 3);
    }

    #[test]
    fn chat_response_to_responses_basic() {
        let body = json!({
            "id": "chatcmpl_1",
            "model": "gpt-4o",
            "choices": [{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],
            "usage": {"prompt_tokens":3,"completion_tokens":2,"total_tokens":5},
        });
        let out = chat_response_to_responses(body.to_string().as_bytes());
        let val: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(val["output"][0]["content"][0]["text"], "hi");
        assert_eq!(val["usage"]["input_tokens"], 3);
        assert_eq!(val["usage"]["output_tokens"], 2);
    }

    #[test]
    fn responses_sse_to_chat_delta() {
        let payload = r#"{"type":"response.output_text.delta","item_id":"item_1","delta":"hello"}"#;
        let out = responses_sse_to_chat(payload).unwrap();
        let val: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(val["choices"][0]["delta"]["content"], "hello");
    }

    #[test]
    fn responses_sse_to_chat_completed() {
        let payload = r#"{"type":"response.completed","response":{"id":"r1","model":"gpt-4o","usage":{"input_tokens":2,"output_tokens":5,"total_tokens":7}}}"#;
        let out = responses_sse_to_chat(payload).unwrap();
        let val: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(val["choices"][0]["finish_reason"], "stop");
        assert_eq!(val["usage"]["prompt_tokens"], 2);
    }

    #[test]
    fn chat_sse_to_responses_content_delta() {
        let payload = r#"{"id":"chatcmpl_1","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}"#;
        let out = chat_sse_to_responses(payload).unwrap();
        let val: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(val["type"], "response.output_text.delta");
        assert_eq!(val["delta"], "hi");
    }

    #[test]
    fn chat_sse_to_responses_done_passes_through() {
        let out = chat_sse_to_responses("[DONE]").unwrap();
        assert_eq!(out, "[DONE]");
    }

    #[test]
    fn chat_path_to_responses_rewrites_completions() {
        assert_eq!(chat_path_to_responses("/v1/chat/completions"), "/v1/responses");
        assert_eq!(chat_path_to_responses("/v1/models"), "/v1/models");
    }

    #[test]
    fn responses_path_to_chat_rewrites_responses() {
        assert_eq!(responses_path_to_chat("/v1/responses"), "/v1/chat/completions");
        assert_eq!(responses_path_to_chat("/v1/models"), "/v1/models");
    }
}
