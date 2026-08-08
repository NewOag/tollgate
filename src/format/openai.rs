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
    prompt_tokens: i64,
    #[serde(default)]
    completion_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
    #[serde(default)]
    prompt_tokens_details: UsageDetails,
}

impl From<RawUsage> for Usage {
    fn from(u: RawUsage) -> Self {
        Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            cache_read_tokens: u.prompt_tokens_details.cached_tokens,
            ..Default::default()
        }
    }
}

#[derive(Deserialize, Default)]
struct RawResponse {
    #[serde(default)]
    usage: RawUsage,
}

pub struct OpenAIAdapter;

impl Adapter for OpenAIAdapter {
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
struct StreamEvent {
    #[serde(default)]
    usage: Option<RawUsage>,
}

/// Picks up the trailing usage object OpenAI sends across
/// `chat.completion.chunk` events when the client requests
/// `stream_options.include_usage`. The generated text itself is not
/// reconstructed here — the raw SSE bytes are captured separately for
/// `response_body`.
#[derive(Default)]
struct StreamState {
    sse: SseBuffer,
    usage: Usage,
}

impl StreamAggregator for StreamState {
    fn feed(&mut self, chunk: &[u8]) {
        for payload in self.sse.feed(chunk) {
            if payload == "[DONE]" {
                continue;
            }
            let Ok(event) = serde_json::from_str::<StreamEvent>(&payload) else {
                continue;
            };
            if let Some(usage) = event.usage {
                self.usage = usage.into();
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
        let got = OpenAIAdapter.parse_request(br#"{"model":"gpt-4o","stream":true}"#);
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
        let got = OpenAIAdapter.parse_request(b"not json");
        assert_eq!(got, ParsedRequest::default());
    }

    #[test]
    fn parse_response() {
        let body = br#"{"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#;
        let got = OpenAIAdapter.parse_response(body);
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
        let body = br#"{"usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120,"prompt_tokens_details":{"cached_tokens":80}}}"#;
        let got = OpenAIAdapter.parse_response(body);
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

    #[test]
    fn stream_aggregator_picks_up_cached_tokens() {
        let mut agg = OpenAIAdapter.new_stream_aggregator();
        agg.feed(b"data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5,\"total_tokens\":105,\"prompt_tokens_details\":{\"cached_tokens\":90}}}\n\n");
        let usage = agg.finish();
        assert_eq!(
            usage,
            Usage {
                prompt_tokens: 100,
                completion_tokens: 5,
                total_tokens: 105,
                cache_read_tokens: 90,
                ..Default::default()
            }
        );
    }

    #[test]
    fn stream_aggregator_picks_up_trailing_usage() {
        let mut agg = OpenAIAdapter.new_stream_aggregator();
        agg.feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n");
        agg.feed(b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\n");
        agg.feed(b"data: [DONE]\n\n");
        let usage = agg.finish();
        assert_eq!(
            usage,
            Usage {
                prompt_tokens: 5,
                completion_tokens: 1,
                total_tokens: 6,
                ..Default::default()
            }
        );
    }

    #[test]
    fn stream_aggregator_feed_split_across_chunks() {
        let mut agg = OpenAIAdapter.new_stream_aggregator();
        let full = "data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n";
        let mid = full.len() / 2;
        agg.feed(&full.as_bytes()[..mid]);
        agg.feed(&full.as_bytes()[mid..]);
        let usage = agg.finish();
        assert_eq!(usage.total_tokens, 3);
    }

    #[test]
    fn stream_aggregator_skips_malformed_event() {
        let mut agg = OpenAIAdapter.new_stream_aggregator();
        agg.feed(b"data: not json\n\n");
        agg.feed(b"data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n");
        let usage = agg.finish();
        assert_eq!(usage.total_tokens, 2);
    }
}
