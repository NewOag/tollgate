//! Captures up to `limit` bytes and silently drops anything past that,
//! rather than growing without bound. Meant to sit on the side of a
//! response relay: the relay itself is never truncated, only what gets
//! captured for logging/parsing.
pub struct BoundedBuffer {
    buf: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl BoundedBuffer {
    pub fn new(limit: usize) -> Self {
        BoundedBuffer {
            buf: Vec::new(),
            limit,
            truncated: false,
        }
    }

    pub fn write(&mut self, chunk: &[u8]) {
        let room = self.limit.saturating_sub(self.buf.len());
        if room > 0 {
            let take = room.min(chunk.len());
            self.buf.extend_from_slice(&chunk[..take]);
            if take < chunk.len() {
                self.truncated = true;
            }
        } else if !chunk.is_empty() {
            self.truncated = true;
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_everything_within_limit() {
        let mut b = BoundedBuffer::new(10);
        b.write(b"hello");
        assert_eq!(b.bytes(), b"hello");
        assert!(!b.truncated());
    }

    #[test]
    fn truncates_past_limit() {
        let mut b = BoundedBuffer::new(3);
        b.write(b"hello");
        assert_eq!(b.bytes(), b"hel");
        assert!(b.truncated());
    }

    #[test]
    fn truncates_across_multiple_writes() {
        let mut b = BoundedBuffer::new(5);
        b.write(b"he");
        b.write(b"llo");
        b.write(b"world");
        assert_eq!(b.bytes(), b"hello");
        assert!(b.truncated());
    }

    #[test]
    fn zero_limit_captures_nothing() {
        let mut b = BoundedBuffer::new(0);
        b.write(b"hello");
        assert_eq!(b.bytes(), b"");
        assert!(b.truncated());
    }

    #[test]
    fn empty_write_does_not_mark_truncated() {
        let mut b = BoundedBuffer::new(0);
        b.write(b"");
        assert!(!b.truncated());
    }
}
