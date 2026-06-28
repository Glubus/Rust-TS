use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

#[cfg(feature = "async-promise")]
use std::future::Future;
#[cfg(feature = "async-promise")]
use std::pin::Pin;

use serde_json::Value;

use super::bindings::FunctionBindingStore;
use super::declarations::render_typescript_declarations;
use super::interface::HostContractRegistry;
use super::sdk::render_typescript_sdk;
use crate::config::{VmContractValidation, VmUnknownFieldValidation};
#[cfg(feature = "tokio")]
use crate::contract::AsyncHostFunction;
use crate::contract::validation::{SchemaValidationOptions, validate_schema_with_options};
use crate::contract::{
    HostCallback, HostContext, HostContractAbi, HostContractDescriptor, HostFunction,
    HostFunctionDescriptor, HostFunctionExecution, TsSchema,
};
use crate::error::VmError;
use crate::sdk_files::{
    GeneratedSdkFiles, SdkFileNames, write_host_sdk_files, write_host_sdk_files_with_names,
};

/// In-memory V0 host contract registry.
#[derive(Default)]
pub struct InMemoryHostContractRegistry {
    by_name: Mutex<HashMap<String, HostContractDescriptor>>,
    function_bindings: FunctionBindingStore,
    validation: VmContractValidation,
    unknown_field_validation: VmUnknownFieldValidation,
}

impl InMemoryHostContractRegistry {
    /// Creates an empty host contract registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty host contract registry with a validation policy.
    pub fn with_validation(validation: VmContractValidation) -> Self {
        Self {
            validation,
            ..Self::default()
        }
    }

    /// Creates an empty host contract registry with explicit validation policies.
    pub fn with_validation_options(
        validation: VmContractValidation,
        unknown_field_validation: VmUnknownFieldValidation,
    ) -> Self {
        Self {
            validation,
            unknown_field_validation,
            ..Self::default()
        }
    }

    /// Registers one host function contract and returns the registry for chaining.
    pub fn function<T>(&self) -> Result<&Self, VmError>
    where
        T: HostFunction + Send + Sync + 'static,
    {
        self.register_function::<T>()?;
        Ok(self)
    }

    /// Registers one host function using `TsSchema` from its input/output types and returns the registry for chaining.
    pub fn typed_function<T>(&self) -> Result<&Self, VmError>
    where
        T: HostFunction + Send + Sync + 'static,
        T::Input: TsSchema,
        T::Output: TsSchema,
    {
        self.register_typed_function::<T>()?;
        Ok(self)
    }

    /// Registers one async host function contract and returns the registry for chaining.
    #[cfg(feature = "tokio")]
    pub fn async_function<T>(&self) -> Result<&Self, VmError>
    where
        T: AsyncHostFunction + Send + Sync + 'static,
    {
        self.register_async_function::<T>()?;
        Ok(self)
    }

    /// Registers one async host function as a JavaScript Promise bridge and returns the registry for chaining.
    #[cfg(feature = "async-promise")]
    pub fn async_promise_function<T>(&self) -> Result<&Self, VmError>
    where
        T: AsyncHostFunction + Send + Sync + 'static,
    {
        self.register_async_promise_function::<T>()?;
        Ok(self)
    }

    /// Registers one host callback contract and returns the registry for chaining.
    pub fn callback<T>(&self) -> Result<&Self, VmError>
    where
        T: HostCallback + Send + Sync + 'static,
    {
        self.register_callback::<T>()?;
        Ok(self)
    }

    /// Registers one host callback using `TsSchema` from its payload type and returns the registry for chaining.
    pub fn typed_callback<T>(&self) -> Result<&Self, VmError>
    where
        T: HostCallback + Send + Sync + 'static,
        T::Payload: TsSchema,
    {
        self.register_typed_callback::<T>()?;
        Ok(self)
    }

    /// Registers one host context contract and returns the registry for chaining.
    pub fn context<T>(&self) -> Result<&Self, VmError>
    where
        T: HostContext,
    {
        self.register_context::<T>()?;
        Ok(self)
    }

    /// Returns one descriptor by stable contract name.
    pub fn descriptor(&self, name: &str) -> Result<Option<HostContractDescriptor>, VmError> {
        self.get(name)
    }

    /// Returns every descriptor known by the registry.
    pub fn descriptors(&self) -> Result<Vec<HostContractDescriptor>, VmError> {
        self.list()
    }

    pub(crate) fn cache_abi_seed(&self) -> Result<String, VmError> {
        self.abi_seed()
    }

    /// Renders TypeScript declarations from registered host contracts.
    pub fn dts(&self) -> Result<String, VmError> {
        self.typescript_declarations()
    }

