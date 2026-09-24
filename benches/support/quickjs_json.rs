//! Native serde_json <-> QuickJS conversion for the "QuickJS direct" baselines.

use rquickjs::{Array, Ctx, Object, Value as JsValue};
use serde_json::{Value, json};

pub fn json_to_js<'js>(ctx: &Ctx<'js>, value: &Value) -> rquickjs::Result<JsValue<'js>> {
    Ok(match value {
        Value::Null => JsValue::new_null(ctx.clone()),
        Value::Bool(flag) => JsValue::new_bool(ctx.clone(), *flag),
        Value::Number(number) => JsValue::new_number(ctx.clone(), number.as_f64().unwrap_or(0.0)),
        Value::String(text) => rquickjs::String::from_str(ctx.clone(), text)?.into_value(),
        Value::Array(items) => array_to_js(ctx, items)?,
        Value::Object(fields) => object_to_js(ctx, fields)?,
    })
}

fn array_to_js<'js>(ctx: &Ctx<'js>, items: &[Value]) -> rquickjs::Result<JsValue<'js>> {
    let array = Array::new(ctx.clone())?;
    for (index, item) in items.iter().enumerate() {
        array.set(index, json_to_js(ctx, item)?)?;
    }
    Ok(array.into_value())
}

fn object_to_js<'js>(
    ctx: &Ctx<'js>,
    fields: &serde_json::Map<String, Value>,
) -> rquickjs::Result<JsValue<'js>> {
    let object = Object::new(ctx.clone())?;
    for (key, field) in fields {
        object.set(key.as_str(), json_to_js(ctx, field)?)?;
    }
    Ok(object.into_value())
}

pub fn js_to_json(value: &JsValue<'_>) -> rquickjs::Result<Value> {
    if let Some(number) = value.as_number() {
        return Ok(json!(number));
    }
    if let Some(flag) = value.as_bool() {
        return Ok(Value::Bool(flag));
    }
    if let Some(text) = value.as_string() {
        return Ok(Value::String(text.to_string()?));
    }
    if let Some(array) = value.as_array() {
        return array_to_json(array);
    }
    if let Some(object) = value.as_object() {
        return object_to_json(object);
    }
    Ok(Value::Null)
}

fn array_to_json(array: &Array<'_>) -> rquickjs::Result<Value> {
    array
        .iter::<JsValue<'_>>()
        .map(|item| js_to_json(&item?))
        .collect::<rquickjs::Result<Vec<_>>>()
        .map(Value::Array)
}

fn object_to_json(object: &Object<'_>) -> rquickjs::Result<Value> {
    let mut fields = serde_json::Map::new();
    for entry in object.props::<String, JsValue<'_>>() {
        let (key, field) = entry?;
        fields.insert(key, js_to_json(&field)?);
    }
    Ok(Value::Object(fields))
}
