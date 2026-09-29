//! OpenAI and Anthropic wire-format parsing: request model/stream
//! extraction, non-streaming usage extraction, and SSE stream
//! reconstruction — just enough of each API's shape to route and log a
//! call without needing to understand its whole schema.

mod anthropic;
pub mod convert;
mod openai;
mod openai_responses;
mod responses;
mod sse;

pub use anthropic::AnthropicAdapter;
pub use openai::OpenAIAdapter;
pub use openai_responses::OpenAIResponsesAdapter;
pub use responses::ResponsesAdapter;

use serde::Deserialize;

/// Provider-agnostic token count. Zero values mean "unknown" (e.g. the
/// client didn't request usage in a streamed OpenAI response).
///
/// `prompt_tokens` always represents the total input token count
/// including any cache-related tokens (OpenAI's `prompt_tokens` is
/// already inclusive of `cached_tokens`; Anthropic's `input_tokens` is
/// normalized here by adding cache_creation/cache_read on top), so both
/// providers share the same semantics. `cache_creation_tokens` and
/// `cache_read_tokens` are informational subsets of `prompt_tokens`, kept
/// for cache-hit-rate reporting and cost calculation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cache_creation_tokens: i64, // Anthropic only; always 0 for OpenAI
    pub cache_read_tokens: i64,     // subset of prompt_tokens, both providers
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedRequest {
    pub model: String,
    pub stream: bool,
}

/// Parses one provider's request/response JSON shape.
pub trait Adapter: Send + Sync {
    /// Extracts model + stream flag from the raw request body.
    fn parse_request(&self, body: &[u8]) -> ParsedRequest;
    /// Extracts usage from a complete (non-streaming) response body.
    fn parse_response(&self, body: &[u8]) -> Usage;
    /// Returns a fresh aggregator for extracting usage from a streamed
    /// (SSE) response as bytes flow through.
    fn new_stream_aggregator(&self) -> Box<dyn StreamAggregator>;
}

/// Consumes raw SSE bytes as they're relayed to the client and
/// reconstructs usage once the stream ends. The raw SSE bytes themselves
/// are captured separately by the caller (for `response_body`); this
/// only extracts the token counts embedded in the event stream. `feed`
/// may be called with arbitrarily-sized chunks that split SSE events
/// mid-line; implementations buffer internally via `sse::SseBuffer`.
pub trait StreamAggregator: Send {
    fn feed(&mut self, chunk: &[u8]);
    fn finish(self: Box<Self>) -> Usage;
}

pub fn adapter_for(format: &str) -> Option<Box<dyn Adapter>> {
    match format {
        "openai" => Some(Box::new(OpenAIAdapter)),
        "anthropic" => Some(Box::new(AnthropicAdapter)),
        "openai_responses" => Some(Box::new(OpenAIResponsesAdapter)),
        "responses" => Some(Box::new(ResponsesAdapter)),
        _ => None,
    }
}

/// OpenAI and Anthropic requests share the same `model`/`stream` shape,
/// so both adapters' `parse_request` delegate to this one parser.
#[derive(Deserialize, Default)]
struct RawRequest {
    #[serde(default)]
    model: String,
    #[serde(default)]
    stream: bool,
}

fn parse_request_common(body: &[u8]) -> ParsedRequest {
    let req: RawRequest = serde_json::from_slice(body).unwrap_or_default();
    ParsedRequest {
        model: req.model,
        stream: req.stream,
    }
}
