//! Binary PTY output transport: frames sent to the frontend over a Tauri
//! IPC channel. Frame = 4-byte little-endian session id + raw output bytes.

use parking_lot::Mutex;
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeResponseBody};

/// `[id as u32 LE][data...]`
pub fn encode_frame(id: u32, data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&id.to_le_bytes());
    frame.extend_from_slice(data);
    frame
}

/// Destination for encoded frames. Implemented by the Tauri channel; tests
/// substitute a recorder.
pub trait FrameSink: Send + Sync {
    fn send(&self, frame: Vec<u8>);
}

impl FrameSink for Channel<InvokeResponseBody> {
    fn send(&self, frame: Vec<u8>) {
        // A closed webview just means nobody is listening; drop the frame.
        let _ = Channel::send(self, InvokeResponseBody::Raw(frame));
    }
}

/// App-wide output destination. The frontend subscribes once on startup;
/// a reload re-subscribes and replaces the previous channel.
#[derive(Default)]
pub struct OutputHub {
    sink: Mutex<Option<Arc<dyn FrameSink>>>,
}

impl OutputHub {
    pub fn subscribe(&self, sink: Arc<dyn FrameSink>) {
        *self.sink.lock() = Some(sink);
    }

    /// No-op until a sink is subscribed.
    pub fn send(&self, frame: Vec<u8>) {
        let sink = self.sink.lock().clone();
        if let Some(sink) = sink {
            sink.send(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<Vec<u8>>>);

    impl FrameSink for RecordingSink {
        fn send(&self, frame: Vec<u8>) {
            self.0.lock().push(frame);
        }
    }

    #[test]
    fn encode_frame_layout() {
        assert_eq!(encode_frame(0x01020304, b"hi"), vec![4, 3, 2, 1, b'h', b'i']);
    }

    #[test]
    fn send_without_sink_is_noop() {
        OutputHub::default().send(vec![1, 2, 3]);
    }

    #[test]
    fn subscribe_replaces_previous_sink() {
        let hub = OutputHub::default();
        let a = Arc::new(RecordingSink::default());
        let b = Arc::new(RecordingSink::default());
        hub.subscribe(a.clone());
        hub.subscribe(b.clone());
        hub.send(vec![9]);
        assert!(a.0.lock().is_empty());
        assert_eq!(*b.0.lock(), vec![vec![9]]);
    }
}
