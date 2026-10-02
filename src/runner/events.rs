//! The engine's copy of each context's event handler lists, and their delivery.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use rquickjs::{Array, CatchResultExt, Ctx, Function, Persistent, Value as JsValue};

use super::errors::{caught_js_error, js_error};
use crate::contract::JsEncode;
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
pub(super) type Handlers = Rc<[Persistent<Function<'static>>]>;

struct Listened {
    event: Box<str>,
    handlers: Handlers,
}

impl ListenedEvents {
    /// The current handlers of `event`, `None` when the context has none. The borrow
    /// ends before any JavaScript runs, so handlers may call `ctx.on` and `ctx.off`.
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
    let ctx = list.ctx();
    list.iter::<Function<'_>>()
        .map(|handler| handler.map(|handler| Persistent::save(ctx, handler)))
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

/// Runs every handler, even after one throws, and hands each returned value to
/// `on_return`; returns the first error of a handler or of `on_return`. The payload
/// is encoded once and shared by all of them.
pub(super) fn deliver<'js, P: JsEncode + ?Sized>(
    ctx: &Ctx<'js>,
    handlers: &[Persistent<Function<'static>>],
    payload: &P,
    mut on_return: impl FnMut(JsValue<'js>) -> Result<(), VmError>,
) -> Result<(), VmError> {
    let payload = payload.encode_js(ctx).map_err(js_error)?;
    let mut first_error = None;
    for handler in handlers {
        let outcome = handler
            .clone()
            .restore(ctx)
            .map_err(js_error)
            .and_then(|handler| {
                handler
                    .call::<_, JsValue<'js>>((payload.clone(),))
                    .catch(ctx)
                    .map_err(caught_js_error)
            })
            .and_then(&mut on_return);
        if let Err(error) = outcome {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}
