//! `Uuid` crosses as a string: hyphenated lowercase on encode; decode accepts every
//! form `Uuid`'s `serde` impl reads from a JSON string (simple, hyphenated, braced, URN).

use ::uuid::Uuid;
use rquickjs::{Ctx, Result as JsResult, Value as JsValue};

use super::super::text::decode_parsed;
use super::super::{JsDecode, JsEncode};
use crate::contract::{TsSchema, TsType};

impl JsEncode for Uuid {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        self.hyphenated()
            .encode_lower(&mut Uuid::encode_buffer())
            .encode_js(ctx)
    }
}

impl JsDecode for Uuid {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        decode_parsed(value, "Uuid", "a UUID")
    }
}

impl TsSchema for Uuid {
    fn ts_type() -> TsType {
        TsType::String
    }
}
