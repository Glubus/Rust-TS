//! Cooperative JavaScript execution limits, independent of the command queue.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "async-promise")]
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Deadline value meaning "no operation is running".
const NO_DEADLINE: u64 = u64::MAX;

/// Shared stop flag and deadline read by the QuickJS interrupt handler.
///
/// The deadline is stored as nanoseconds after `origin` in an atomic, so starting
/// and ending a budget costs one clock read and two atomic stores, with no lock.
#[derive(Debug)]
pub(crate) struct ExecutionControl {
    stopping: AtomicBool,
    origin: Instant,
    deadline: AtomicU64,
    #[cfg(feature = "async-promise")]
    wake: Mutex<Option<std::task::Waker>>,
    #[cfg(feature = "async-promise")]
    serial: tokio::sync::Mutex<()>,
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self {
            stopping: AtomicBool::new(false),
            origin: Instant::now(),
            deadline: AtomicU64::new(NO_DEADLINE),
            #[cfg(feature = "async-promise")]
            wake: Mutex::default(),
            #[cfg(feature = "async-promise")]
            serial: tokio::sync::Mutex::default(),
        }
    }
}

impl ExecutionControl {
    pub(crate) fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        #[cfg(feature = "async-promise")]
        if let Some(waker) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).take() {
            waker.wake();
        }
    }

    pub(crate) fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    pub(crate) fn interrupted(&self) -> bool {
        self.is_stopping() || self.deadline_passed()
    }

    fn deadline_passed(&self) -> bool {
        let deadline = self.deadline.load(Ordering::Acquire);
        deadline != NO_DEADLINE && self.elapsed_nanos() >= deadline
    }

    /// Starts a budget of `budget` from now; it ends when the returned guard drops.
    pub(crate) fn enter(&self, budget: Duration) -> ExecutionGuard<'_> {
        let deadline = u64::try_from(budget.as_nanos())
            .ok()
            .and_then(|budget| self.elapsed_nanos().checked_add(budget))
            .unwrap_or(NO_DEADLINE);
        self.deadline.store(deadline, Ordering::Release);
        ExecutionGuard(self)
    }

    fn elapsed_nanos(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(NO_DEADLINE - 1)
    }

    #[cfg(feature = "async-promise")]
    pub(crate) async fn run_async<T>(
        self: &Arc<Self>,
        budget: Duration,
        future: impl std::future::Future<Output = Result<T, crate::error::VmError>>,
    ) -> Result<T, crate::error::VmError> {
        use std::future::{Future, poll_fn};
        use std::task::Poll;
        let _serial = self.serial.lock().await;
        let _guard = self.enter(budget);
        let mut future = std::pin::pin!(future);
        let mut timer = std::pin::pin!(futures_timer::Delay::new(budget));
        poll_fn(|cx| {
            *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
            if self.is_stopping() {
                return Poll::Ready(Err(crate::error::VmError::WorkerOffline));
            }
            if timer.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Err(crate::error::VmError::ExecutionTimeout));
            }
            future.as_mut().poll(cx)
        })
        .await
    }
}

pub(crate) struct ExecutionGuard<'a>(&'a ExecutionControl);

impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        self.0.deadline.store(NO_DEADLINE, Ordering::Release);
    }
}
