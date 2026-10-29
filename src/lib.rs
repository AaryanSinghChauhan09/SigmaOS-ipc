//! SigmaOS IPC — inter-process communication primitives.
//!
//! Principles: Safe Concurrency, Race-Free Design, Deadlock Avoidance,
//! Separation of Mechanism & Policy, Memory Safety, Process Isolation.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum IpcError {
    ChannelClosed,
    SendFailed(String),
    RecvTimeout,
    Overflow,
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::ChannelClosed => write!(f, "channel closed"),
            IpcError::SendFailed(e) => write!(f, "send failed: {e}"),
            IpcError::RecvTimeout => write!(f, "receive timed out"),
            IpcError::Overflow => write!(f, "channel buffer overflow"),
        }
    }
}

/// Bounded MPSC channel — race-free design using Mutex + Condvar.
struct ChannelInner<T> {
    queue: VecDeque<T>,
    capacity: usize,
    closed: bool,
}

/// Sender side of the IPC channel.
pub struct Sender<T> {
    inner: Arc<(Mutex<ChannelInner<T>>, Condvar)>,
}

/// Receiver side of the IPC channel.
pub struct Receiver<T> {
    inner: Arc<(Mutex<ChannelInner<T>>, Condvar)>,
}

/// Create a bounded IPC channel with given capacity.
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    assert!(capacity > 0, "capacity must be positive");
    let inner = Arc::new((
        Mutex::new(ChannelInner {
            queue: VecDeque::with_capacity(capacity),
            capacity,
            closed: false,
        }),
        Condvar::new(),
    ));
    (Sender { inner: inner.clone() }, Receiver { inner })
}

impl<T: Send> Sender<T> {
    /// Send a message. Returns Err if channel is closed or full.
    pub fn send(&self, value: T) -> Result<(), IpcError> {
        let (lock, cvar) = &*self.inner;
        let mut state = lock.lock().map_err(|e| IpcError::SendFailed(e.to_string()))?;
        if state.closed {
            return Err(IpcError::ChannelClosed);
        }
        if state.queue.len() >= state.capacity {
            return Err(IpcError::Overflow);
        }
        state.queue.push_back(value);
        cvar.notify_one();
        Ok(())
    }

    /// Close the sender side.
    pub fn close(&self) {
        if let Ok(mut state) = self.inner.0.lock() {
            state.closed = true;
            self.inner.1.notify_all();
        }
    }
}

impl<T: Send> Receiver<T> {
    /// Receive a message, blocking until one is available or channel closes.
    pub fn recv(&self) -> Result<T, IpcError> {
        let (lock, cvar) = &*self.inner;
        let mut state = lock.lock().unwrap();
        loop {
            if let Some(value) = state.queue.pop_front() {
                return Ok(value);
            }
            if state.closed {
                return Err(IpcError::ChannelClosed);
            }
            state = cvar.wait(state).unwrap();
        }
    }

    /// Try to receive with a timeout.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, IpcError> {
        let (lock, cvar) = &*self.inner;
        let mut state = lock.lock().unwrap();
        loop {
            if let Some(value) = state.queue.pop_front() {
                return Ok(value);
            }
            if state.closed {
                return Err(IpcError::ChannelClosed);
            }
            let (new_state, timed_out) = cvar.wait_timeout(state, timeout).unwrap();
            state = new_state;
            if timed_out.timed_out() {
                return Err(IpcError::RecvTimeout);
            }
        }
    }

    /// Non-blocking try to receive.
    pub fn try_recv(&self) -> Option<T> {
        let (lock, _) = &*self.inner;
        lock.lock().ok()?.queue.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_send_recv_basic() {
        // Arrange
        let (tx, rx) = channel::<i32>(10);
        // Act
        tx.send(42).unwrap();
        let value = rx.recv().unwrap();
        // Assert
        assert_eq!(value, 42);
    }

    #[test]
    fn test_channel_overflow_returns_error() {
        // Arrange
        let (tx, _rx) = channel::<i32>(2);
        // Act
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        let result = tx.send(3);
        // Assert
        assert_eq!(result, Err(IpcError::Overflow));
    }

    #[test]
    fn test_closed_channel_returns_error() {
        // Arrange
        let (tx, rx) = channel::<i32>(10);
        // Act
        tx.close();
        let result = rx.recv();
        // Assert
        assert_eq!(result, Err(IpcError::ChannelClosed));
    }

    #[test]
    fn test_recv_timeout() {
        // Arrange
        let (_tx, rx) = channel::<i32>(10);
        // Act
        let result = rx.recv_timeout(Duration::from_millis(10));
        // Assert
        assert_eq!(result, Err(IpcError::RecvTimeout));
    }

    #[test]
    fn test_multi_thread_send_recv() {
        // Arrange
        let (tx, rx) = channel::<i32>(100);
        // Act
        let handle = thread::spawn(move || {
            for i in 0..50 {
                tx.send(i).unwrap();
            }
        });
        handle.join().unwrap();
        // Assert — collect all messages
        let mut received = Vec::new();
        while let Some(v) = rx.try_recv() {
            received.push(v);
        }
        assert_eq!(received.len(), 50);
        assert_eq!(received.iter().sum::<i32>(), (0..50).sum());
    }
}
