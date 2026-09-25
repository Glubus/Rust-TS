use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Mutex;

use rquickjs::{Ctx, Result as JsResult, Value as JsValue};
use serde_json::Value;

use super::bindings::FunctionBindingStore;
use super::declarations::render_typescript_declarations;
use super::import_modules::render_host_import_modules;
use super::interface::HostContractRegistry;
use super::sdk::render_typescript_sdk;
use crate::config::{VmContractValidation, VmUnknownFieldValidation};
use crate::contract::validation::{SchemaValidationOptions, validate_schema_with_options};
use crate::contract::{
    HostCallback, HostContext, HostContractAbi, HostContractDescriptor, HostFunction,
    HostFunctionDescriptor, JsDecode, JsEncode, TsSchema, js_value_to_json, json_to_js_value,
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
    ///
    /// Calls from scripts convert the input and output natively through [`JsDecode`] and
    /// [`JsEncode`] when contract validation is off.
    pub fn typed_function<T>(&self) -> Result<&Self, VmError>
    where
        T: HostFunction + Send + Sync + 'static,
        T::Input: TsSchema + JsDecode,
        T::Output: TsSchema + JsEncode,
    {
        self.register_typed_function::<T>()?;
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
        T::Payload: TsSchema + JsEncode,
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

    pub(crate) fn import_module_names(&self) -> Result<BTreeSet<String>, VmError> {
        Ok(self
            .descriptors()?
            .into_iter()
            .map(|descriptor| descriptor.import.module)
            .collect())
    }

    /// Source of every host import module, keyed by module name.
    pub(crate) fn import_modules(&self) -> Result<BTreeMap<String, String>, VmError> {
        Ok(render_host_import_modules(&self.descriptors()?))
    }

    /// Installs every sync host function on `target` as a native QuickJS function keyed
    /// by contract name.
    pub(crate) fn install_native_functions<'js>(
        self: &std::sync::Arc<Self>,
        target: &rquickjs::Object<'js>,
    ) -> JsResult<()> {
        if self.validates_any() {
            return self.install_validated_functions(target);
        }
        self.function_bindings.install_native(target)
    }

    fn validates_any(&self) -> bool {
        self.validation.validates_inputs() || self.validation.validates_outputs()
    }

    /// Validation needs the descriptor, so each function goes through the registry.
    fn install_validated_functions<'js>(
        self: &std::sync::Arc<Self>,
        target: &rquickjs::Object<'js>,
    ) -> JsResult<()> {
        for name in self.function_bindings.names().map_err(js_host_error)? {
            let registry = self.clone();
            let contract_name = name.clone();
            target.set(
                name,
                rquickjs::prelude::Func::from(
                    move |ctx: Ctx<'js>, input: rquickjs::function::Opt<JsValue<'js>>| {
                        registry.invoke_validated_native(&ctx, &contract_name, input)
                    },
                ),
            )?;
        }
        Ok(())
    }

    fn invoke_validated_native<'js>(
        &self,
        ctx: &Ctx<'js>,
        contract_name: &str,
        input: rquickjs::function::Opt<JsValue<'js>>,
    ) -> JsResult<JsValue<'js>> {
        let input = super::bindings::input_or_null(ctx, input);
        self.invoke_function_js(ctx, contract_name, input)?
            .ok_or_else(|| {
                rquickjs::Exception::throw_message(
                    ctx,
                    &format!("missing host function: {contract_name}"),
                )
            })
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

    pub(crate) fn invoke_function_js<'js>(
        &self,
        ctx: &Ctx<'js>,
        name: &str,
        input: JsValue<'js>,
    ) -> JsResult<Option<JsValue<'js>>> {
        if self.validation.validates_inputs() || self.validation.validates_outputs() {
            let input = js_value_to_json(ctx, input)?;
            return self
                .invoke_function(name, input)
                .map_err(js_host_error)?
                .map(|output| json_to_js_value(ctx, &output))
                .transpose();
        }

        self.function_bindings.invoke_js(ctx, name, input)
    }

    fn insert_descriptor(&self, descriptor: HostContractDescriptor) -> Result<(), VmError> {
        let mut guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
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
        if !self.validation.validates_outputs() {
            return Ok(());
        }
        let Some(function) = &descriptor.function else {
            return Ok(());
        };
        validate_schema_with_options(
            &function.output_schema,
            output,
            validation_options(self.unknown_field_validation),
        )
        .map_err(|details| VmError::ContractValidation {
            contract_name: descriptor.name.clone(),
            direction: "output",
            details,
        })
    }
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
        };
        descriptor.function = Some(function);
        self.insert_descriptor(descriptor)?;
        self.function_bindings.insert_static::<T>()
    }

    fn register_typed_function<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + Send + Sync + 'static,
        T::Input: TsSchema + JsDecode,
        T::Output: TsSchema + JsEncode,
    {
        let mut descriptor = T::descriptor();
        let function = typed_function_descriptor::<T>();
        descriptor.schema = function.input_schema.clone();
        descriptor.abi = HostContractAbi::Function {
            input: function.input_schema.clone(),
            output: function.output_schema.clone(),
        };
        descriptor.function = Some(function);
        self.insert_descriptor(descriptor)?;
        self.function_bindings.insert_typed_static::<T>()
    }

    fn register_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static,
    {
        let mut descriptor = T::descriptor();
        let callback = T::callback_descriptor();
        descriptor.abi = HostContractAbi::Callback {
            payload: callback.payload_schema.clone(),
        };
        descriptor.callback = Some(callback);
        self.insert_descriptor(descriptor)
    }

    fn register_typed_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static,
        T::Payload: TsSchema + JsEncode,
    {
        let mut descriptor = T::descriptor();
        let mut callback = T::callback_descriptor();
        callback.payload_schema = T::Payload::schema();
        descriptor.schema = callback.payload_schema.clone();
        descriptor.abi = HostContractAbi::Callback {
            payload: callback.payload_schema.clone(),
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
        let guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
        Ok(guard.get(name).cloned())
    }

    fn list(&self) -> Result<Vec<HostContractDescriptor>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
        let mut descriptors = guard.values().cloned().collect::<Vec<_>>();
        descriptors.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(descriptors)
    }

    fn typescript_declarations(&self) -> Result<String, VmError> {
        Ok(render_typescript_declarations(&self.list()?))
    }

    fn typescript_sdk(&self) -> Result<String, VmError> {
        Ok(render_typescript_sdk(&self.list()?))
    }
}

fn js_host_error(error: impl ToString) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("host", "function", error.to_string())
}
