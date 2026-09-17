//! Cooperative JavaScript execution limits, independent of the command queue.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub(crate) struct ExecutionControl {
    stopping: AtomicBool,
    deadline: Mutex<Option<Instant>>,
    #[cfg(feature = "async-promise")]
    wake: Mutex<Option<std::task::Waker>>,
    #[cfg(feature = "async-promise")]
    serial: tokio::sync::Mutex<()>,
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
        self.is_stopping()
            || self.deadline.lock().map_or(true, |deadline| {
                deadline.is_some_and(|deadline| Instant::now() >= deadline)
            })
    }

    pub(crate) fn enter(self: &Arc<Self>, budget: Duration) -> ExecutionGuard {
        *self.deadline.lock().unwrap_or_else(|e| e.into_inner()) =
            Instant::now().checked_add(budget);
        ExecutionGuard(self.clone())
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

pub(crate) struct ExecutionGuard(Arc<ExecutionControl>);

impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        *self.0.deadline.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