    /// Renders TypeScript declarations from registered host contracts.
    pub fn types(&self) -> Result<String, VmError> {
        self.typescript_declarations()
    }

    /// Renders an ergonomic TypeScript SDK from registered host contracts.
    pub fn sdk(&self) -> Result<String, VmError> {
        self.typescript_sdk()
    }

    /// Writes generated declaration and SDK source files into one directory.
    pub fn write_sdk_files(
        &self,
        directory: impl AsRef<Path>,
    ) -> Result<GeneratedSdkFiles, VmError> {
        write_host_sdk_files(directory, &self.descriptors()?)
    }

    /// Writes generated declaration and SDK source files with explicit file names.
    pub fn write_sdk_files_with_names(
        &self,
        directory: impl AsRef<Path>,
        names: &SdkFileNames,
    ) -> Result<GeneratedSdkFiles, VmError> {
        write_host_sdk_files_with_names(directory, &self.descriptors()?, names)
    }

    pub(crate) fn invoke_function(
        &self,
        name: &str,
        input: Value,
    ) -> Result<Option<Value>, VmError> {
        let Some(descriptor) = self.descriptor(name)? else {
            return Ok(None);
        };
        self.validate_function_input(&descriptor, &input)?;
        let Some(output) = self.function_bindings.invoke(name, input)? else {
            return Ok(None);
        };
        self.validate_function_output(&descriptor, &output)?;
        Ok(Some(output))
    }

    #[cfg(feature = "async-promise")]
    pub(crate) fn invoke_function_async(
        &self,
        name: String,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, VmError>> + Send + 'static>> {
        let descriptor = match self.descriptor(&name) {
            Ok(descriptor) => descriptor,
            Err(error) => return Box::pin(std::future::ready(Err(error))),
        };
        let Some(descriptor) = descriptor else {
            return Box::pin(std::future::ready(Ok(None)));
        };
        if let Err(error) = self.validate_function_input(&descriptor, &input) {
            return Box::pin(std::future::ready(Err(error)));
        }

        let validation = self.validation;
        let unknown_field_validation = self.unknown_field_validation;
        let future = self.function_bindings.invoke_async(name, input);
        Box::pin(async move {
            let output = future.await?;
            if let Some(output) = &output {
                validate_function_output_with_policy(
                    validation,
                    unknown_field_validation,
                    &descriptor,
                    output,
                )?;
            }
            Ok(output)
        })
    }

    fn insert_descriptor(&self, descriptor: HostContractDescriptor) -> Result<(), VmError> {
        let mut guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        guard.insert(descriptor.name.clone(), descriptor);
        Ok(())
    }

    fn validate_function_input(
        &self,
        descriptor: &HostContractDescriptor,
        input: &Value,
    ) -> Result<(), VmError> {
        if !self.validation.validates_inputs() {
            return Ok(());
        }
        let Some(function) = &descriptor.function else {
            return Ok(());
        };
        validate_schema_with_options(
            &function.input_schema,
            input,
            validation_options(self.unknown_field_validation),
        )
        .map_err(|details| VmError::ContractValidation {
            contract_name: descriptor.name.clone(),
            direction: "input",
            details,
        })
    }

    fn validate_function_output(
        &self,
        descriptor: &HostContractDescriptor,
        output: &Value,
    ) -> Result<(), VmError> {
        validate_function_output_with_policy(
            self.validation,
            self.unknown_field_validation,
            descriptor,
            output,
        )
    }
}

fn validate_function_output_with_policy(
    validation: VmContractValidation,
    unknown_field_validation: VmUnknownFieldValidation,
    descriptor: &HostContractDescriptor,
    output: &Value,
) -> Result<(), VmError> {
    if !validation.validates_outputs() {
        return Ok(());
    }
    let Some(function) = &descriptor.function else {
        return Ok(());
    };
    validate_schema_with_options(
        &function.output_schema,
        output,
        validation_options(unknown_field_validation),
    )
    .map_err(|details| VmError::ContractValidation {
        contract_name: descriptor.name.clone(),
        direction: "output",
        details,
    })
}

fn validation_options(
    unknown_field_validation: VmUnknownFieldValidation,
) -> SchemaValidationOptions {
    SchemaValidationOptions {
        reject_unknown_fields: unknown_field_validation.rejects_unknown_fields(),
    }
}

fn typed_function_descriptor<T>() -> HostFunctionDescriptor
where
    T: HostFunction,
    T::Input: TsSchema,
    T::Output: TsSchema,
{
    HostFunctionDescriptor {
        input_schema: T::Input::schema(),
        output_schema: T::Output::schema(),
        execution: HostFunctionExecution::Sync,
    }
}

