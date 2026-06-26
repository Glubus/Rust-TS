//! Derive macros for `ts_embed_vm`.

mod attrs;
mod expand;
mod rename;

use proc_macro::TokenStream;
use syn::{Error, parse_macro_input};

use expand::expand_ts_schema;

/// Derives `ts_embed_vm::TsSchema` for simple Rust data shapes.
///
/// Supported V0 shapes:
///
/// - named-field structs
/// - tuple structs
/// - unit enums
/// - payload enums
/// - generic structs/enums whose type parameters implement `TsSchema`
///
/// Supported field/container attributes:
///
/// - `#[tsvm(name = "TypeName")]`
/// - `#[tsvm(rename = "fieldName")]`
/// - `#[tsvm(optional)]`
/// - `#[serde(rename = "fieldName")]`
/// - `#[serde(rename_all = "camelCase")]`
/// - `#[serde(default)]`
/// - `#[serde(skip)]`
/// - `#[serde(transparent)]` on single-field structs
/// - `#[serde(untagged)]` on enums
///
/// `Option<T>` fields are emitted as optional nullable TypeScript fields.
#[proc_macro_derive(TsSchema, attributes(tsvm, serde))]
pub fn derive_ts_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    expand_ts_schema(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
