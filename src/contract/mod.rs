//! Host contract traits and metadata.

#[cfg(feature = "tokio")]
mod async_function;
mod base;
mod bridge;
mod callback;
mod context;
mod function;
mod metadata;
mod schema;
pub(crate) mod validation;

#[cfg(feature = "tokio")]
pub use async_function::AsyncHostFunction;
pub use base::HostContract;
pub use bridge::NativeBytes;
pub(crate) use bridge::{js_value_to_json, json_to_js_value};
pub use callback::HostCallback;
pub use context::HostContext;
pub use function::HostFunction;
pub use metadata::{
    DeliveryMode, HostCallbackDescriptor, HostContractAbi, HostContractDescriptor,
    HostContractKind, HostFunctionDescriptor, HostFunctionExecution, HostImportBinding,
    HostMetadata, Schema, TsEnumVariant, TsField, TsLiteral, TsRecordKey, TsType,
};
pub use schema::{TsSchema, push_schema_dependency, schema_type_ref};
