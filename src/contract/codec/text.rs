//! Values that cross as JavaScript strings: text, `char`, paths and IP addresses.

use std::fmt::Display;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use rquickjs::{Ctx, Error as JsError, Result as JsResult, String as JsString, Value as JsValue};

use super::stack_text::StackText;
use super::{JsDecode, JsEncode, codec_error, encode_error, mismatch};

/// Room for the longest address text, `ffff:ffff:ffff:ffff:ffff:ffff:255.255.255.255`.
const ADDRESS_TEXT_CAPACITY: usize = 64;

impl JsEncode for str {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        JsString::from_str(ctx.clone(), self).map(JsString::into_value)
    }
}

impl JsEncode for String {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        self.as_str().encode_js(ctx)
    }
}

impl JsDecode for String {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        decode_string(value, "String")
    }
}

impl JsEncode for char {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        self.encode_utf8(&mut [0; 4]).encode_js(ctx)
    }
}

impl JsDecode for char {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        let text = decode_string(value, "char")?;
        let mut characters = text.chars();
        match (characters.next(), characters.next()) {
            (Some(character), None) => Ok(character),
            _ => Err(codec_error(
                "string",
                "char",
                format!(
                    "expected a string of exactly one character, got {} characters",
                    text.chars().count()
                ),
            )),
        }
    }
}

impl JsEncode for Path {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        self.to_str()
            .ok_or_else(|| {
                encode_error(
                    "Path",
                    "string",
                    "path contains invalid UTF-8 characters".to_owned(),
                )
            })?
            .encode_js(ctx)
    }
}

impl JsEncode for PathBuf {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        self.as_path().encode_js(ctx)
    }
}

impl JsDecode for PathBuf {
    fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
        decode_string(value, "PathBuf").map(PathBuf::from)
    }
}

macro_rules! address_codecs {
    ($($ty:ty => $expected:literal),+ $(,)?) => {
        $(
            impl JsEncode for $ty {
                fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
                    encode_display(ctx, self, stringify!($ty))
                }
            }

            impl JsDecode for $ty {
                fn decode_js<'js>(_ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self> {
                    let text = decode_string(value, stringify!($ty))?;
                    text.parse().map_err(|_| {
                        codec_error(
                            "string",
                            stringify!($ty),
                            format!(concat!("expected ", $expected, ", got {:?}"), text),
                        )
                    })
                }
            }
        )+
    };
}

address_codecs!(
    IpAddr => "an IP address",
    Ipv4Addr => "an IPv4 address",
    Ipv6Addr => "an IPv6 address",
);

/// Encodes the `Display` text of a short value without allocating.
fn encode_display<'js>(
    ctx: &Ctx<'js>,
    value: &impl Display,
    rust: &'static str,
) -> JsResult<JsValue<'js>> {
    let text =
        StackText::<ADDRESS_TEXT_CAPACITY>::format(format_args!("{value}")).ok_or_else(|| {
            encode_error(
                rust,
                "string",
                format!("text is longer than {ADDRESS_TEXT_CAPACITY} bytes"),
            )
        })?;
    text.as_str().encode_js(ctx)
}

/// Reads a JavaScript string as Rust text; lone UTF-16 surrogates are rejected, as
/// `serde_json` rejects them.
fn decode_string(value: JsValue<'_>, rust: &'static str) -> JsResult<String> {
    value
        .try_into_string()
        .map_err(|value| mismatch(&value, rust, "string"))?
        .to_string()
        .map_err(|error| match error {
            JsError::Utf8(_) => codec_error(
                "string",
                rust,
                "expected Unicode text, got a string with lone surrogates",
            ),
            other => other,
        })
}
