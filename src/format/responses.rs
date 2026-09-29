use serde::Deserialize;

use super::{sse::SseBuffer, Adapter, ParsedRequest, StreamAggregator, Usage};

#[derive(Deserialize, Default)]
struct RawUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
}

impl From<RawUsage> for Usage {
    fn from(u: RawUsage) -> Self {
        let total = if u.total_tokens > 0 {
            u.total_tokens
        } else {
            u.input_tokens + u.output_tokens
        };
        Usage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: total,
            ..Default::default()
        }
    }
}

/// Parses the top-level `model` and `stream` fields from a Responses API
/// request. `input` may be a string or array; neither is inspected here.
#[derive(Deserialize, Default)]
struct RawRequest {
    #[serde(default)]
    model: String,
    #[serde(default)]
    stream: bool,
}

#[derive(Deserialize, Default)]
struct RawResponse {
    #[serde(default)]
    usage: RawUsage,
}

pub struct ResponsesAdapter;

impl Adapter for ResponsesAdapter {
    fn parse_request(&self, body: &[u8]) -> ParsedRequest {
        let req: RawRequest = serde_json::from_slice(body).unwrap_or_default();
        ParsedRequest {
            model: req.model,
            stream: req.stream,
        }
    }

    fn parse_response(&self, body: &[u8]) -> Usage {
        let resp: RawResponse = serde_json::from_slice(body).unwrap_or_default();
        resp.usage.into()
    }

    fn new_stream_aggregator(&self) -> Box<dyn StreamAggregator> {
        Box::new(StreamState::default())
    }
}

/// SSE event types emitted by the Responses API streaming endpoint.
/// Only `response.completed` carries the final usage; other events carry
/// output text deltas which are not reconstructed here (raw SSE bytes
/// are captured separately for `response_body`).
#[derive(Deserialize, Default)]
struct StreamEvent {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    response: RawResponseInEvent,
}

#[derive(Deserialize, Default)]
struct RawResponseInEvent {
    #[serde(default)]
    usage: RawUsage,
}

#[derive(Default)]
struct StreamState {
    sse: SseBuffer,
    usage: Usage,
}

impl StreamAggregator for StreamState {
    fn feed(&mut self, chunk: &[u8]) {
        for payload in self.sse.feed(chunk) {
            let Ok(event) = serde_json::from_str::<StreamEvent>(&payload) else {
                continue;
            };
            // response.completed carries the final usage object.
            // response.done is an alias some versions emit.
            match event.kind.as_str() {
                "response.completed" | "response.done" => {
                    let u: Usage = event.response.usage.into();
                    if u.total_tokens > 0 || u.prompt_tokens > 0 || u.completion_tokens > 0 {
                        self.usage = u;
                    }
                }
                // Fallback: if any event has a top-level usage, accept it.
                _ => {}
            }
        }
    }

    fn finish(self: Box<Self>) -> Usage {
        self.usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request() {
        let got = ResponsesAdapter.parse_request(br#"{"model":"gpt-4o","stream":true,"input":"hello"}"#);
        assert_eq!(got.model, "gpt-4o");
        assert!(got.stream);
    }

    #[test]
    fn parse_request_array_input() {
        let got = ResponsesAdapter.parse_request(
            br#"{"model":"gpt-4o","stream":false,"input":[{"role":"user","content":"hi"}]}"#,
        );
        assert_eq!(got.model, "gpt-4o");
        assert!(!got.stream);
    }

    #[test]
    fn parse_request_malformed_yields_defaults() {
        let got = ResponsesAdapter.parse_request(b"not json");
        assert_eq!(got, ParsedRequest::default());
    }

    #[test]
    fn parse_response() {
        let body = br#"{"usage":{"input_tokens":10,"output_tokens":20,"total_tokens":30}}"#;
        let got = ResponsesAdapter.parse_response(body);
        assert_eq!(got.prompt_tokens, 10);
        assert_eq!(got.completion_tokens, 20);
        assert_eq!(got.total_tokens, 30);
    }

    #[test]
    fn parse_response_infers_total_when_zero() {
        let body = br#"{"usage":{"input_tokens":10,"output_tokens":20}}"#;
        let got = ResponsesAdapter.parse_response(body);
        assert_eq!(got.total_tokens, 30);
    }

    #[test]
    fn stream_aggregator_picks_up_response_completed() {
        let mut agg = ResponsesAdapter.new_stream_aggregator();
        agg.feed(
            b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":5,\"output_tokens\":10,\"total_tokens\":15}}}\n\n",
        );
        let usage = agg.finish();
        assert_eq!(usage.prompt_tokens, 5);
        assert_eq!(usage.completion_tokens, 10);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn stream_aggregator_picks_up_response_done_alias() {
        let mut agg = ResponsesAdapter.new_stream_aggregator();
        agg.feed(
            b"data: {\"type\":\"response.done\",\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":7,\"total_tokens\":10}}}\n\n",
        );
        let usage = agg.finish();
        assert_eq!(usage.total_tokens, 10);
    }

    #[test]
    fn stream_aggregator_ignores_delta_events() {
        let mut agg = ResponsesAdapter.new_stream_aggregator();
        agg.feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n");
        agg.feed(
            b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        );
        let usage = agg.finish();
        assert_eq!(usage.total_tokens, 3);
    }

    #[test]
    fn stream_aggregator_feed_split_across_chunks() {
        let mut agg = ResponsesAdapter.new_stream_aggregator();
        let full = b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":4,\"total_tokens\":6}}}\n\n";
        let mid = full.len() / 2;
        agg.feed(&full[..mid]);
        agg.feed(&full[mid..]);
        let usage = agg.finish();
        assert_eq!(usage.total_tokens, 6);
    }
}
