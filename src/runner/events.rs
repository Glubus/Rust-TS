//! The engine's copy of each context's event handler lists, and their delivery.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use rquickjs::{Array, CatchResultExt, Ctx, Function, Value as JsValue, qjs};

use super::errors::{caught_js_error, js_error};
use crate::error::VmError;

/// Handler lists of one context, by event name, as the prelude hands them over: only
/// events with at least one handler are listed. Each list is kept as a snapshot of
/// the functions themselves, so delivering an event creates no property key, reads
/// no global and no array, and a context without handlers for the event is not
/// entered at all. A change replaces the snapshot: a delivery in progress keeps
/// running the one it took.
///
/// The script owns the only strong reference. The prelude's hook holds a weak one:
/// the snapshots keep the context's functions alive, so a strong reference from
/// inside the context would be a cycle the garbage collector cannot see.
#[derive(Default)]
pub(super) struct ListenedEvents(Rc<RefCell<Vec<Listened>>>);

/// The handlers of one event, in registration order.
pub(super) type Handlers = Rc<[RawFunction]>;

/// A JavaScript function the engine keeps and calls without cloning anything.
///
/// A `Persistent<Function>` has to be cloned and restored to be called, which takes and
/// gives back two references (the function and its context) per call. This holds one
/// reference to the function and calls it through the C API with borrowed values. It
/// does not keep its context alive: a QuickJS function holds its own realm.
///
/// Like a `Persistent`, it must be dropped before the runtime it belongs to, on the
/// thread that owns the runtime; it is neither `Send` nor `Sync`, and a function leaked
/// past the runtime aborts it when it drops, as a leaked `Persistent` does.
pub(super) struct RawFunction {
    rt: *mut qjs::JSRuntime,
    value: qjs::JSValue,
}

impl RawFunction {
    fn new(function: &Function<'_>) -> Self {
        let value = function.as_value();
        let ctx = value.ctx().as_raw().as_ptr();
        // SAFETY: `ctx` is the live context `function` belongs to, and `as_raw` is a
        // value of it: the duplicate this takes is the reference `Drop` gives back.
        unsafe {
            Self {
                rt: qjs::JS_GetRuntime(ctx),
                value: qjs::JS_DupValue(ctx, value.as_raw()),
            }
        }
    }
}

impl Drop for RawFunction {
    fn drop(&mut self) {
        // SAFETY: `value` holds the reference `new` took, and the runtime is alive: see
        // the type's contract.
        unsafe { qjs::JS_FreeValueRT(self.rt, self.value) }
    }
}

struct Listened {
    event: Box<str>,
    handlers: Handlers,
}

impl ListenedEvents {
    /// Whether the script has a handler for `event`, without taking its handlers.
    pub(super) fn listens(&self, event: &str) -> bool {
        self.0
            .borrow()
            .iter()
            .any(|listened| &*listened.event == event)
    }

    /// The current handlers of `event`, `None` when the context has none. The borrow
    /// ends before any JavaScript runs, so handlers may call `ctx.on` and `ctx.off`.
    #[inline]
    pub(super) fn handlers(&self, event: &str) -> Option<Handlers> {
        self.0
            .borrow()
            .iter()
            .find(|listened| &*listened.event == event)
            .map(|listened| Rc::clone(&listened.handlers))
    }

    /// The hook the prelude hands handler lists to.
    pub(super) fn hook<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, VmError> {
        let events = Rc::downgrade(&self.0);
        Function::new(
            ctx.clone(),
            move |event: String, handlers: Option<Array<'_>>| {
                let handlers = handlers.map(|list| snapshot(&list)).transpose()?;
                record(&events, event, handlers);
                rquickjs::Result::Ok(())
            },
        )
        .map_err(js_error)
    }
}

/// The functions of a handler list the prelude built; it only holds functions.
fn snapshot(list: &Array<'_>) -> rquickjs::Result<Handlers> {
    list.iter::<Function<'_>>()
        .map(|handler| handler.map(|handler| RawFunction::new(&handler)))
        .collect()
}

/// Stores `handlers` as the list of `event`, or forgets the event when `None`.
fn record(events: &Weak<RefCell<Vec<Listened>>>, event: String, handlers: Option<Handlers>) {
    let Some(events) = events.upgrade() else {
        return;
    };
    let mut events = events.borrow_mut();
    let position = events.iter().position(|listened| *listened.event == *event);
    match (position, handlers) {
        (Some(position), Some(handlers)) => events[position].handlers = handlers,
        (None, Some(handlers)) => events.push(Listened {
            event: event.into_boxed_str(),
            handlers,
        }),
        (Some(position), None) => {
            events.swap_remove(position);
        }
        (None, None) => {}
    }
}

/// Runs every handler with `payload`, the encoded event, even after one throws; returns
/// the first error of a handler or of `on_return`. With `keep_results`, each returned
/// value goes to `on_return`; without, it is dropped unread, which skips building a
/// value around it.
///
/// The handler and the payload are only borrowed for each call: delivering to a handler
/// takes no reference, where `Function::call` would take and release several.
pub(super) fn deliver<'js>(
    ctx: &Ctx<'js>,
    handlers: &[RawFunction],
    payload: &JsValue<'js>,
    keep_results: bool,
    mut on_return: impl FnMut(JsValue<'js>) -> Result<(), VmError>,
) -> Result<(), VmError> {
    let mut first_error = None;
    let mut arguments = [payload.as_raw()];
    for handler in handlers {
        // SAFETY: `handler.value` is a live function of this runtime and `arguments`
        // holds the payload, a value of `ctx`: `JS_Call` borrows both, and `this` is
        // `undefined`, which needs no release.
        let returned = unsafe {
            qjs::JS_Call(
                ctx.as_raw().as_ptr(),
                handler.value,
                qjs::JS_UNDEFINED,
                1,
                arguments.as_mut_ptr(),
            )
        };
        // SAFETY: `returned` is the value `JS_Call` just gave back.
        let outcome = if unsafe { qjs::JS_IsException(returned) } {
            Err(rquickjs::Error::Exception)
                .catch(ctx)
                .map_err(caught_js_error)
        } else if keep_results {
            // SAFETY: `returned` is an owned value of `ctx`, which the wrapper releases.
            on_return(unsafe { JsValue::from_raw(ctx.clone(), returned) })
        } else {
            // SAFETY: `returned` is an owned value of `ctx` that nothing else releases;
            // `undefined`, the usual result, holds no reference and is skipped.
            unsafe {
                if !qjs::JS_IsUndefined(returned) {
                    qjs::JS_FreeValue(ctx.as_raw().as_ptr(), returned);
                }
            }
            Ok(())
        };
        if let Err(error) = outcome {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}
