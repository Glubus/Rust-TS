//! Embedded TypeScript for Rust hosts: `oxc` transpiles, QuickJS (`rquickjs`) runs the
//! scripts on the host's own thread, and typed host contracts generate the scripts'
//! TypeScript declarations.

#![deny(clippy::correctness)]
#![warn(clippy::suspicious)]
#![warn(clippy::style)]
#![warn(clippy::complexity)]
#![warn(clippy::perf)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(missing_docs)]

mod cache;
mod compiler;
mod config;
mod contract;
mod error;
pub mod js;
mod registry;
mod runner;
mod sdk_files;
mod types;

#[doc(hidden)]
pub use serde as __serde;

pub use config::{VmContractValidation, VmOptions, VmUnknownFieldValidation};
pub use contract::{
    HostCallback, HostCallbackDescriptor, HostContext, HostContract, HostContractAbi,
    HostContractDescriptor, HostContractKind, HostFunction, HostFunctionDescriptor, HostMetadata,
    NativeBytes, ObjectSchema, Schema, TsEnumVariant, TsField, TsLiteral, TsRecordKey, TsSchema,
    TsType, push_schema_dependency, schema_type_ref,
};
pub use contract::{JsArgs, JsDecode, JsEncode};
#[doc(hidden)]
pub use contract::{
    at_path as __codec_at_path, codec_error as __codec_error, derive as __derive,
    expect_array_len as __codec_expect_array_len, expect_object as __codec_expect_object,
};
pub use error::VmError;
pub use registry::{
    HostContractRegistry, InMemoryHostContractRegistry,
    render_typescript_declarations_for_descriptors, render_typescript_sdk_for_descriptors,
};
pub use runner::Engine;
#[cfg(feature = "derive")]
pub use rustts_macros::TsSchema;
pub use sdk_files::{
    GeneratedSdkFiles, SdkFileNames, write_host_sdk_files, write_host_sdk_files_with_names,
};
pub use types::{MemoryStats, ScriptId};
