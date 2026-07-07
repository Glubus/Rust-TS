//! Native QuickJS value bridge helpers for schema-aware Rust types.

use std::sync::Arc;

use rquickjs::{
    Array, ArrayBuffer, Ctx, Error as JsError, Filter, FromJs, IntoJs, Object, Result as JsResult,
    TypedArray, Value as JsValue,
};
use serde::{Serialize, Serializer};
use serde_json::{Map, Number, Value};

use super::{Schema, TsType};

/// Byte payload that crosses the typed host bridge as a native-backed `Uint8Array`.
///
/// `NativeBytes` is an explicit opt-in for large byte outputs. The JSON-compatible
/// fallback serializes as a byte array, while the `callValue` typed bridge exposes
/// the same bytes as an immutable `Uint8Array` backed by native memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeBytes {
    bytes: Arc<[u8]>,
}

impl NativeBytes {
    /// Creates a native byte payload from owned bytes.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into().into(),
        }
    }

    /// Creates a native byte payload from shared bytes.
    #[must_use]
    pub fn from_shared(bytes: Arc<[u8]>) -> Self {
        Self { bytes }
    }

    /// Returns the bytes as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.bytes.as_ref()
    }

    /// Returns the shared backing bytes.
    #[must_use]
    pub fn into_shared(self) -> Arc<[u8]> {
        self.bytes
    }
}

impl From<Vec<u8>> for NativeBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::new(bytes)
    }
}

impl From<Box<[u8]>> for NativeBytes {
    fn from(bytes: Box<[u8]>) -> Self {
        Self {
            bytes: Arc::from(bytes),
        }
    }
}

impl From<Arc<[u8]>> for NativeBytes {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self::from_shared(bytes)
    }
}

impl AsRef<[u8]> for NativeBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Serialize for NativeBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(self.as_slice())
    }
}

impl super::TsSchema for NativeBytes {
    fn schema_name() -> &'static str {
        "NativeBytes"
    }

    fn ts_type() -> TsType {
        TsType::Uint8Array
    }

    fn schema() -> Schema {
        Schema::typed(Self::schema_name(), Self::ts_type())
    }

    fn __rustts_into_js_value<'js>(self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>>
    where
        Self: Sized + Serialize,
    {
        let buffer = ArrayBuffer::from_source_immutable(ctx.clone(), self.bytes)?;
        TypedArray::<u8>::from_arraybuffer(buffer).map(TypedArray::into_value)
    }
}

/// Converts one QuickJS value into a JSON value.
///
/// This remains useful as a compatibility layer for untyped host boundaries.
pub(crate) fn js_value_to_json<'js>(ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Value> {
    if value.is_null() || value.is_undefined() {
        return Ok(Value::Null);
    }
    if let Some(value) = value.as_bool() {
        return Ok(Value::Bool(value));
    }
    if let Some(value) = value.as_int() {
        return Ok(Value::Number(Number::from(value)));
    }
    if let Some(value) = value.as_float() {
        return Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| JsError::new_from_js_message("number", "json", "non-finite number"));
    }
    if value.is_string() {
        return String::from_js(ctx, value).map(Value::String);
    }
    if value.is_array() {
        let array = Array::from_js(ctx, value)?;
        let mut items = Vec::with_capacity(array.len());
        for item in array {
            items.push(js_value_to_json(ctx, item?)?);
        }
        return Ok(Value::Array(items));
    }
    if value.is_object() {
        let object = Object::from_js(ctx, value)?;
        let mut map = Map::new();
        for property in object.own_props::<String, JsValue<'_>>(Filter::default()) {
            let (key, value) = property?;
            map.insert(key, js_value_to_json(ctx, value)?);
        }
        return Ok(Value::Object(map));
    }
    Err(JsError::new_from_js_message(
        value.type_name(),
        "json",
        "unsupported host bridge value",
    ))
}

/// Converts one JSON value into a QuickJS value.
pub(crate) fn json_to_js_value<'js>(ctx: &Ctx<'js>, value: Value) -> JsResult<JsValue<'js>> {
    match value {
        Value::Null => Ok(JsValue::new_null(ctx.clone())),
        Value::Bool(value) => value.into_js(ctx),
        Value::Number(value) => number_to_js_value(ctx, value),
        Value::String(value) => value.into_js(ctx),
        Value::Array(values) => json_array_to_js_value(ctx, values),
        Value::Object(values) => {
            let object = Object::new(ctx.clone())?;
            for (key, value) in values {
                object.set(key, json_to_js_value(ctx, value)?)?;
            }
            Ok(object.into_value())
        }
    }
}

fn json_array_to_js_value<'js>(ctx: &Ctx<'js>, values: Vec<Value>) -> JsResult<JsValue<'js>> {
    let array = Array::new(ctx.clone())?;
    for (index, value) in values.into_iter().enumerate() {
        array.set(index, json_to_js_value(ctx, value)?)?;
    }
    Ok(array.into_object().into_value())
}

fn number_to_js_value<'js>(ctx: &Ctx<'js>, value: Number) -> JsResult<JsValue<'js>> {
    if let Some(value) = value.as_i64()
        && let Ok(value) = i32::try_from(value)
    {
        return value.into_js(ctx);
    }
    if let Some(value) = value.as_u64()
        && let Ok(value) = i32::try_from(value)
    {
        return value.into_js(ctx);
    }
    value
        .as_f64()
        .ok_or_else(|| JsError::new_from_js_message("json number", "number", "invalid number"))?
        .into_js(ctx)
}
