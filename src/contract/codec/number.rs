//! Numbers. Integers cross exactly or fail; floats cross as-is, `NaN` and infinities
//! included.

use std::fmt::Display;

use rquickjs::{Ctx, Result as JsResult, Value as JsValue};

use super::stack_text::StackText;
use super::{JsDecode, JsEncode, encode_error, mismatch};

/// Largest integer JavaScript numbers represent exactly: 2^53 − 1.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Every `f32` integer up to this magnitude prints as itself, so it widens exactly.
const F32_EXACT_INTEGER_LIMIT: f64 = 16_777_216.0;

/// Reads `value` as an `i64` when it is an integer within the safe range.
pub(crate) fn exact_integer(value: f64) -> Option<i64> {
    let in_range = value.abs() <= MAX_SAFE_INTEGER as f64;
    (in_range && value.trunc() == value).then_some(value as i64)
}

macro_rules! integer_codecs {
    ($expected:literal: $($ty:ty),+ $(,)?) => {
        $(
            impl JsEncode for $ty {
                fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
                    encode_integer(ctx, *self, stringify!($ty))
                }
            }

            impl JsDecode for $ty {
                fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
                    decode_integer(
                        &value,
                        stringify!($ty),
                        concat!($expected, " in ", stringify!($ty), " range"),
                    )
                }
            }
        )+
    };
}

integer_codecs!("integer": i8, i16, i32, u8, u16, u32);
integer_codecs!("safe integer": i64, i128, isize, u64, u128, usize);

fn encode_integer<'js, T>(ctx: &Ctx<'js>, value: T, rust: &'static str) -> JsResult<JsValue<'js>>
where
    T: Copy + Display,
    i32: TryFrom<T>,
    i64: TryFrom<T>,
{
    if let Ok(small) = i32::try_from(value) {
        return Ok(JsValue::new_int(ctx.clone(), small));
    }
    match i64::try_from(value) {
        Ok(wide) if wide.unsigned_abs() <= MAX_SAFE_INTEGER => {
            Ok(JsValue::new_float(ctx.clone(), wide as f64))
        }
        _ => Err(encode_error(
            rust,
            "number",
            format!("{value} is outside the JavaScript safe integer range ±(2^53 - 1)"),
        )),
    }
}

fn decode_integer<T>(value: &JsValue<'_>, rust: &'static str, expected: &str) -> JsResult<T>
where
    T: TryFrom<i64>,
{
    safe_integer(value)
        .and_then(|wide| T::try_from(wide).ok())
        .ok_or_else(|| mismatch(value, rust, expected))
}

fn safe_integer(value: &JsValue<'_>) -> Option<i64> {
    match value.as_int() {
        Some(int) => Some(i64::from(int)),
        None => exact_integer(value.as_float()?),
    }
}

impl JsEncode for f64 {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        Ok(number_value(ctx, *self))
    }
}

impl JsDecode for f64 {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        decode_number(&value, "f64")
    }
}

impl JsEncode for f32 {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        Ok(number_value(ctx, widen_f32(*self)))
    }
}

impl JsDecode for f32 {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        decode_number(&value, "f32").map(|number| number as f32)
    }
}

fn decode_number(value: &JsValue<'_>, rust: &'static str) -> JsResult<f64> {
    value
        .as_number()
        .ok_or_else(|| mismatch(value, rust, "number"))
}

/// Integral values become QuickJS ints; `-0.0` stays a float so its sign survives.
fn number_value<'js>(ctx: &Ctx<'js>, value: f64) -> JsValue<'js> {
    if value == 0.0 && value.is_sign_negative() {
        JsValue::new_float(ctx.clone(), value)
    } else {
        JsValue::new_number(ctx.clone(), value)
    }
}

/// Widens `value` like `serde_json` text does: the shortest decimal that identifies the
/// `f32`, read back as an `f64` (`1.1f32` crosses as `1.1`, not `1.100000023841858`).
fn widen_f32(value: f32) -> f64 {
    let exact = f64::from(value);
    if !value.is_finite() || (exact.trunc() == exact && exact.abs() <= F32_EXACT_INTEGER_LIMIT) {
        return exact;
    }
    StackText::<32>::format(format_args!("{value:e}"))
        .and_then(|text| text.as_str().parse().ok())
        .unwrap_or(exact)
}
