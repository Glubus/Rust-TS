//! Cooperative JavaScript execution budget.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Deadline value meaning "no operation is running, or it has no time limit".
const NO_DEADLINE: u64 = u64::MAX;

/// Deadline and interrupt request read by the QuickJS interrupt handler.
///
/// Starting a budget sets the deadline, in nanoseconds after `origin`, to the budget
/// from that moment: everything the operation does counts against it, host
/// functions included. QuickJS checks it about every ten thousand interpreter
/// steps, so JavaScript is stopped at the first check after it passes. Only atomics
/// are shared, so an interrupt can be requested from any thread.
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
        self.interrupt_requested() || self.budget_expired()
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

    /// Whether the running operation has used up its budget. False when no
    /// operation runs or it has no time limit.
    pub(crate) fn budget_expired(&self) -> bool {
        match self.deadline.load(Ordering::Acquire) {
            NO_DEADLINE => false,
            deadline => self.elapsed_nanos() >= deadline,
        }
    }

    /// Starts a budget of `budget` from now; it ends when the returned guard drops.
    pub(crate) fn enter(&self, budget: Duration) -> ExecutionGuard<'_> {
        let deadline = u64::try_from(budget.as_nanos())
            .ok()
            .and_then(|budget| self.elapsed_nanos().checked_add(budget))
            .filter(|&deadline| deadline != NO_DEADLINE)
            .unwrap_or(NO_DEADLINE);
        self.interrupt_requested.store(false, Ordering::Release);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_budget_is_expired_at_the_first_check() {
        let control = ExecutionControl::default();
        let _guard = control.enter(Duration::ZERO);

        assert!(control.budget_expired());
        assert!(control.interrupted());
        assert!(!control.interrupt_requested());
    }

    #[test]
    fn an_unlimited_budget_never_expires() {
        let control = ExecutionControl::default();
        let _guard = control.enter(Duration::MAX);

        assert!(!control.interrupted());
    }

    #[test]
    fn a_budget_ends_with_its_guard() {
        let control = ExecutionControl::default();
        drop(control.enter(Duration::ZERO));

        assert!(!control.budget_expired());
        assert!(!control.interrupted());
    }

    #[test]
    fn entering_clears_an_interrupt_request_but_not_a_later_one() {
        let control = ExecutionControl::default();
        control.request_interrupt();
        let _guard = control.enter(Duration::MAX);
        assert!(!control.interrupted());

        control.request_interrupt();
        assert!(control.interrupted());
        assert!(!control.budget_expired());
    }
}
