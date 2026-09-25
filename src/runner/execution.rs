//! Cooperative JavaScript execution budget.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Deadline value meaning "no operation is running".
const NO_DEADLINE: u64 = u64::MAX;

/// Deadline and interrupt request read by the QuickJS interrupt handler.
///
/// The deadline is stored as nanoseconds after `origin`, so starting and ending a
/// budget costs one clock read and a few stores, with no lock. Only atomics are shared,
/// so an interrupt can be requested from any thread.
#[derive(Debug)]
pub(crate) struct ExecutionControl {
    origin: Instant,
    deadline: AtomicU64,
    interrupt_requested: AtomicBool,
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            deadline: AtomicU64::new(NO_DEADLINE),
            interrupt_requested: AtomicBool::new(false),
        }
    }
}

impl ExecutionControl {
    pub(crate) fn interrupted(&self) -> bool {
        self.interrupt_requested.load(Ordering::Acquire) || self.deadline_passed()
    }

    /// Asks the running operation to stop at its next interrupt check. Starting an
    /// operation clears the request, so a request made while none runs has no effect.
    pub(crate) fn request_interrupt(&self) {
        self.interrupt_requested.store(true, Ordering::Release);
    }

    /// Whether the current or last operation was asked to stop.
    pub(crate) fn interrupt_requested(&self) -> bool {
        self.interrupt_requested.load(Ordering::Acquire)
    }

    /// Starts a budget of `budget` from now; it ends when the returned guard drops.
    pub(crate) fn enter(&self, budget: Duration) -> ExecutionGuard<'_> {
        let deadline = u64::try_from(budget.as_nanos())
            .ok()
            .and_then(|budget| self.elapsed_nanos().checked_add(budget))
            .unwrap_or(NO_DEADLINE);
        self.interrupt_requested.store(false, Ordering::Release);
        self.deadline.store(deadline, Ordering::Release);
        ExecutionGuard(self)
    }

    fn deadline_passed(&self) -> bool {
        let deadline = self.deadline.load(Ordering::Acquire);
        deadline != NO_DEADLINE && self.elapsed_nanos() >= deadline
    }

    fn elapsed_nanos(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(NO_DEADLINE - 1)
    }
}

pub(crate) struct ExecutionGuard<'a>(&'a ExecutionControl);

impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        self.0.deadline.store(NO_DEADLINE, Ordering::Release);
    }
}
