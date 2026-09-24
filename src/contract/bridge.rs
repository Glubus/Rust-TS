//! Structural conversion between QuickJS values and `serde_json` values, without JSON
//! text. Untyped host boundaries use it directly; `serde_json::Value` codecs wrap it.

use rquickjs::{
    Array, Ctx, Error as JsError, Filter, FromJs, IntoJs, Object, Result as JsResult,
    String as JsString, Value as JsValue,
};
use serde_json::{Map, Number, Value};

use super::JsEncode;
use super::codec::{array_length, at_index, at_path, cautious_capacity, exact_integer};

/// Converts one QuickJS value into a JSON value.
///
/// Integral numbers within the JavaScript safe range become integer JSON numbers, as
/// `JSON.stringify` followed by `serde_json` would produce.
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
        return float_to_json(value);
    }
    if value.is_string() {
        return String::from_js(ctx, value).map(Value::String);
    }
    if value.is_array() {
        return array_to_json(ctx, &Array::from_js(ctx, value)?);
    }
    if value.is_object() {
        return object_to_json(ctx, &Object::from_js(ctx, value)?);
    }
    Err(JsError::new_from_js_message(
        value.type_name(),
        "json",
        "unsupported host bridge value",
    ))
}

fn array_to_json<'js>(ctx: &Ctx<'js>, array: &Array<'js>) -> JsResult<Value> {
    let len = array_length(array, "json")?;
    let mut items = Vec::with_capacity(cautious_capacity::<Value>(len));
    for index in 0..len {
        let item =
            js_value_to_json(ctx, array.get(index)?).map_err(|error| at_index(error, index))?;
        items.push(item);
    }
    Ok(Value::Array(items))
}

fn object_to_json<'js>(ctx: &Ctx<'js>, object: &Object<'js>) -> JsResult<Value> {
    let mut map = Map::new();
    for property in object.own_props::<JsString<'js>, JsValue<'js>>(Filter::default()) {
        let (key, value) = property?;
        let key = key.to_string()?;
        let value = js_value_to_json(ctx, value)
            .map_err(|error| at_path(error, format_args!("[{key}]")))?;
        map.insert(key, value);
    }
    Ok(Value::Object(map))
}

fn float_to_json(value: f64) -> JsResult<Value> {
    if let Some(integer) = exact_integer(value) {
        return Ok(Value::Number(Number::from(integer)));
    }
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| JsError::new_from_js_message("number", "json", "non-finite number"))
}

/// Converts one JSON value into a QuickJS value.
pub(crate) fn json_to_js_value<'js>(ctx: &Ctx<'js>, value: &Value) -> JsResult<JsValue<'js>> {
    match value {
        Value::Null => Ok(JsValue::new_null(ctx.clone())),
        Value::Bool(value) => value.into_js(ctx),
        Value::Number(value) => number_to_js_value(ctx, value),
        Value::String(value) => value.as_str().into_js(ctx),
        Value::Array(values) => json_array_to_js_value(ctx, values),
        Value::Object(values) => {
            let object = Object::new(ctx.clone())?;
            for (key, value) in values {
                object.set(key.as_str(), json_to_js_value(ctx, value)?)?;
            }
            Ok(object.into_value())
        }
    }
}

fn json_array_to_js_value<'js>(ctx: &Ctx<'js>, values: &[Value]) -> JsResult<JsValue<'js>> {
    let array = Array::new(ctx.clone())?;
    for (index, value) in values.iter().enumerate() {
        array.set(index, json_to_js_value(ctx, value)?)?;
    }
    Ok(array.into_object().into_value())
}

/// Integer numbers follow the integer codecs, so values outside the safe range fail
/// instead of rounding.
fn number_to_js_value<'js>(ctx: &Ctx<'js>, value: &Number) -> JsResult<JsValue<'js>> {
    if let Some(value) = value.as_i64() {
        return value.encode_js(ctx);
    }
    if let Some(value) = value.as_u64() {
        return value.encode_js(ctx);
    }
    value
        .as_f64()
        .ok_or_else(|| JsError::new_from_js_message("json number", "number", "invalid number"))?
        .encode_js(ctx)
}
