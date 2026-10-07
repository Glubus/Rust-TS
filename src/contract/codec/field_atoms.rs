//! Field-name atoms of the derived encoders, cached for the runtime's lifetime.
//!
//! Setting `object["lane"]` from a Rust `&str` makes QuickJS hash the name and look
//! it up in its atom table on every call, about as costly as the property store itself.
//! Derived field names are `&'static str`, so their atoms are looked up once per runtime
//! and then found by the name's address.

use std::cell::RefCell;
use std::ptr::NonNull;

use rquickjs::{
    Ctx, Error as JsError, JsLifetime, Object, Result as JsResult, Value as JsValue, qjs,
};
use rustc_hash::FxHashMap;

use super::define_atom_property;

/// Atoms by field-name address and length, owned until the runtime is freed.
struct FieldAtoms {
    runtime: NonNull<qjs::JSRuntime>,
    atoms: RefCell<FxHashMap<(usize, usize), qjs::JSAtom>>,
}

// SAFETY: `FieldAtoms` holds no value tied to a `'js` lifetime: its atoms belong to the
// runtime, not to a context, and are released in `Drop`, which rquickjs runs when it
// clears the runtime's userdata, before freeing the runtime.
unsafe impl<'js> JsLifetime<'js> for FieldAtoms {
    type Changed<'to> = FieldAtoms;
}

impl Drop for FieldAtoms {
    fn drop(&mut self) {
        for atom in self.atoms.get_mut().values() {
            // SAFETY: each atom was created on this runtime, which is still alive (see
            // the `JsLifetime` impl), and is released exactly once.
            unsafe { qjs::JS_FreeAtomRT(self.runtime.as_ptr(), *atom) };
        }
    }
}

/// Defines `object[name] = value` as an own enumerable, writable, configurable data
/// property, as `JSON.parse` would. `object` must be a plain object the encoder just
/// created: defining on it runs no script code, not even an inherited setter.
pub(super) fn define_field<'js>(
    ctx: &Ctx<'js>,
    object: &Object<'js>,
    name: &'static str,
    value: JsValue<'js>,
) -> JsResult<()> {
    define_atom_property(object, field_atom(ctx, name)?, value)
}

/// The atom of `name`, created on first use in this runtime.
fn field_atom(ctx: &Ctx<'_>, name: &'static str) -> JsResult<qjs::JSAtom> {
    let key = (name.as_ptr() as usize, name.len());
    if let Some(cache) = ctx.userdata::<FieldAtoms>() {
        if let Some(atom) = cache.atoms.borrow().get(&key) {
            return Ok(*atom);
        }
        let atom = new_atom(ctx, name)?;
        cache.atoms.borrow_mut().insert(key, atom);
        return Ok(atom);
    }
    let atom = new_atom(ctx, name)?;
    // SAFETY: `ctx` is a live context, so its runtime pointer is valid and non-null.
    let runtime = unsafe { NonNull::new_unchecked(qjs::JS_GetRuntime(ctx.as_raw().as_ptr())) };
    let mut atoms = FxHashMap::default();
    atoms.insert(key, atom);
    ctx.store_userdata(FieldAtoms {
        runtime,
        atoms: RefCell::new(atoms),
    })
    .map_err(|_| JsError::Unknown)?;
    Ok(atom)
}

fn new_atom(ctx: &Ctx<'_>, name: &str) -> JsResult<qjs::JSAtom> {
    // SAFETY: `name` is valid UTF-8 of `name.len()` bytes; QuickJS copies it.
    let atom =
        unsafe { qjs::JS_NewAtomLen(ctx.as_raw().as_ptr(), name.as_ptr().cast(), name.len() as _) };
    if atom == qjs::JS_ATOM_NULL {
        return Err(JsError::Exception);
    }
    Ok(atom)
}