impl HostContractRegistry for InMemoryHostContractRegistry {
    fn register_function<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + Send + Sync + 'static,
    {
        let mut descriptor = T::descriptor();
        let function = T::function_descriptor();
        descriptor.abi = HostContractAbi::Function {
            input: function.input_schema.clone(),
            output: function.output_schema.clone(),
            execution: function.execution,
        };
        descriptor.function = Some(function);
        self.insert_descriptor(descriptor)?;
        self.function_bindings.insert_static::<T>()
    }

    fn register_typed_function<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + Send + Sync + 'static,
        T::Input: TsSchema,
        T::Output: TsSchema,
    {
        let mut descriptor = T::descriptor();
        let function = typed_function_descriptor::<T>();
        descriptor.schema = function.input_schema.clone();
        descriptor.abi = HostContractAbi::Function {
            input: function.input_schema.clone(),
            output: function.output_schema.clone(),
            execution: function.execution,
        };
        descriptor.function = Some(function);
        self.insert_descriptor(descriptor)?;
        self.function_bindings.insert_static::<T>()
    }

    #[cfg(feature = "tokio")]
    fn register_async_function<T>(&self) -> Result<(), VmError>
    where
        T: AsyncHostFunction + Send + Sync + 'static,
    {
        let handle = tokio::runtime::Handle::try_current().map_err(|error| VmError::Execution {
            details: format!("registering async host function requires a tokio runtime: {error}"),
        })?;
        let mut descriptor = T::descriptor();
        let function = T::function_descriptor();
        descriptor.abi = HostContractAbi::Function {
            input: function.input_schema.clone(),
            output: function.output_schema.clone(),
            execution: function.execution,
        };
        descriptor.function = Some(function);
        self.insert_descriptor(descriptor)?;
        self.function_bindings.insert_async_static::<T>(handle)
    }

    #[cfg(feature = "async-promise")]
    fn register_async_promise_function<T>(&self) -> Result<(), VmError>
    where
        T: AsyncHostFunction + Send + Sync + 'static,
    {
        let handle = tokio::runtime::Handle::try_current().map_err(|error| VmError::Execution {
            details: format!(
                "registering async promise host function requires a tokio runtime: {error}"
            ),
        })?;
        let mut descriptor = T::descriptor();
        let mut function = T::function_descriptor();
        function.execution = HostFunctionExecution::AsyncPromise;
        descriptor.abi = HostContractAbi::Function {
            input: function.input_schema.clone(),
            output: function.output_schema.clone(),
            execution: function.execution,
        };
        descriptor.function = Some(function);
        self.insert_descriptor(descriptor)?;
        self.function_bindings.insert_async_static::<T>(handle)
    }

    fn register_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static,
    {
        let mut descriptor = T::descriptor();
        let callback = T::callback_descriptor();
        descriptor.abi = HostContractAbi::Callback {
            payload: callback.payload_schema.clone(),
            delivery: callback.delivery,
            hot: callback.hot,
        };
        descriptor.callback = Some(callback);
        self.insert_descriptor(descriptor)
    }

    fn register_typed_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static,
        T::Payload: TsSchema,
    {
        let mut descriptor = T::descriptor();
        let mut callback = T::callback_descriptor();
        callback.payload_schema = T::Payload::schema();
        descriptor.schema = callback.payload_schema.clone();
        descriptor.abi = HostContractAbi::Callback {
            payload: callback.payload_schema.clone(),
            delivery: callback.delivery,
            hot: callback.hot,
        };
        descriptor.callback = Some(callback);
        self.insert_descriptor(descriptor)
    }

    fn register_context<T>(&self) -> Result<(), VmError>
    where
        T: HostContext,
    {
        let mut descriptor = T::descriptor();
        descriptor.abi = HostContractAbi::Context {
            schema: descriptor.schema.clone(),
        };
        self.insert_descriptor(descriptor)
    }

    fn get(&self, name: &str) -> Result<Option<HostContractDescriptor>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        Ok(guard.get(name).cloned())
    }

    fn list(&self) -> Result<Vec<HostContractDescriptor>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        let mut descriptors = guard.values().cloned().collect::<Vec<_>>();
        descriptors.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(descriptors)
    }

    fn abi_seed(&self) -> Result<String, VmError> {
        serde_json::to_string(&self.list()?).map_err(VmError::from)
    }

    fn typescript_declarations(&self) -> Result<String, VmError> {
        Ok(render_typescript_declarations(&self.list()?))
    }

    fn typescript_sdk(&self) -> Result<String, VmError> {
        Ok(render_typescript_sdk(&self.list()?))
    }
}
