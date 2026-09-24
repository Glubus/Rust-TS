//! Native conversion between Rust values and QuickJS values.
//!
//! Reference semantics: a value crosses the boundary exactly as if Rust had serialized
//! it with `serde_json` and JavaScript had called `JSON.parse` on the text, and the
//! reverse (`JSON.stringify`, then `serde_json`). The native path is an optimization
//! of that reference, never a different contract. Deliberate differences, all stricter
//! or more exact than JSON text:
//!
//! - integers outside the JavaScript safe range (±(2^53 − 1)) fail instead of rounding;
//! - non-finite `f32`/`f64` values cross as `NaN`/`Infinity` instead of `null`;
//! - byte types marked for it cross as `Uint8Array`.

mod args;
mod bytes;
mod collections;
pub mod derive;
mod js_text;
mod json;
mod maps;
mod number;
mod scalar;
mod stack_text;
mod text;
mod tuples;
mod wrappers;

use std::fmt::{self, Display};

use rquickjs::atom::PredefinedAtom;
use rquickjs::function::Args;
use rquickjs::{Array, Ctx, Error as JsError, Object, Result as JsResult, Value as JsValue};

pub(crate) use number::exact_integer;

/// Upper bound, in bytes, on memory reserved up front from a length the other side
/// controls; larger collections still decode, growing as items arrive.
const MAX_PREALLOCATED_BYTES: usize = 1 << 20;

/// Capacity to reserve for `len` items of `T` announced by untrusted input.
pub(crate) fn cautious_capacity<T>(len: usize) -> usize {
    len.min(MAX_PREALLOCATED_BYTES / size_of::<T>().max(1))
}

/// Converts a Rust value into a QuickJS value.
///
/// Implemented for primitives, standard containers and every type deriving
/// [`TsSchema`](crate::TsSchema) together with `serde::Serialize`.
pub trait JsEncode {
    /// Builds the JavaScript representation of `self`.
    ///
    /// # Errors
    ///
    /// Fails when the value has no faithful JavaScript representation, such as an
    /// integer outside the safe range, or when QuickJS cannot allocate the result.
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>>;
}

/// Converts a QuickJS value into a Rust value.
///
/// Implemented for primitives, standard containers and every type deriving
/// [`TsSchema`](crate::TsSchema) together with `serde::Deserialize`.
pub trait JsDecode: Sized {
    /// Reads `value` as `Self`.
    ///
    /// # Errors
    ///
    /// Fails when `value` does not have the shape `serde_json` would accept for
    /// `Self`, or when a number does not fit the target type exactly.
    fn decode_js<'js>(ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self>;
}

/// Argument list for calling a JavaScript function from Rust.
///
/// Tuples of [`JsEncode`] values pass one argument per element; `Vec<T>` and slices
/// pass one argument per item.
pub trait JsArgs {
    /// Encodes every argument.
    ///
    /// # Errors
    ///
    /// Fails when one argument cannot be encoded.
    fn encode_args<'js>(&self, ctx: &Ctx<'js>) -> JsResult<Args<'js>>;
}

/// Builds a conversion error. `expected` names the Rust-side shape.
#[doc(hidden)]
pub fn codec_error(
    from: &'static str,
    expected: &'static str,
    message: impl Into<String>,
) -> JsError {
    JsError::new_from_js_message(from, expected, message.into())
}

/// Builds the error for a Rust value that has no faithful JavaScript representation.
pub(crate) fn encode_error(from: &'static str, to: &'static str, message: String) -> JsError {
    JsError::new_into_js_message(from, to, message)
}

/// Builds the error for a JavaScript value that does not have the shape `rust` needs,
/// reading `expected <shape>, got <value>`.
pub(crate) fn mismatch(value: &JsValue<'_>, rust: &'static str, expected: &str) -> JsError {
    codec_error(
        value.type_name(),
        rust,
        format!("expected {expected}, got {}", Got(value)),
    )
}

/// Short description of a JavaScript value for error messages: numbers show their
/// value, anything else its type.
struct Got<'a, 'js>(&'a JsValue<'js>);

impl Display for Got<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.as_number() {
            Some(number) => write!(formatter, "{number}"),
            None => formatter.write_str(self.0.type_name()),
        }
    }
}

/// Prefixes a conversion error with the field, index or key where it happened, so nested
/// failures read like `position.x: expected a finite number`.
///
/// Pass bare field names (`"x"`) and bracketed indices or keys (`"[3]"`); nested
/// segments are joined with `.` automatically.
#[doc(hidden)]
pub fn at_path(error: JsError, segment: impl Display) -> JsError {
    match error {
        JsError::FromJs { from, to, message } => JsError::FromJs {
            from,
            to,
            message: Some(prefix_path(segment, message, from, to)),
        },
        JsError::IntoJs { from, to, message } => JsError::IntoJs {
            from,
            to,
            message: Some(prefix_path(segment, message, from, to)),
        },
        other => other,
    }
}

/// Prefixes the error at array or tuple position `index` with `[index]`.
pub(crate) fn at_index(error: JsError, index: usize) -> JsError {
    at_path(error, format_args!("[{index}]"))
}

