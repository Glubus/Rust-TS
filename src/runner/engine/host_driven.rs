//! Work the host drives: timers, Promise pumping and deferred calls and requests.

use std::time::Duration;

use rquickjs::{CatchResultExt, Value as JsValue};

use crate::contract::{JsArgs, JsDecode, JsEncode};
use crate::error::VmError;
use crate::types::ScriptId;

use super::super::errors::{caught_js_error, js_error};
use super::super::tasks::{PendingCall, RequestGuard, RequestResults, ScriptTask};
use super::super::timers::run_due_timers;
use super::Engine;

impl Engine {
    /// Moves the engine clock forward by `elapsed` and fires the timers now due, script
    /// by script in load order, each script's in due order; returns the number of
    /// scripts that had timers to fire. Call it from the host loop, typically once per
    /// frame with the frame time; nothing else moves the clock, so timers are
    /// deterministic.
    ///
    /// A timer fires at most once per call: one a callback schedules waits for the
    /// next call even with a zero delay, and an interval late by several periods fires
    /// once, its next due time staying its previous one plus its delay. A throwing
    /// callback does not stop the others; the first error is returned. Timers belong
    /// to the script version that set them: a reload or unload drops them.
    pub fn advance_timers(&self, elapsed: Duration) -> Result<usize, VmError> {
        let now = self.timer_clock.advance(elapsed);
        let due = self
            .scripts
            .values()
            .filter(|script| script.signals.next_timer.is_due(now));
        let _budget = self.budget();
        let mut fired = 0;
        let mut first_error = None;
        for script in due {
            fired += 1;
            if let Err(error) = script.context.with(|ctx| run_due_timers(&ctx, now)) {
                first_error.get_or_insert(error);
            }
        }
        if fired == 0 {
            // No JavaScript ran, so there is no Promise job to settle.
            return Ok(0);
        }
        self.attribute_interrupt(self.settle(first_error.map_or(Ok(fired), Err)))
    }

    /// Starts an exported function without requiring its Promise to settle in this
    /// operation. The returned handle completes when the engine next drains jobs;
    /// `pump` delivers host replies without advancing the timer clock.
    pub fn call_deferred<R: JsDecode + 'static>(
        &self,
        script_id: &str,
        function: &str,
        args: impl JsArgs,
    ) -> Result<PendingCall<R>, VmError> {
        let script = self.script(script_id)?;
        let _budget = self.budget();
        let result = script.context.with(|ctx| {
            let export = script.export(&ctx, script_id, function)?;
            let args = args.encode_args(&ctx).map_err(js_error)?;
            let returned = export
                .call_arg::<JsValue<'_>>(args)
                .catch(&ctx)
                .map_err(caught_js_error)?;
            let handle = PendingCall::new();
            if let Some(task) = ScriptTask::observe(&ctx, returned, &handle)? {
                script.tasks.borrow_mut().push(task);
                self.active_tasks.set(self.active_tasks.get() + 1);
            }
            Ok(handle)
        });
        self.attribute_interrupt(self.settle(result))
    }

    /// Delivers queued host replies on the engine thread and drains their Promise
    /// jobs under one execution budget. The host clock is not advanced.
    pub fn pump(&self) -> Result<(), VmError> {
        let _budget = self.budget();
        let mut first_error = None;
        for script in self.scripts.values() {
            if self.execution.interrupted() {
                break;
            }
            let result = script.context.with(|ctx| {
                script
                    .host_promises
                    .poll(&ctx)
                    .catch(&ctx)
                    .map_err(caught_js_error)
            });
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
        self.attribute_interrupt(self.settle(first_error.map_or(Ok(()), Err)))
    }

    /// Starts a request whose handlers may reply in later host-driven operations.
    /// Completed replies retain script and registration order, not resolution order.
    /// A handler error is returned through the handle after all replies finish.
    pub fn request_deferred<P: JsEncode + ?Sized, R: JsDecode + 'static>(
        &self,
        event: &str,
        payload: &P,
    ) -> Result<PendingCall<Vec<(ScriptId, R)>>, VmError> {
        let handle = PendingCall::new();
        let replies = RequestResults::new(handle.clone());
        let delivery = self.dispatch(event, payload, |script_id, ctx, value| {
            let script = self.script(script_id)?;
            script
                .request_guards
                .borrow_mut()
                .push(RequestGuard::new(&replies));
            if let Some(task) = RequestResults::observe(&replies, ctx, script_id, value)? {
                script.tasks.borrow_mut().push(task);
                self.active_tasks.set(self.active_tasks.get() + 1);
            }
            Ok(())
        });
        if let Err(error) = delivery {
            replies.borrow_mut().fail(error);
        }
        replies.borrow_mut().activate();
        for script in self.scripts.values() {
            script
                .request_guards
                .borrow_mut()
                .retain(RequestGuard::is_pending);
        }
        Ok(handle)
    }

    pub(super) fn harvest_tasks(&self) {
        if self.active_tasks.get() == 0 {
            return;
        }
        for script in self.scripts.values() {
            let mut tasks = script.tasks.borrow_mut();
            if !tasks.is_empty() {
                let before = tasks.len();
                script.context.with(|ctx| {
                    tasks.retain_mut(|task| !task.poll(&ctx, &self.rejections, &self.module_store));
                });
                self.active_tasks
                    .set(self.active_tasks.get() - (before - tasks.len()));
            }
            script
                .request_guards
                .borrow_mut()
                .retain(RequestGuard::is_pending);
        }
    }
}
