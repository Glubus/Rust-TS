use crate::contract::{
    HostCallback, HostContext, HostContractDescriptor, HostFunction, JsDecode, JsEncode, TsSchema,
};
use crate::error::VmError;

/// Access to the host contract registry.
pub trait HostContractRegistry: Send + Sync {
    /// Registers one host function contract.
    fn register_function<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + Send + Sync + 'static;

    /// Registers one host function contract using `TsSchema` from its input and output types.
    ///
    /// Script calls convert the input with [`JsDecode`] and the output with [`JsEncode`],
    /// natively and without JSON text.
    fn register_typed_function<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + Send + Sync + 'static,
        T::Input: TsSchema + JsDecode,
        T::Output: TsSchema + JsEncode;

    /// Registers one host callback contract.
    fn register_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static;

    /// Registers one host callback contract using `TsSchema` from its payload type.
    fn register_typed_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static,
        T::Payload: TsSchema + JsEncode;

    /// Registers one host context contract.
    fn register_context<T>(&self) -> Result<(), VmError>
    where
        T: HostContext;

    /// Returns one descriptor by stable contract name.
    fn get(&self, name: &str) -> Result<Option<HostContractDescriptor>, VmError>;

    /// Returns every stored descriptor.
    fn list(&self) -> Result<Vec<HostContractDescriptor>, VmError>;

    /// Returns a stable ABI fingerprint seed for cache invalidation.
    fn abi_seed(&self) -> Result<String, VmError>;

    /// Renders TypeScript declarations from registered ABI and schema metadata.
    fn typescript_declarations(&self) -> Result<String, VmError>;

    /// Renders an ergonomic TypeScript SDK from registered ABI and schema metadata.
    fn typescript_sdk(&self) -> Result<String, VmError>;
}
