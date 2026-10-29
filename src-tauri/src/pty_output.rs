//! Binary PTY output transport: frames sent to the frontend over a Tauri
//! IPC channel. Frame = 4-byte little-endian session id + raw output bytes.

use crate::ring_buffer::RingBuffer;
use parking_lot::{Condvar, Mutex, MutexGuard};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};

/// A batch is sent once it reaches this size...
pub const MAX_BATCH_BYTES: usize = 64 * 1024;
/// ...or this long after its first byte arrived, whichever comes first.
pub const MAX_BATCH_DELAY: Duration = Duration::from_millis(4);

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

struct BatchState {
    attached: bool,
    ring: RingBuffer,
    pending: Vec<u8>,
    /// When the first byte of the current (non-empty) batch arrived.
    first_at: Option<Instant>,
    closed: bool,
}

/// Per-session output coalescing. Owns the session's attach flag and ring
/// buffer so that record / detach / attach are atomic under one lock: a byte
/// is either sent to the frontend or left in the ring buffer for replay,
/// never discarded by a concurrent detach.
pub struct OutputBatcher {
    id: u32,
    hub: Arc<OutputHub>,
    state: Mutex<BatchState>,
    wake: Condvar,
}

impl OutputBatcher {
    /// Starts attached and spawns the flusher thread, which runs until
    /// [`close`](Self::close).
    pub fn new(id: u32, ring_capacity: usize, hub: Arc<OutputHub>) -> Arc<Self> {
        let batcher = Arc::new(Self {
            id,
            hub,
            state: Mutex::new(BatchState {
                attached: true,
                ring: RingBuffer::new(ring_capacity),
                pending: Vec::new(),
                first_at: None,
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let flusher = batcher.clone();
        thread::spawn(move || flusher.run_flusher());
        batcher
    }

    /// Record PTY output: always into the ring buffer, and (while attached)
    /// into the pending batch for live delivery. Ignored after `close`.
    pub fn record(&self, data: &[u8]) {
        let mut st = self.state.lock();
        if st.closed {
            return;
        }
        st.ring.write(data);
        // Detached output lives only in the ring buffer; attach replays it.
        // Queuing it too would send it a second time after attach.
        if !st.attached {
            return;
        }
        st.pending.extend_from_slice(data);
        if st.first_at.is_none() {
            st.first_at = Some(Instant::now());
            self.wake.notify_one();
        }
        if st.pending.len() >= MAX_BATCH_BYTES {
            self.flush_locked(&mut st);
        }
    }

    /// Send what is pending, then stop live delivery and discard the ring
    /// buffer (its contents have all been sent). No-op when already detached:
    /// the ring then holds unsent output that the next attach must replay.
    pub fn detach(&self) {
        let mut st = self.state.lock();
        if !st.attached {
            return;
        }
        self.flush_locked(&mut st);
        st.attached = false;
        st.ring.drain();
    }

    /// Resume live delivery. If the session was detached, the bytes recorded
    /// since then go out as one frame first — under the lock, so no output
    /// recorded after this call can overtake the replay.
    pub fn attach(&self) {
        let mut st = self.state.lock();
        if st.attached {
            return;
        }
        st.attached = true;
        let replay = st.ring.drain();
        if !replay.is_empty() {
            self.hub.send(encode_frame(self.id, &replay));
        }
    }

    /// Stop the flusher. With `flush`, pending output is sent first (PTY
    /// EOF, so the last output precedes `pty-exit`); without it, dropped (kill).
    pub fn close(&self, flush: bool) {
        let mut st = self.state.lock();
        if flush {
            self.flush_locked(&mut st);
        } else {
            st.pending.clear();
            st.first_at = None;
        }
        st.closed = true;
        self.wake.notify_one();
    }

    /// Sends under the lock so frames keep byte order and a concurrent
    /// detach cannot slip between "taken" and "sent".
    fn flush_locked(&self, st: &mut MutexGuard<'_, BatchState>) {
        st.first_at = None;
        if st.pending.is_empty() {
            return;
        }
        let data = std::mem::take(&mut st.pending);
        if st.attached {
            self.hub.send(encode_frame(self.id, &data));
        }
    }

    // Sole waiter on `wake`, which is why every notifier uses `notify_one`.
    fn run_flusher(&self) {
        let mut st = self.state.lock();
        loop {
            if st.closed {
                return;
            }
            match st.first_at {
                None => self.wake.wait(&mut st),
                Some(first) => {
                    let deadline = first + MAX_BATCH_DELAY;
                    if Instant::now() >= deadline {
                        self.flush_locked(&mut st);
                    } else {
                        self.wake.wait_until(&mut st, deadline);
                    }
                }
            }
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

    fn batcher_with_sink() -> (Arc<OutputBatcher>, Arc<RecordingSink>) {
        let hub = Arc::new(OutputHub::default());
        let sink = Arc::new(RecordingSink::default());
        hub.subscribe(sink.clone());
        (OutputBatcher::new(7, 1024 * 1024, hub), sink)
    }

    fn payloads(sink: &RecordingSink) -> Vec<Vec<u8>> {
        sink.0
            .lock()
            .iter()
            .map(|f| {
                assert_eq!(&f[..4], &7u32.to_le_bytes());
                f[4..].to_vec()
            })
            .collect()
    }

    fn settle() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    #[test]
    fn small_writes_coalesce() {
        let (b, sink) = batcher_with_sink();
        b.record(b"a");
        b.record(b"b");
        settle();
        assert_eq!(payloads(&sink), vec![b"ab".to_vec()]);
    }

    #[test]
    fn large_burst_splits_in_order() {
        let (b, sink) = batcher_with_sink();
        let mut input = Vec::new();
        for i in 0..3u8 {
            let chunk = vec![i; 40 * 1024];
            b.record(&chunk);
            input.extend_from_slice(&chunk);
        }
        settle();
        let frames = payloads(&sink);
        assert!(frames.len() >= 2);
        assert!(frames.iter().all(|f| f.len() <= 128 * 1024));
        assert_eq!(frames.concat(), input);
    }

    #[test]
    fn detached_does_not_send() {
        let (b, sink) = batcher_with_sink();
        b.detach();
        b.record(b"x");
        settle();
        assert!(sink.0.lock().is_empty());
    }

    #[test]
    fn attach_replays_detached_output_as_frame() {
        let (b, sink) = batcher_with_sink();
        b.detach();
        b.record(b"x");
        b.attach();
        assert_eq!(payloads(&sink), vec![b"x".to_vec()]);
    }

    #[test]
    fn attach_replay_precedes_later_output() {
        let (b, sink) = batcher_with_sink();
        b.detach();
        b.record(b"old");
        b.attach();
        b.record(b"new");
        settle();
        assert_eq!(payloads(&sink), vec![b"old".to_vec(), b"new".to_vec()]);
    }

    #[test]
    fn attach_when_attached_sends_nothing() {
        let (b, sink) = batcher_with_sink();
        b.attach();
        settle();
        assert!(sink.0.lock().is_empty());
    }

    #[test]
    fn second_detach_keeps_detached_output() {
        let (b, sink) = batcher_with_sink();
        b.detach();
        b.record(b"x");
        b.detach();
        b.attach();
        assert_eq!(payloads(&sink), vec![b"x".to_vec()]);
    }

    #[test]
    fn attach_right_after_detached_output_does_not_resend() {
        let (b, sink) = batcher_with_sink();
        b.detach();
        b.record(b"xyz");
        b.attach();
        settle();
        assert_eq!(payloads(&sink), vec![b"xyz".to_vec()]);
    }

    #[test]
    fn detach_flushes_pending() {
        let (b, sink) = batcher_with_sink();
        b.record(b"x");
        b.detach();
        assert_eq!(payloads(&sink), vec![b"x".to_vec()]);
    }

    #[test]
    fn concurrent_record_during_detach_loses_nothing() {
        let (b, sink) = batcher_with_sink();
        let writer = {
            let b = b.clone();
            std::thread::spawn(move || {
                for i in 0..1000u32 {
                    b.record(&[(i % 251) as u8]);
                    if i % 100 == 0 {
                        std::thread::yield_now();
                    }
                }
            })
        };
        std::thread::sleep(std::time::Duration::from_micros(200));
        b.detach();
        writer.join().unwrap();
        settle();
        b.attach();
        let got = payloads(&sink).concat();
        let expected: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn close_with_flush_sends_pending() {
        let (b, sink) = batcher_with_sink();
        b.record(b"z");
        b.close(true);
        assert_eq!(payloads(&sink), vec![b"z".to_vec()]);
    }

    #[test]
    fn close_without_flush_drops_pending() {
        let (b, sink) = batcher_with_sink();
        b.record(b"z");
        b.close(false);
        b.record(b"after");
        settle();
        assert!(sink.0.lock().is_empty());
    }
}
