//! Cooperative JavaScript execution budget.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Deadline value meaning "no operation is running, or it has no time limit".
const NO_DEADLINE: u64 = u64::MAX;
/// Deadline value meaning "an operation is running; its deadline is set at the first
/// interrupt check".
const DEADLINE_PENDING: u64 = u64::MAX - 1;
/// Budget value meaning "no time limit".
const UNLIMITED: u64 = u64::MAX;

/// Deadline and interrupt request read by the QuickJS interrupt handler.
///
/// Starting a budget reads no clock: the deadline is set, in nanoseconds after
/// `origin`, by the first interrupt check, which QuickJS makes after about ten
/// thousand interpreter steps. An operation that ends sooner, like most event
/// deliveries and calls, never reads the clock; a long one gets its full budget from
/// that first check, a fraction of a millisecond of JavaScript after it started, so
/// time spent in host functions before it is not counted. Only atomics are shared,
/// so an interrupt can be requested from any thread.
#[derive(Debug)]
pub(crate) struct ExecutionControl {
    origin: Instant,
    deadline: AtomicU64,
    budget: AtomicU64,
    interrupt_requested: AtomicBool,
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            deadline: AtomicU64::new(NO_DEADLINE),
            budget: AtomicU64::new(UNLIMITED),
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

    /// Starts a budget of `budget`; it ends when the returned guard drops.
    pub(crate) fn enter(&self, budget: Duration) -> ExecutionGuard<'_> {
        let budget = u64::try_from(budget.as_nanos()).unwrap_or(UNLIMITED);
        self.budget.store(budget, Ordering::Relaxed);
        self.interrupt_requested.store(false, Ordering::Release);
        self.deadline.store(DEADLINE_PENDING, Ordering::Release);
        ExecutionGuard(self)
    }

    fn deadline_passed(&self) -> bool {
        match self.deadline.load(Ordering::Acquire) {
            NO_DEADLINE => false,
            DEADLINE_PENDING => {
                self.start_deadline();
                false
            }
            deadline => self.elapsed_nanos() >= deadline,
        }
    }

    /// Sets the running operation's deadline to its budget from now.
    fn start_deadline(&self) {
        let budget = self.budget.load(Ordering::Relaxed);
        let deadline = if budget == UNLIMITED {
            NO_DEADLINE
        } else {
            self.elapsed_nanos()
                .saturating_add(budget)
                .min(DEADLINE_PENDING - 1)
        };
        self.deadline.store(deadline, Ordering::Release);
    }

    fn elapsed_nanos(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(DEADLINE_PENDING - 1)
    }
}

pub(crate) struct ExecutionGuard<'a>(&'a ExecutionControl);

impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        self.0.deadline.store(NO_DEADLINE, Ordering::Release);
    }
}
