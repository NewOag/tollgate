use serde::Deserialize;

use super::{parse_request_common, sse::SseBuffer, Adapter, ParsedRequest, StreamAggregator, Usage};

#[derive(Deserialize, Default, Clone, Copy)]
struct RawUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
}

/// `input_tokens` excludes cache_creation/cache_read tokens (Anthropic
/// reports them as sibling fields, unlike OpenAI where `cached_tokens`
/// is already a subset of `prompt_tokens`) — normalize `prompt_tokens`
/// to the total input actually sent, so both providers share one
/// semantics.
impl From<RawUsage> for Usage {
    fn from(u: RawUsage) -> Self {
        let total_input = u.input_tokens + u.cache_creation_input_tokens + u.cache_read_input_tokens;
        Usage {
            prompt_tokens: total_input,
            completion_tokens: u.output_tokens,
            total_tokens: total_input + u.output_tokens,
            cache_creation_tokens: u.cache_creation_input_tokens,
            cache_read_tokens: u.cache_read_input_tokens,
        }
    }
}

#[derive(Deserialize, Default)]
struct RawResponse {
    #[serde(default)]
    usage: RawUsage,
}

pub struct AnthropicAdapter;

impl Adapter for AnthropicAdapter {
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
struct MessageStart {
    #[serde(default)]
    usage: RawUsage,
}

#[derive(Deserialize, Default)]
struct MessageDeltaUsage {
    #[serde(default)]
    output_tokens: i64,
}

#[derive(Deserialize, Default)]
struct StreamEvent {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    message: MessageStart,
    #[serde(default)]
    usage: MessageDeltaUsage,
}

/// Extracts usage from the event stream without reconstructing the
/// assistant text — the raw SSE bytes are captured separately for
/// `response_body`. Input tokens (including cache_creation/cache_read,
/// which Anthropic only reports on `message_start`) come from
/// `message_start`'s embedded usage; output tokens come from
/// `message_delta`'s usage, which Anthropic sends as a running total
/// (last value wins).
#[derive(Default)]
struct StreamState {
    sse: SseBuffer,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
}

impl StreamAggregator for StreamState {
    fn feed(&mut self, chunk: &[u8]) {
        for payload in self.sse.feed(chunk) {
            let Ok(event) = serde_json::from_str::<StreamEvent>(&payload) else {
                continue;
            };
            match event.kind.as_str() {
                "message_start" => {
                    self.input_tokens = event.message.usage.input_tokens;
                    self.cache_creation_tokens = event.message.usage.cache_creation_input_tokens;
                    self.cache_read_tokens = event.message.usage.cache_read_input_tokens;
                    if event.message.usage.output_tokens > 0 {
                        self.output_tokens = event.message.usage.output_tokens;
                    }
                }
                "message_delta" if event.usage.output_tokens > 0 => {
                    self.output_tokens = event.usage.output_tokens;
                }
                _ => {}
            }
        }
    }

    fn finish(self: Box<Self>) -> Usage {
        let total_input = self.input_tokens + self.cache_creation_tokens + self.cache_read_tokens;
        Usage {
            prompt_tokens: total_input,
            completion_tokens: self.output_tokens,
            total_tokens: total_input + self.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            cache_read_tokens: self.cache_read_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request() {
        let got = AnthropicAdapter.parse_request(br#"{"model":"claude-3-5-sonnet","stream":true}"#);
        assert_eq!(
            got,
            ParsedRequest {
                model: "claude-3-5-sonnet".to_string(),
                stream: true
            }
        );
    }

    #[test]
    fn parse_response() {
        let got = AnthropicAdapter.parse_response(br#"{"usage":{"input_tokens":10,"output_tokens":20}}"#);
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
    fn parse_response_normalizes_cache_tokens_into_prompt_tokens() {
        let body = br#"{"usage":{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":5,"cache_read_input_tokens":85}}"#;
        let got = AnthropicAdapter.parse_response(body);
        assert_eq!(
            got,
            Usage {
                prompt_tokens: 100,
                completion_tokens: 20,
                total_tokens: 120,
                cache_creation_tokens: 5,
                cache_read_tokens: 85,
            }
        );
    }

    #[test]
    fn stream_aggregator_normalizes_cache_tokens_from_message_start() {
        let mut agg = AnthropicAdapter.new_stream_aggregator();
        for e in [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":0,"cache_creation_input_tokens":3,"cache_read_input_tokens":50}}}"#,
            r#"{"type":"message_delta","usage":{"output_tokens":7}}"#,
        ] {
            agg.feed(format!("data: {e}\n\n").as_bytes());
        }
        let usage = agg.finish();
        assert_eq!(
            usage,
            Usage {
                prompt_tokens: 63,
                completion_tokens: 7,
                total_tokens: 70,
                cache_creation_tokens: 3,
                cache_read_tokens: 50,
            }
        );
    }

    /// Replays a realistic message_start -> content_block_delta* ->
    /// message_delta -> message_stop sequence and checks that usage
    /// combines message_start's input_tokens with message_delta's
    /// (cumulative) output_tokens; content_block_delta events are
    /// ignored by the aggregator (raw SSE is captured separately).
    #[test]
    fn stream_aggregator_full_sequence_extracts_usage() {
        let mut agg = AnthropicAdapter.new_stream_aggregator();
        for e in [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":42,"output_tokens":0}}}"#,
            r#"{"type":"content_block_start","index":0}"#,
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hel"}}"#,
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"lo"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}"#,
            r#"{"type":"message_stop"}"#,
        ] {
            agg.feed(format!("event: x\ndata: {e}\n\n").as_bytes());
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

    /// When Anthropic sends multiple message_delta events with a running
    /// output_tokens total, the aggregator keeps the last one rather
    /// than summing them.
    #[test]
    fn stream_aggregator_message_delta_last_value_wins() {
        let mut agg = AnthropicAdapter.new_stream_aggregator();
        for e in [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":1,"output_tokens":0}}}"#,
            r#"{"type":"message_delta","usage":{"output_tokens":3}}"#,
            r#"{"type":"message_delta","usage":{"output_tokens":9}}"#,
        ] {
            agg.feed(format!("data: {e}\n\n").as_bytes());
        }
        let usage = agg.finish();
        assert_eq!(usage.completion_tokens, 9);
    }

    #[test]
    fn stream_aggregator_feed_split_across_chunks() {
        let mut agg = AnthropicAdapter.new_stream_aggregator();
        let full = r#"data: {"type":"message_start","message":{"usage":{"input_tokens":5,"output_tokens":0}}}"#.to_string() + "\n\n";
        let mid = full.len() / 2;
        agg.feed(&full.as_bytes()[..mid]);
        agg.feed(&full.as_bytes()[mid..]);
        let usage = agg.finish();
        assert_eq!(usage.prompt_tokens, 5);
    }
}
