//! Accumulates raw SSE bytes and yields complete event payloads
//! (delimited by a blank line) as they become available, buffering any
//! trailing partial event for the next `feed` call.

#[derive(Default)]
pub struct SseBuffer {
    buf: Vec<u8>,
}

impl SseBuffer {
    /// Returns the complete `data:`-line payloads found in newly fed
    /// bytes, in order. Non-data lines (e.g. `event: message_start`) are
    /// dropped here; callers needing the event type parse the JSON
    /// payload's `type` field instead, since both OpenAI and Anthropic
    /// include it there.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        // Drop CR bytes as they arrive so the blank-line delimiter search
        // below only ever needs to look for "\n\n", regardless of
        // whether the upstream uses LF or CRLF line endings (and
        // regardless of where a chunk boundary falls relative to a
        // "\r\n"). Safe because a raw CR byte should never occur inside a
        // valid `data:` JSON payload — only ever as a line-ending
        // artifact.
        if chunk.contains(&b'\r') {
            self.buf.extend(chunk.iter().copied().filter(|&b| b != b'\r'));
        } else {
            self.buf.extend_from_slice(chunk);
        }

        let mut payloads = Vec::new();
        while let Some(idx) = find_blank_line(&self.buf) {
            let raw: Vec<u8> = self.buf.drain(..idx + 2).collect();
            let event = &raw[..idx];
            for line in event.split(|&b| b == b'\n') {
                if let Some(payload) = line.strip_prefix(b"data:") {
                    payloads.push(trim_ascii(payload));
                }
            }
        }
        payloads
    }
}

fn find_blank_line(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

fn trim_ascii(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_event_in_one_chunk() {
        let mut sse = SseBuffer::default();
        let payloads = sse.feed(b"data: hello\n\n");
        assert_eq!(payloads, vec!["hello".to_string()]);
    }

    #[test]
    fn event_split_across_chunks() {
        let mut sse = SseBuffer::default();
        let full = b"data: hello\n\n";
        let mid = full.len() / 2;
        assert!(sse.feed(&full[..mid]).is_empty());
        let payloads = sse.feed(&full[mid..]);
        assert_eq!(payloads, vec!["hello".to_string()]);
    }

    #[test]
    fn multiple_events_in_one_chunk() {
        let mut sse = SseBuffer::default();
        let payloads = sse.feed(b"data: one\n\ndata: two\n\n");
        assert_eq!(payloads, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn ignores_non_data_lines() {
        let mut sse = SseBuffer::default();
        let payloads = sse.feed(b"event: message_start\ndata: hello\nid: 1\n\n");
        assert_eq!(payloads, vec!["hello".to_string()]);
    }

    #[test]
    fn handles_crlf_line_endings() {
        let mut sse = SseBuffer::default();
        let payloads = sse.feed(b"data: hello\r\n\r\n");
        assert_eq!(payloads, vec!["hello".to_string()]);
    }

    #[test]
    fn crlf_split_across_chunk_boundary() {
        let mut sse = SseBuffer::default();
        let full = b"data: hello\r\n\r\n";
        let mid = full.len() / 2;
        sse.feed(&full[..mid]);
        let payloads = sse.feed(&full[mid..]);
        assert_eq!(payloads, vec!["hello".to_string()]);
    }
}
