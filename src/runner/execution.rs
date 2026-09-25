//! Cooperative JavaScript execution budget.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Deadline value meaning "no operation is running".
const NO_DEADLINE: u64 = u64::MAX;

/// Deadline read by the QuickJS interrupt handler.
///
/// The deadline is stored as nanoseconds after `origin`, so starting and ending a
/// budget costs one clock read and two stores, with no lock.
#[derive(Debug)]
pub(crate) struct ExecutionControl {
    origin: Instant,
    deadline: AtomicU64,
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            deadline: AtomicU64::new(NO_DEADLINE),
        }
    }
}

impl ExecutionControl {
    pub(crate) fn interrupted(&self) -> bool {
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
}

pub(crate) struct ExecutionGuard<'a>(&'a ExecutionControl);

impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        self.0.deadline.store(NO_DEADLINE, Ordering::Release);
    }
}
