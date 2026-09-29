//! Script timers (`setTimeout`, `setInterval`) on a clock the host advances.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use rquickjs::prelude::Func;
use rquickjs::{CatchResultExt, Ctx, Function, Object};

use super::errors::{caught_js_error, js_error};
use crate::error::VmError;

/// Reads the engine clock, in milliseconds.
const NOW_GLOBAL: &str = "__rustts_now";
/// Records the due time of the context's earliest timer.
const SCHEDULE_GLOBAL: &str = "__rustts_schedule";
/// Frozen `{ run }` hook the prelude installs to fire due timers.
const TIMERS_GLOBAL: &str = "__rustts_timers";
const TIMERS_RUN: &str = "run";

/// Engine time in milliseconds, advanced only by
/// [`Engine::advance_timers`](crate::Engine::advance_timers); starts at 0.
#[derive(Clone, Default)]
pub(super) struct TimerClock(Rc<Cell<f64>>);

impl TimerClock {
    /// Moves the clock forward by `elapsed`; returns the new time.
    pub(super) fn advance(&self, elapsed: Duration) -> f64 {
        let now = self.0.get() + elapsed.as_secs_f64() * 1000.0;
        self.0.set(now);
        now
    }
}

/// Due time of a context's earliest timer, so the engine enters only contexts with a
/// timer to fire.
#[derive(Clone)]
pub(super) struct NextTimer(Rc<Cell<f64>>);

impl Default for NextTimer {
    fn default() -> Self {
        Self(Rc::new(Cell::new(f64::INFINITY)))
    }
}

impl NextTimer {
    pub(super) fn is_due(&self, now: f64) -> bool {
        self.0.get() <= now
    }
}

/// Installs the clock and scheduling hooks the prelude builds the timer functions on.
pub(super) fn install_timer_hooks(
    ctx: &Ctx<'_>,
    clock: &TimerClock,
    next: &NextTimer,
) -> Result<(), VmError> {
    let globals = ctx.globals();
    let clock = Rc::clone(&clock.0);
    globals
        .set(NOW_GLOBAL, Func::from(move || clock.get()))
        .map_err(js_error)?;
    let next = Rc::clone(&next.0);
    globals
        .set(SCHEDULE_GLOBAL, Func::from(move |due: f64| next.set(due)))
        .map_err(js_error)
}

/// Fires the context's timers due at `now`, through the prelude's locked hook.
pub(super) fn run_due_timers(ctx: &Ctx<'_>, now: f64) -> Result<(), VmError> {
    ctx.globals()
        .get::<_, Object<'_>>(TIMERS_GLOBAL)
        .and_then(|timers| timers.get::<_, Function<'_>>(TIMERS_RUN))
        .map_err(js_error)?
        .call::<_, ()>((now,))
        .catch(ctx)
        .map_err(caught_js_error)
}
