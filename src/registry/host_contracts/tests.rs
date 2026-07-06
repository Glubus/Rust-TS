use serde_json::Value;

use super::{HostContractRegistry, InMemoryHostContractRegistry};
use crate::contract::{
    HostCallback, HostCallbackDescriptor, HostContext, HostContract, HostContractAbi,
    HostContractDescriptor, HostContractKind, HostFunction, HostFunctionDescriptor,
    HostFunctionExecution, HostImportBinding, HostMetadata, Schema, TsEnumVariant, TsField,
    TsLiteral, TsRecordKey, TsType,
};
use crate::error::VmError;

struct DemoFunction;
struct GeneratedFunction;
struct DemoCallback;
struct DemoContext;

impl HostContract for DemoFunction {
    const NAME: &'static str = "demo.function";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["demo", "function"];

    fn schema() -> Schema {
        Schema::typed("DemoFunctionInput", TsType::Void)
    }

    fn metadata() -> HostMetadata {
        HostMetadata {
            name: String::from(Self::NAME),
            tags: vec![String::from("function")],
        }
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for DemoFunction {
    type Input = ();
    type Output = ();

    fn call(_input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(())
    }
}

impl HostContract for GeneratedFunction {
    const NAME: &'static str = "user.find";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["user", "find"];

    fn schema() -> Schema {
        Schema::typed("FindUserInput", TsType::Number)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for GeneratedFunction {
    type Input = ();
    type Output = ();

    fn output_schema() -> Schema {
        Schema::typed("FindUserOutput", TsType::String)
    }

    fn call(_input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(())
    }
}

impl HostContract for DemoCallback {
    const NAME: &'static str = "demo.callback";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["demo", "callback"];

    fn schema() -> Schema {
        Schema::typed(
            "DemoCallbackPayload",
            TsType::Object(vec![TsField::required("combo", TsType::Number)]),
        )
    }

    fn kind() -> HostContractKind {
        HostContractKind::Callback
    }
}

impl HostCallback for DemoCallback {
    type Payload = ();
}

impl HostContract for DemoContext {
    const NAME: &'static str = "demo.context";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["demo", "context"];

    fn schema() -> Schema {
        Schema::typed(
            "DemoContext",
            TsType::Object(vec![TsField::required("visible", TsType::Boolean)]),
        )
    }

    fn kind() -> HostContractKind {
        HostContractKind::Context
    }
}

impl HostContext for DemoContext {}

#[test]
fn register_function_stores_descriptor() {
    let registry = InMemoryHostContractRegistry::new();

    registry.register_function::<DemoFunction>().unwrap();

    let descriptor = registry.get(DemoFunction::NAME).unwrap().unwrap();
    assert_eq!(descriptor.name, DemoFunction::NAME);
    assert_eq!(descriptor.kind, HostContractKind::Function);
    assert_eq!(descriptor.schema.name, "DemoFunctionInput");
    let function = descriptor.function.expect("function descriptor");
    assert_eq!(function.input_schema.name, "DemoFunctionInput");
    assert_eq!(function.output_schema.name, "unknown");
    assert_eq!(function.execution, HostFunctionExecution::Sync);
    assert!(matches!(
        descriptor.abi,
        HostContractAbi::Function {
            input,
            output,
            execution,
        } if input.name == "DemoFunctionInput"
            && output.name == "unknown"
            && execution == HostFunctionExecution::Sync
    ));
}

#[test]
fn register_callback_stores_descriptor() {
    let registry = InMemoryHostContractRegistry::new();

    registry.register_callback::<DemoCallback>().unwrap();

    let descriptor = registry.get(DemoCallback::NAME).unwrap().unwrap();
    assert_eq!(descriptor.name, DemoCallback::NAME);
    assert_eq!(descriptor.kind, HostContractKind::Callback);
    assert_eq!(descriptor.schema.name, "DemoCallbackPayload");
    let callback = descriptor.callback.unwrap();
    assert_eq!(callback.payload_schema.name, "DemoCallbackPayload");
    assert_eq!(callback.delivery, crate::contract::DeliveryMode::Broadcast);
    assert!(callback.hot);
    assert!(matches!(
        descriptor.abi,
        HostContractAbi::Callback { payload, hot, .. }
            if payload.name == "DemoCallbackPayload" && hot
    ));
}

#[test]
fn register_context_stores_descriptor() {
    let registry = InMemoryHostContractRegistry::new();

    registry.register_context::<DemoContext>().unwrap();

    let descriptor = registry.get(DemoContext::NAME).unwrap().unwrap();
    assert_eq!(descriptor.name, DemoContext::NAME);
    assert_eq!(descriptor.kind, HostContractKind::Context);
    assert!(matches!(
        descriptor.abi,
        HostContractAbi::Context { schema } if schema.name == "DemoContext"
    ));
}

#[test]
fn fluent_registration_chains_contracts() {
    let registry = InMemoryHostContractRegistry::new();

    registry
        .callback::<DemoCallback>()
        .and_then(|registry| registry.function::<DemoFunction>())
        .and_then(|registry| registry.context::<DemoContext>())
        .unwrap();

    assert!(registry.descriptor(DemoCallback::NAME).unwrap().is_some());
    assert!(registry.descriptor(DemoFunction::NAME).unwrap().is_some());
    assert!(registry.descriptor(DemoContext::NAME).unwrap().is_some());
}

#[test]
fn invoke_function_executes_registered_binding() {
    let registry = InMemoryHostContractRegistry::new();
    registry.register_function::<DemoFunction>().unwrap();

    let result = registry
        .invoke_function(DemoFunction::NAME, Value::Null)
        .unwrap();

    assert_eq!(result, Some(Value::Null));
}

#[test]
fn list_returns_descriptors_sorted_by_name() {
    let registry = InMemoryHostContractRegistry::new();
    registry.register_context::<DemoContext>().unwrap();
    registry.register_function::<DemoFunction>().unwrap();
    registry.register_callback::<DemoCallback>().unwrap();

    let names = registry
        .list()
        .unwrap()
        .into_iter()
        .map(|descriptor| descriptor.name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            String::from(DemoCallback::NAME),
            String::from(DemoContext::NAME),
            String::from(DemoFunction::NAME),
        ]
    );
}

struct ComplexGeneratedFunction;

impl HostContract for ComplexGeneratedFunction {
    const NAME: &'static str = "billing.invoice.create";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["billing", "invoice", "create"];

    fn schema() -> Schema {
        Schema::typed(
            "CreateInvoiceInput",
            TsType::Object(vec![
                TsField::required(
                    "status",
                    TsType::Union(vec![
                        TsType::Literal(TsLiteral::String(String::from("draft"))),
                        TsType::Literal(TsLiteral::String(String::from("paid"))),
                    ]),
                ),
                TsField::optional(
                    "metadata",
                    TsType::Record {
                        key: TsRecordKey::String,
                        value: Box::new(TsType::Json),
                    },
                ),
                TsField::required(
                    "lines",
                    TsType::Array(Box::new(TsType::Tuple(vec![
                        TsType::String,
                        TsType::Number,
                    ]))),
                ),
                TsField::required(
                    "paymentMethod",
                    TsType::Enum {
                        tag: None,
                        variants: vec![TsEnumVariant::unit("card"), TsEnumVariant::unit("wire")],
                    },
                ),
            ]),
        )
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for ComplexGeneratedFunction {
    type Input = ();
    type Output = ();

    fn output_schema() -> Schema {
        Schema::typed(
            "CreateInvoiceOutput",
            TsType::Object(vec![
                TsField::required("id", TsType::Nullable(Box::new(TsType::String))),
                TsField::required(
                    "state",
                    TsType::Enum {
                        tag: Some(String::from("kind")),
                        variants: vec![
                            TsEnumVariant::unit("created"),
                            TsEnumVariant::payload(
                                "failed",
                                vec![TsField::required("reason", TsType::String)],
                            ),
                        ],
                    },
                ),
            ]),
        )
    }

    fn call(_input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(())
    }
}

#[test]
fn dts_renders_nested_namespaces_and_complex_schema_types() {
    let registry = InMemoryHostContractRegistry::new();
    registry
        .register_function::<ComplexGeneratedFunction>()
        .unwrap();

    let declarations = registry.dts().unwrap();

    assert_eq!(
        declarations,
        "type CreateInvoiceInput = { status: \"draft\" | \"paid\"; metadata?: Record<string, unknown>; lines: [string, number][]; paymentMethod: \"card\" | \"wire\"; };\n\ntype CreateInvoiceOutput = { id: string | null; state: { kind: \"created\"; } | { kind: \"failed\"; reason: string; }; };\n\ndeclare namespace billing {\n  namespace invoice {\n    export function create(input: CreateInvoiceInput): CreateInvoiceOutput;\n  }\n}\n"
    );
}

#[test]
fn dts_is_generated_from_contract_schemas() {
    let registry = InMemoryHostContractRegistry::new();
    registry.register_function::<DemoFunction>().unwrap();
    registry.register_callback::<DemoCallback>().unwrap();

    let declarations = registry.dts().unwrap();

    assert_eq!(
        declarations,
        "type DemoCallbackPayload = { combo: number; };\n\ntype DemoFunctionInput = void;\n\ndeclare namespace demo {\n  export function function(input: DemoFunctionInput): unknown;\n}\n\ntype HostEvents = {\n  \"demo.callback\": DemoCallbackPayload;\n};\n\ndeclare const ctx: {\n  on<K extends keyof HostEvents>(event: K, handler: (payload: HostEvents[K]) => void | Promise<void>): void;\n};\n"
    );
}

#[test]
fn dts_generates_function_declaration_from_contract_model() {
    let registry = InMemoryHostContractRegistry::new();
    registry.register_function::<GeneratedFunction>().unwrap();

    let declarations = registry.dts().unwrap();

    assert_eq!(
        declarations,
        "type FindUserInput = number;\n\ntype FindUserOutput = string;\n\ndeclare namespace user {\n  export function find(input: FindUserInput): FindUserOutput;\n}\n"
    );
}

#[test]
fn sdk_generates_function_wrapper_from_contract_model() {
    let registry = InMemoryHostContractRegistry::new();
    registry.register_function::<GeneratedFunction>().unwrap();

    let sdk = registry.sdk().unwrap();

    assert!(sdk.contains("type FindUserInput = number;"));
    assert!(sdk.contains("type FindUserOutput = string;"));
    assert!(sdk.contains("function __hostCall<T>(name: string, input?: unknown): T"));
    assert!(sdk.contains("export const user = {"));
    assert!(sdk.contains("find(input: FindUserInput): FindUserOutput"));
    assert!(sdk.contains("return __hostCall<FindUserOutput>(\"user.find\", input);"));
    assert!(sdk.contains("export const tsvmSdk = {"));
    assert!(sdk.contains("functions: {"));
    assert!(sdk.contains("user,"));
}

#[test]
fn sdk_generates_event_wrapper_from_callback_contracts() {
    let registry = InMemoryHostContractRegistry::new();
    registry.register_callback::<DemoCallback>().unwrap();

    let sdk = registry.sdk().unwrap();

    assert!(sdk.contains("type DemoCallbackPayload = { combo: number; };"));
    assert!(sdk.contains("type HostEvents = {"));
    assert!(sdk.contains("\"demo.callback\": DemoCallbackPayload;"));
    assert!(sdk.contains("export const ctx = __ctx;"));
    assert!(sdk.contains("export const events = {"));
    assert!(sdk.contains("callback(handler: HostEventHandler<\"demo.callback\">): void"));
    assert!(sdk.contains("return __ctx.on(\"demo.callback\", handler);"));
    assert!(sdk.contains("export const tsvmSdk = {"));
    assert!(sdk.contains("events,"));
    assert!(sdk.contains("ctx,"));
}

#[test]
fn types_alias_returns_declaration_output() {
    let registry = InMemoryHostContractRegistry::new();
    registry.register_function::<GeneratedFunction>().unwrap();

    assert_eq!(registry.types().unwrap(), registry.dts().unwrap());
}

#[test]
fn dts_renders_promise_return_only_for_async_promise_contracts() {
    let descriptors = vec![HostContractDescriptor {
        name: String::from("user.find"),
        kind: HostContractKind::Function,
        schema: Schema::typed("FindUserInput", TsType::Number),
        metadata: HostMetadata {
            name: String::from("user.find"),
            tags: Vec::new(),
        },
        import: HostImportBinding {
            module: String::from("test"),
            export_path: vec![String::from("user"), String::from("find")],
        },
        callback: Option::<HostCallbackDescriptor>::None,
        function: Some(HostFunctionDescriptor {
            input_schema: Schema::typed("FindUserInput", TsType::Number),
            output_schema: Schema::typed("FindUserOutput", TsType::String),
            execution: HostFunctionExecution::AsyncPromise,
        }),
        abi: HostContractAbi::Function {
            input: Schema::typed("FindUserInput", TsType::Number),
            output: Schema::typed("FindUserOutput", TsType::String),
            execution: HostFunctionExecution::AsyncPromise,
        },
    }];

    let declarations = super::declarations::render_typescript_declarations(&descriptors);

    assert_eq!(
        declarations,
        "type FindUserInput = number;\n\ntype FindUserOutput = string;\n\ndeclare namespace user {\n  export function find(input: FindUserInput): Promise<FindUserOutput>;\n}\n"
    );
}

#[test]
fn sdk_renders_promise_return_only_for_async_promise_contracts() {
    let descriptors = vec![HostContractDescriptor {
        name: String::from("user.find"),
        kind: HostContractKind::Function,
        schema: Schema::typed("FindUserInput", TsType::Number),
        metadata: HostMetadata {
            name: String::from("user.find"),
            tags: Vec::new(),
        },
        import: HostImportBinding {
            module: String::from("test"),
            export_path: vec![String::from("user"), String::from("find")],
        },
        callback: Option::<HostCallbackDescriptor>::None,
        function: Some(HostFunctionDescriptor {
            input_schema: Schema::typed("FindUserInput", TsType::Number),
            output_schema: Schema::typed("FindUserOutput", TsType::String),
            execution: HostFunctionExecution::AsyncPromise,
        }),
        abi: HostContractAbi::Function {
            input: Schema::typed("FindUserInput", TsType::Number),
            output: Schema::typed("FindUserOutput", TsType::String),
            execution: HostFunctionExecution::AsyncPromise,
        },
    }];

    let sdk = super::sdk::render_typescript_sdk(&descriptors);

    assert!(sdk.contains("async function __hostCallAsync<T>"));
    assert!(sdk.contains("find(input: FindUserInput): Promise<FindUserOutput>"));
    assert!(sdk.contains("return __hostCallAsync<FindUserOutput>(\"user.find\", input);"));
    assert!(sdk.contains("export const tsvmSdk = {"));
}
