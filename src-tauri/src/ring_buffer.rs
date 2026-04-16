/// Fixed-capacity ring buffer for PTY output buffering.
/// Used to retain data while a session is detached (not actively viewed).
/// On reattach, the buffer is drained and sent to the frontend.
pub struct RingBuffer {
    buf: Vec<u8>,
    capacity: usize,
    write_pos: usize,
    len: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            capacity,
            write_pos: 0,
            len: 0,
        }
    }

    pub fn write(&mut self, data: &[u8]) {
        if data.len() >= self.capacity {
            // Data larger than buffer: keep only the last `capacity` bytes
            let start = data.len() - self.capacity;
            self.buf.copy_from_slice(&data[start..]);
            self.write_pos = 0;
            self.len = self.capacity;
            return;
        }

        let n = data.len();
        let space_to_end = self.capacity - self.write_pos;

        if n <= space_to_end {
            // Fits without wrapping
            self.buf[self.write_pos..self.write_pos + n].copy_from_slice(data);
        } else {
            // Split into two chunks at the wrap point
            self.buf[self.write_pos..self.write_pos + space_to_end]
                .copy_from_slice(&data[..space_to_end]);
            self.buf[..n - space_to_end].copy_from_slice(&data[space_to_end..]);
        }

        self.write_pos = (self.write_pos + n) % self.capacity;
        self.len = (self.len + n).min(self.capacity);
    }

    /// Drain all buffered data in order. Returns the data and resets the buffer.
    pub fn drain(&mut self) -> Vec<u8> {
        if self.len == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(self.len);
        let start = if self.len < self.capacity {
            0
        } else {
            self.write_pos
        };

        for i in 0..self.len {
            let idx = (start + i) % self.capacity;
            result.push(self.buf[idx]);
        }

        self.write_pos = 0;
        self.len = 0;
        result
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buffer() {
        let mut rb = RingBuffer::new(16);
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.drain(), Vec::<u8>::new());
    }

    #[test]
    fn test_simple_write_and_drain() {
        let mut rb = RingBuffer::new(16);
        rb.write(b"hello");
        assert_eq!(rb.len(), 5);
        assert_eq!(rb.drain(), b"hello");
        assert!(rb.is_empty());
    }

    #[test]
    fn test_multiple_writes() {
        let mut rb = RingBuffer::new(16);
        rb.write(b"hello ");
        rb.write(b"world");
        assert_eq!(rb.len(), 11);
        assert_eq!(rb.drain(), b"hello world");
    }

    #[test]
    fn test_overflow_wraps_around() {
        let mut rb = RingBuffer::new(8);
        rb.write(b"abcdefgh"); // fills exactly
        rb.write(b"ij"); // overwrites first 2
        assert_eq!(rb.len(), 8);
        assert_eq!(rb.drain(), b"cdefghij");
    }

    #[test]
    fn test_data_larger_than_capacity() {
        let mut rb = RingBuffer::new(4);
        rb.write(b"abcdefghij");
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.drain(), b"ghij"); // last 4 bytes
    }

    #[test]
    fn test_drain_resets_buffer() {
        let mut rb = RingBuffer::new(16);
        rb.write(b"first");
        rb.drain();
        rb.write(b"second");
        assert_eq!(rb.drain(), b"second");
    }

    #[test]
    fn test_1mb_buffer() {
        let mut rb = RingBuffer::new(1024 * 1024); // 1MB
        let data = vec![0x42u8; 512 * 1024]; // 512KB
        rb.write(&data);
        assert_eq!(rb.len(), 512 * 1024);
        rb.write(&data); // another 512KB = 1MB total
        assert_eq!(rb.len(), 1024 * 1024);
        rb.write(b"overflow");
        assert_eq!(rb.len(), 1024 * 1024);
        let drained = rb.drain();
        assert_eq!(drained.len(), 1024 * 1024);
        // Last bytes should be "overflow"
        assert_eq!(&drained[drained.len() - 8..], b"overflow");
    }
}
