//! Shared counting semaphore for concurrency control.
//!
//! Used by both [`ToolExecutor`] and [`AgentPool`] to cap the number of
//! concurrent async operations.

use tokio::sync::{Mutex, Notify};
use std::collections::VecDeque;
use std::sync::Arc;

/// Classic counting semaphore for concurrency control.
///
/// `acquire()` resolves immediately if a slot is free, otherwise queues the
/// caller. `release()` unblocks the next waiter in FIFO order.
#[derive(Clone)]
pub struct Semaphore {
    inner: Arc<SemaphoreInner>,
}

struct SemaphoreInner {
    max: usize,
    state: Mutex<SemaphoreState>,
}

struct SemaphoreState {
    current: usize,
    queue: VecDeque<Arc<Notify>>,
}

impl Semaphore {
    /// Create a new semaphore with the given maximum concurrency.
    ///
    /// # Panics
    /// Panics if `max` is 0.
    pub fn new(max: usize) -> Self {
        assert!(max >= 1, "Semaphore max must be at least 1, got {}", max);
        Self {
            inner: Arc::new(SemaphoreInner {
                max,
                state: Mutex::new(SemaphoreState {
                    current: 0,
                    queue: VecDeque::new(),
                }),
            }),
        }
    }

    /// Acquire a slot. Resolves immediately when one is free, or waits until a
    /// holder calls `release()`.
    pub async fn acquire(&self) {
        let notify = {
            let mut state = self.inner.state.lock().await;
            if state.current < self.inner.max {
                state.current += 1;
                return;
            }
            let notify = Arc::new(Notify::new());
            state.queue.push_back(notify.clone());
            notify
        };
        notify.notified().await;
    }

    /// Release a previously acquired slot.
    /// If callers are queued, the next one is unblocked.
    pub async fn release(&self) {
        let mut state = self.inner.state.lock().await;
        if let Some(next) = state.queue.pop_front() {
            // Hand the slot directly to the next waiter.
            next.notify_one();
        } else {
            state.current -= 1;
        }
    }

    /// Run `f` while holding one slot, automatically releasing it afterward
    /// even if `f` returns an error.
    pub async fn run<T, F, Fut>(&self, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        self.acquire().await;
        let result = f().await;
        self.release().await;
        result
    }
}