fn prefix_path(segment: impl Display, message: Option<String>, from: &str, to: &str) -> String {
    let message = message.unwrap_or_else(|| format!("expected {to}, got {from}"));
    if !has_path(&message) {
        format!("{segment}: {message}")
    } else if message.starts_with('[') {
        format!("{segment}{message}")
    } else {
        format!("{segment}.{message}")
    }
}

/// Whether `message` already starts with a `path: ` written by [`at_path`]. A path has
/// no whitespace outside bracketed segments, unlike the leading words of a message.
fn has_path(message: &str) -> bool {
    let mut depth = 0_usize;
    for (index, character) in message.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return index > 0 && message[index + 1..].starts_with(' '),
            _ if depth == 0 && character.is_whitespace() => return false,
            _ => {}
        }
    }
    false
}

/// Reads `value` as a JavaScript array.
pub(crate) fn expect_array<'js>(value: JsValue<'js>, rust: &'static str) -> JsResult<Array<'js>> {
    value
        .try_into_array()
        .map_err(|value| mismatch(&value, rust, "array"))
}

/// Reads `value` as a JavaScript array of exactly `len` items.
#[doc(hidden)]
pub fn expect_array_len<'js>(
    value: JsValue<'js>,
    rust: &'static str,
    len: usize,
) -> JsResult<Array<'js>> {
    let array = expect_array(value, rust)?;
    let actual = array_length(&array, rust)?;
    if actual == len {
        return Ok(array);
    }
    Err(codec_error(
        "array",
        rust,
        format!("expected array of length {len}, got length {actual}"),
    ))
}

/// Length of a JavaScript array. Lengths beyond `i32::MAX` are stored as floats, which
/// `Array::len` panics on; this reads them, and reports anything else as an error.
pub(crate) fn array_length(array: &Array<'_>, rust: &'static str) -> JsResult<usize> {
    let length = array
        .as_object()
        .get::<_, JsValue<'_>>(PredefinedAtom::Length)?;
    let valid = match length.as_int() {
        Some(int) => usize::try_from(int).ok(),
        None => length
            .as_float()
            .and_then(exact_integer)
            .and_then(|wide| usize::try_from(wide).ok()),
    };
    valid.ok_or_else(|| mismatch(&length, rust, "array length"))
}

/// Reads `value` as a JavaScript object that is neither an array nor a function.
#[doc(hidden)]
pub fn expect_object<'js>(value: JsValue<'js>, rust: &'static str) -> JsResult<Object<'js>> {
    if value.is_array() || value.is_function() {
        return Err(mismatch(&value, rust, "object"));
    }
    value
        .try_into_object()
        .map_err(|value| mismatch(&value, rust, "object"))
}

/// Decodes item `index` of `array`, prefixing errors with `[index]`.
pub(crate) fn decode_item<'js, T: JsDecode>(
    ctx: &Ctx<'js>,
    array: &Array<'js>,
    index: usize,
) -> JsResult<T> {
    let item = array.get::<JsValue<'js>>(index)?;
    T::decode_js(ctx, item).map_err(|error| at_index(error, index))
}

/// Encodes `item` found at position `index`, prefixing errors with `[index]`.
pub(crate) fn encode_item<'js, T: JsEncode + ?Sized>(
    ctx: &Ctx<'js>,
    item: &T,
    index: usize,
) -> JsResult<JsValue<'js>> {
    item.encode_js(ctx).map_err(|error| at_index(error, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> JsError {
        codec_error("int", "u8", "expected integer in u8 range, got 300")
    }

    fn message(error: &JsError) -> &str {
        match error {
            JsError::FromJs {
                message: Some(message),
                ..
            } => message,
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn nested_field_segments_join_with_dots() {
        let error = at_path(at_path(leaf(), "x"), "position");

        assert_eq!(
            message(&error),
            "position.x: expected integer in u8 range, got 300"
        );
    }

    #[test]
    fn index_and_key_segments_attach_without_dots() {
        let error = at_path(at_index(at_path(leaf(), "[a b]"), 2), "items");

        assert_eq!(
            message(&error),
            "items[2][a b]: expected integer in u8 range, got 300"
        );
    }

    #[test]
    fn field_after_index_joins_with_a_dot() {
        let error = at_index(at_path(leaf(), "name"), 0);

        assert_eq!(
            message(&error),
            "[0].name: expected integer in u8 range, got 300"
        );
    }

    #[test]
    fn leaf_messages_containing_colons_are_not_mistaken_for_paths() {
        let error = at_path(
            at_path(
                codec_error("string", "u8", "invalid type: string, expected u8"),
                "x",
            ),
            "position",
        );

        assert_eq!(
            message(&error),
            "position.x: invalid type: string, expected u8"
        );
    }

    #[test]
    fn errors_without_message_describe_the_conversion() {
        let error = at_path(JsError::new_from_js("object", "Uint8Array"), "bytes");

        assert_eq!(message(&error), "bytes: expected Uint8Array, got object");
    }
}
