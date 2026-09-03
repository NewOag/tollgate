use serde::Deserialize;

use super::{parse_request_common, sse::SseBuffer, Adapter, ParsedRequest, StreamAggregator, Usage};

#[derive(Deserialize, Default)]
struct UsageDetails {
    #[serde(default)]
    cached_tokens: i64,
}

#[derive(Deserialize, Default)]
struct RawUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
    #[serde(default)]
    input_tokens_details: UsageDetails,
}

/// Like OpenAI Chat Completions' `prompt_tokens`, Responses API's
/// `input_tokens` is already inclusive of `input_tokens_details.cached_tokens`
/// — no additive normalization needed (contrast with the Anthropic
/// adapter, whose `input_tokens` excludes cache tokens).
impl From<RawUsage> for Usage {
    fn from(u: RawUsage) -> Self {
        Usage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.total_tokens,
            cache_read_tokens: u.input_tokens_details.cached_tokens,
            ..Default::default()
        }
    }
}

#[derive(Deserialize, Default)]
struct RawResponse {
    #[serde(default)]
    usage: RawUsage,
}

pub struct OpenAIResponsesAdapter;

impl Adapter for OpenAIResponsesAdapter {
    fn parse_request(&self, body: &[u8]) -> ParsedRequest {
        parse_request_common(body)
    }

    fn parse_response(&self, body: &[u8]) -> Usage {
        let resp: RawResponse = serde_json::from_slice(body).unwrap_or_default();
        resp.usage.into()
    }

    fn new_stream_aggregator(&self) -> Box<dyn StreamAggregator> {
        Box::new(StreamState::default())
    }
}

#[derive(Deserialize, Default)]
struct ResponseObject {
    #[serde(default)]
    usage: RawUsage,
}

#[derive(Deserialize, Default)]
struct StreamEvent {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    response: ResponseObject,
}

/// Unlike Chat Completions' single homogeneous chunk shape, the Responses
/// API SSE stream is a sequence of typed events (`response.created`,
/// `response.output_item.added`, `response.output_text.delta`, ...); only
/// the terminal `response.completed`/`response.incomplete` events carry
/// the full `response` object (and therefore `usage`), one level nested.
/// Everything else is ignored — the raw SSE bytes are captured separately
/// for `response_body`.
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
            if matches!(event.kind.as_str(), "response.completed" | "response.incomplete") {
                self.usage = event.response.usage.into();
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
        let got = OpenAIResponsesAdapter.parse_request(br#"{"model":"gpt-4o","stream":true}"#);
        assert_eq!(
            got,
            ParsedRequest {
                model: "gpt-4o".to_string(),
                stream: true
            }
        );
    }

    #[test]
    fn parse_request_malformed_yields_zero_value() {
        let got = OpenAIResponsesAdapter.parse_request(b"not json");
        assert_eq!(got, ParsedRequest::default());
    }

    #[test]
    fn parse_response() {
        let body = br#"{"usage":{"input_tokens":10,"output_tokens":20,"total_tokens":30}}"#;
        let got = OpenAIResponsesAdapter.parse_response(body);
        assert_eq!(
            got,
            Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                ..Default::default()
            }
        );
    }

    #[test]
    fn parse_response_with_cached_tokens() {
        let body = br#"{"usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120,"input_tokens_details":{"cached_tokens":80}}}"#;
        let got = OpenAIResponsesAdapter.parse_response(body);
        assert_eq!(
            got,
            Usage {
                prompt_tokens: 100,
                completion_tokens: 20,
                total_tokens: 120,
                cache_read_tokens: 80,
                ..Default::default()
            }
        );
    }

    /// Replays a realistic response.created -> output_item.added ->
    /// output_text.delta* -> output_item.done -> response.completed
    /// sequence (with `event:` lines present, as the real wire format
    /// sends) and checks that only the terminal response.completed
    /// event's nested usage is picked up.
    #[test]
    fn stream_aggregator_extracts_usage_from_response_completed() {
        let mut agg = OpenAIResponsesAdapter.new_stream_aggregator();
        let events = [
            ("response.created", r#"{"type":"response.created","response":{"id":"resp_1"}}"#),
            ("response.output_item.added", r#"{"type":"response.output_item.added","output_index":0}"#),
            ("response.output_text.delta", r#"{"type":"response.output_text.delta","delta":"Hel"}"#),
            ("response.output_text.delta", r#"{"type":"response.output_text.delta","delta":"lo"}"#),
            ("response.output_item.done", r#"{"type":"response.output_item.done","output_index":0}"#),
            (
                "response.completed",
                r#"{"type":"response.completed","response":{"id":"resp_1","usage":{"input_tokens":42,"output_tokens":7,"total_tokens":49}}}"#,
            ),
        ];
        for (event_type, data) in events {
            agg.feed(format!("event: {event_type}\ndata: {data}\n\n").as_bytes());
        }
        let usage = agg.finish();
        assert_eq!(
            usage,
            Usage {
                prompt_tokens: 42,
                completion_tokens: 7,
                total_tokens: 49,
                ..Default::default()
            }
        );
    }

    #[test]
    fn stream_aggregator_feed_split_across_chunks() {
        let mut agg = OpenAIResponsesAdapter.new_stream_aggregator();
        let full = "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n";
        let mid = full.len() / 2;
        agg.feed(&full.as_bytes()[..mid]);
        agg.feed(&full.as_bytes()[mid..]);
        let usage = agg.finish();
        assert_eq!(usage.total_tokens, 3);
    }

    #[test]
    fn stream_aggregator_skips_malformed_event() {
        let mut agg = OpenAIResponsesAdapter.new_stream_aggregator();
        agg.feed(b"data: not json\n\n");
        agg.feed(b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n");
        let usage = agg.finish();
        assert_eq!(usage.total_tokens, 2);
    }
}
