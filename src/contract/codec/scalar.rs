//! `bool` and `()`.

use rquickjs::{Ctx, Result as JsResult, Value as JsValue};

use super::{JsDecode, JsEncode, mismatch};

impl JsEncode for bool {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        Ok(JsValue::new_bool(ctx.clone(), *self))
    }
}

impl JsDecode for bool {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        value
            .as_bool()
            .ok_or_else(|| mismatch(&value, "bool", "boolean"))
    }
}

/// `()` is `null`, like `serde_json`; `undefined` is accepted because a function
/// without a return value produces it.
impl JsEncode for () {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        Ok(JsValue::new_null(ctx.clone()))
    }
}

impl JsDecode for () {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        if value.is_null() || value.is_undefined() {
            Ok(())
        } else {
            Err(mismatch(&value, "()", "null"))
        }
    }
}
