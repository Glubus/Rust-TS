//! Host contract traits and metadata.

mod arity;
#[cfg(feature = "tokio")]
mod async_function;
mod base;
mod bridge;
mod callback;
mod codec;
mod context;
mod function;
mod metadata;
mod native_bytes;
mod schema;
pub(crate) mod validation;

#[cfg(feature = "tokio")]
pub use async_function::AsyncHostFunction;
pub use base::HostContract;
pub(crate) use bridge::{js_value_to_json, json_to_js_value};
pub use callback::HostCallback;
pub(crate) use codec::array_length;
pub use codec::{
    JsArgs, JsDecode, JsEncode, at_path, codec_error, derive, expect_array_len, expect_object,
};
pub use context::HostContext;
pub use function::HostFunction;
pub use metadata::{
    DeliveryMode, HostCallbackDescriptor, HostContractAbi, HostContractDescriptor,
    HostContractKind, HostFunctionDescriptor, HostFunctionExecution, HostImportBinding,
    HostMetadata, Schema, TsEnumVariant, TsField, TsLiteral, TsRecordKey, TsType,
};
pub use native_bytes::NativeBytes;
pub use schema::{ObjectSchema, TsSchema, push_schema_dependency, schema_type_ref};
