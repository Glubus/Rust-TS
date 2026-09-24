//! `serde_json::Value`, converted structurally without JSON text.

use rquickjs::{Ctx, Result as JsResult, Value as JsValue};
use serde_json::Value;

use super::{JsDecode, JsEncode};
use crate::contract::{js_value_to_json, json_to_js_value};

impl JsEncode for Value {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        json_to_js_value(ctx, self)
    }
}

impl JsDecode for Value {
    fn decode_js<'js>(ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        js_value_to_json(ctx, value)
    }
}
