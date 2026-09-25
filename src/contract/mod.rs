//! Host contract traits and metadata.

mod arity;
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
    HostCallbackDescriptor, HostContractAbi, HostContractDescriptor, HostContractKind,
    HostFunctionDescriptor, HostImportBinding, HostMetadata, Schema, TsEnumVariant, TsField,
    TsLiteral, TsRecordKey, TsType,
};
pub use native_bytes::NativeBytes;
pub use schema::{ObjectSchema, TsSchema, push_schema_dependency, schema_type_ref};
