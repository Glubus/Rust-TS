use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rquickjs::function::Opt;
use rquickjs::prelude::Func;
use rquickjs::{Ctx, Exception, Object, Result as JsResult, Value as JsValue};
use serde_json::Value;

use super::bindings::{FunctionBindingStore, JsonCodec, TypedCodec, input_or_null};
use super::declarations::render_typescript_declarations;
use super::import_modules::render_host_import_modules;
use super::interface::HostContractRegistry;
use super::sdk::render_typescript_sdk;
use crate::config::{VmContractValidation, VmUnknownFieldValidation};
use crate::contract::validation::{SchemaValidationOptions, validate_schema_with_options};
use crate::contract::{
    Caller, HostCallback, HostCallbackDescriptor, HostContext, HostContractAbi,
    HostContractDescriptor, HostFunction, HostFunctionDescriptor, HostFunctionSignature,
    HostRequest, JsDecode, JsEncode, Schema, TsSchema, js_value_to_json, json_to_js_value,
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

    /// Registers one host function contract, implemented by its static
    /// [`HostFunction::call`], and returns the registry for chaining.
    pub fn function<T>(&self) -> Result<&Self, VmError>
    where
        T: HostFunction + 'static,
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
        T: HostFunction + 'static,
        T::Input: TsSchema + JsDecode,
        T::Output: TsSchema + JsEncode,
    {
        self.register_typed_function::<T>()?;
        Ok(self)
    }

    /// Registers contract `C` implemented by `handler`, a closure that can hold state,
    /// and returns the registry for chaining.
    ///
    /// Like [`Self::function`], values cross as JSON and the schemas are the ones `C`
    /// declares.
    pub fn function_with<C>(
        &self,
        handler: impl Fn(C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<&Self, VmError>
    where
        C: HostFunctionSignature + 'static,
    {
        self.register_function_with::<C>(handler)?;
        Ok(self)
    }

    /// Registers contract `C` implemented by `handler`, a closure that can hold state,
    /// and returns the registry for chaining.
    ///
    /// Like [`Self::typed_function`], the schemas come from `TsSchema` of the input and
    /// output types, and values cross natively.
    pub fn typed_function_with<C>(
        &self,
        handler: impl Fn(C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<&Self, VmError>
    where
        C: HostFunctionSignature + 'static,
        C::Input: TsSchema + JsDecode,
        C::Output: TsSchema + JsEncode,
    {
        self.register_typed_function_with::<C>(handler)?;
        Ok(self)
    }

    /// Registers contract `C` implemented by `handler`, which also receives the
    /// [`Caller`], and returns the registry for chaining.
    ///
    /// Like [`Self::function_with`] otherwise.
    pub fn function_with_caller<C>(
        &self,
        handler: impl Fn(&Caller<'_>, C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<&Self, VmError>
    where
        C: HostFunctionSignature + 'static,
    {
        self.register_function_with_caller::<C>(handler)?;
        Ok(self)
    }

    /// Registers contract `C` implemented by `handler`, which also receives the
    /// [`Caller`], and returns the registry for chaining.
    ///
    /// Like [`Self::typed_function_with`] otherwise.
    pub fn typed_function_with_caller<C>(
        &self,
        handler: impl Fn(&Caller<'_>, C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<&Self, VmError>
    where
        C: HostFunctionSignature + 'static,
        C::Input: TsSchema + JsDecode,
        C::Output: TsSchema + JsEncode,
    {
        self.register_typed_function_with_caller::<C>(handler)?;
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

    /// Registers one host request, a callback whose handlers reply, using `TsSchema` from
    /// its payload and reply types, and returns the registry for chaining.
    pub fn typed_request<T>(&self) -> Result<&Self, VmError>
    where
        T: HostRequest + Send + Sync + 'static,
        T::Payload: TsSchema + JsEncode,
        T::Reply: TsSchema + JsDecode,
    {
        self.register_typed_request::<T>()?;
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

    /// Installs every sync host function of script `script_id` on `target` as a native
    /// QuickJS function keyed by contract name.
    pub(crate) fn install_native_functions<'js>(
        self: &Arc<Self>,
        target: &Object<'js>,
        script_id: &str,
    ) -> JsResult<()> {
        if self.validates_any() {
            return self.install_validated_functions(target, script_id);
        }
        self.function_bindings.install_native(target, script_id)
    }

    fn validates_any(&self) -> bool {
        self.validation.validates_inputs() || self.validation.validates_outputs()
    }

    /// Validation needs the descriptor, so each function goes through the registry.
    fn install_validated_functions<'js>(
        self: &Arc<Self>,
        target: &Object<'js>,
        script_id: &str,
    ) -> JsResult<()> {
        let script_id = Rc::<str>::from(script_id);
        for name in self.function_bindings.names().map_err(js_host_error)? {
            let registry = self.clone();
            let script_id = Rc::clone(&script_id);
            let contract_name = name.clone();
            target.set(
                name,
                Func::from(move |ctx: Ctx<'js>, input: Opt<JsValue<'js>>| {
                    let caller = Caller::new(&script_id);
                    registry.invoke_validated_native(&ctx, &contract_name, &caller, input)
                }),
            )?;
        }
        Ok(())
    }

    fn invoke_validated_native<'js>(
        &self,
        ctx: &Ctx<'js>,
        contract_name: &str,
        caller: &Caller<'_>,
        input: Opt<JsValue<'js>>,
    ) -> JsResult<JsValue<'js>> {
        let input = js_value_to_json(ctx, input_or_null(ctx, input))?;
        let output = self
            .invoke_function(contract_name, caller, input)
            .map_err(js_host_error)?
            .ok_or_else(|| {
                Exception::throw_message(ctx, &format!("missing host function: {contract_name}"))
            })?;
        json_to_js_value(ctx, &output)
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

    /// Calls the function `name` with a JSON input, validating the input and output as
    /// the registry's policy asks.
    pub(crate) fn invoke_function(
        &self,
        name: &str,
        caller: &Caller<'_>,
        input: Value,
    ) -> Result<Option<Value>, VmError> {
        let Some(descriptor) = self.descriptor(name)? else {
            return Ok(None);
        };
        self.validate_function_input(&descriptor, &input)?;
        let Some(output) = self.function_bindings.invoke(name, caller, input)? else {
            return Ok(None);
        };
        self.validate_function_output(&descriptor, &output)?;
        Ok(Some(output))
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

/// Descriptor of function contract `C` with the schemas its [`HostFunctionSignature`]
/// declares.
fn declared_function_descriptor<C: HostFunctionSignature>() -> HostContractDescriptor {
    with_function(C::descriptor(), C::function_descriptor())
}

/// Descriptor of function contract `C` with the schemas `TsSchema` gives its input and
/// output types.
fn typed_function_descriptor<C>() -> HostContractDescriptor
where
    C: HostFunctionSignature,
    C::Input: TsSchema,
    C::Output: TsSchema,
{
    let function = HostFunctionDescriptor {
        input_schema: C::Input::schema(),
        output_schema: C::Output::schema(),
    };
    let mut descriptor = C::descriptor();
    descriptor.schema = function.input_schema.clone();
    with_function(descriptor, function)
}

/// Adds the function ABI and metadata of `function` to `descriptor`.
fn with_function(
    mut descriptor: HostContractDescriptor,
    function: HostFunctionDescriptor,
) -> HostContractDescriptor {
    descriptor.abi = HostContractAbi::Function {
        input: function.input_schema.clone(),
        output: function.output_schema.clone(),
    };
    descriptor.function = Some(function);
    descriptor
}

/// Descriptor of callback contract `T` with the payload schema `TsSchema` gives its
/// payload type and, for a request, the schema of its handlers' `reply`.
fn typed_callback_descriptor<T>(reply: Option<Schema>) -> HostContractDescriptor
where
    T: HostCallback,
    T::Payload: TsSchema,
{
    let mut callback = T::callback_descriptor();
    callback.payload_schema = T::Payload::schema();
    callback.reply_schema = reply;
    let mut descriptor = T::descriptor();
    descriptor.schema = callback.payload_schema.clone();
    with_callback(descriptor, callback)
}

/// Adds the callback ABI and metadata of `callback` to `descriptor`.
fn with_callback(
    mut descriptor: HostContractDescriptor,
    callback: HostCallbackDescriptor,
) -> HostContractDescriptor {
    descriptor.abi = HostContractAbi::Callback {
        payload: callback.payload_schema.clone(),
        reply: callback.reply_schema.clone(),
    };
    descriptor.callback = Some(callback);
    descriptor
}

impl HostContractRegistry for InMemoryHostContractRegistry {
    fn register_function<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + 'static,
    {
        self.register_function_with::<T>(T::call)
    }

    fn register_typed_function<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + 'static,
        T::Input: TsSchema + JsDecode,
        T::Output: TsSchema + JsEncode,
    {
        self.register_typed_function_with::<T>(T::call)
    }

    fn register_function_with<C>(
        &self,
        handler: impl Fn(C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<(), VmError>
    where
        C: HostFunctionSignature + 'static,
    {
        self.insert_descriptor(declared_function_descriptor::<C>())?;
        self.function_bindings.insert_plain::<C, JsonCodec>(handler)
    }

    fn register_typed_function_with<C>(
        &self,
        handler: impl Fn(C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<(), VmError>
    where
        C: HostFunctionSignature + 'static,
        C::Input: TsSchema + JsDecode,
        C::Output: TsSchema + JsEncode,
    {
        self.insert_descriptor(typed_function_descriptor::<C>())?;
        self.function_bindings.insert_plain::<C, TypedCodec>(handler)
    }

    fn register_function_with_caller<C>(
        &self,
        handler: impl Fn(&Caller<'_>, C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<(), VmError>
    where
        C: HostFunctionSignature + 'static,
    {
        self.insert_descriptor(declared_function_descriptor::<C>())?;
        self.function_bindings
            .insert_with_caller::<C, JsonCodec>(handler)
    }

    fn register_typed_function_with_caller<C>(
        &self,
        handler: impl Fn(&Caller<'_>, C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<(), VmError>
    where
        C: HostFunctionSignature + 'static,
        C::Input: TsSchema + JsDecode,
        C::Output: TsSchema + JsEncode,
    {
        self.insert_descriptor(typed_function_descriptor::<C>())?;
        self.function_bindings
            .insert_with_caller::<C, TypedCodec>(handler)
    }

    fn register_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static,
    {
        self.insert_descriptor(with_callback(T::descriptor(), T::callback_descriptor()))
    }

    fn register_typed_callback<T>(&self) -> Result<(), VmError>
    where
        T: HostCallback + Send + Sync + 'static,
        T::Payload: TsSchema + JsEncode,
    {
        self.insert_descriptor(typed_callback_descriptor::<T>(None))
    }

    fn register_typed_request<T>(&self) -> Result<(), VmError>
    where
        T: HostRequest + Send + Sync + 'static,
        T::Payload: TsSchema + JsEncode,
        T::Reply: TsSchema + JsDecode,
    {
        self.insert_descriptor(typed_callback_descriptor::<T>(Some(T::Reply::schema())))
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
