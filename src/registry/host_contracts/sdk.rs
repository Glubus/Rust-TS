mod ident;
mod tree;

use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{
    HostContractAbi, HostContractDescriptor, HostFunctionExecution, Schema, TsType,
};

use super::declarations::{is_unknown_schema, render_ts_type, schema_type_name};
use ident::{identifier, indent, property_name};
use tree::{ObjectNode, ObjectTree};

const HOST_HELPERS: &str = include_str!("../../../assets/sdk_host_helpers.ts");
const EVENT_HELPERS: &str = include_str!("../../../assets/sdk_event_helpers.ts");

pub(crate) fn render_typescript_sdk(descriptors: &[HostContractDescriptor]) -> String {
    let mut builder = SdkBuilder::default();

    for descriptor in descriptors {
        builder.push_descriptor(descriptor);
    }

    builder.finish()
}

#[derive(Default)]
struct SdkBuilder {
    schemas: SchemaSection,
    functions: ObjectTree<SdkFunction>,
    events: ObjectTree<SdkEvent>,
    contexts: ObjectTree<SdkContext>,
    host_functions: BTreeMap<String, SdkFunctionType>,
    host_events: BTreeMap<String, String>,
    has_host_functions: bool,
    has_async_functions: bool,
}

impl SdkBuilder {
    fn push_descriptor(&mut self, descriptor: &HostContractDescriptor) {
        match &descriptor.abi {
            HostContractAbi::Function {
                input,
                output,
                execution,
            } => self.push_function(&descriptor.name, input, output, *execution),
            HostContractAbi::Callback { payload, hot, .. } => {
                self.push_callback(&descriptor.name, payload, *hot);
            }
            HostContractAbi::Context { schema } => self.push_context(&descriptor.name, schema),
            HostContractAbi::Unknown => {}
        }
    }

    fn push_function(
        &mut self,
        name: &str,
        input: &Schema,
        output: &Schema,
        execution: HostFunctionExecution,
    ) {
        self.schemas.push(input);
        self.schemas.push(output);
        self.has_host_functions = true;
        self.has_async_functions |= execution == HostFunctionExecution::AsyncPromise;
        self.host_functions.insert(
            name.to_owned(),
            SdkFunctionType {
                input_type: schema_type_name(input),
                output_type: schema_type_name(output),
                execution,
            },
        );
        self.functions.insert(
            name,
            SdkFunction {
                contract_name: name.to_owned(),
                input_type: schema_type_name(input),
                output_type: schema_type_name(output),
                takes_input: !matches!(input.ts_type, TsType::Void),
                execution,
            },
        );
    }

    fn push_callback(&mut self, name: &str, payload: &Schema, hot: bool) {
        if !hot {
            return;
        }

        self.schemas.push(payload);
        let payload_type = schema_type_name(payload);
        self.host_events
            .insert(name.to_owned(), payload_type.clone());
        self.events.insert(
            name,
            SdkEvent {
                event_name: name.to_owned(),
            },
        );
    }

    fn push_context(&mut self, name: &str, schema: &Schema) {
        self.schemas.push(schema);
        self.contexts.insert(
            name,
            SdkContext {
                contract_name: name.to_owned(),
                ty: schema_type_name(schema),
            },
        );
    }

    fn finish(self) -> String {
        let host_helpers = self.host_helpers();
        let host_function_api = self.host_function_api();
        let host_event_types = self.host_event_types();
        let event_helpers = self.event_helpers();
        let mut sections = self.schemas.finish();
        push_if_some(&mut sections, host_helpers);
        push_if_some(&mut sections, host_function_api);
        push_if_some(&mut sections, host_event_types);
        push_if_some(&mut sections, event_helpers);
        sections.extend(render_context_exports(&self.contexts));
        sections.extend(render_function_exports(&self.functions));
        push_if_some(&mut sections, render_events_export(&self.events));
        push_if_some(
            &mut sections,
            render_sdk_aggregate(&self.functions, &self.events, &self.contexts),
        );

        if sections.is_empty() {
            return String::new();
        }

        let mut output = sections.join("\n\n");
        output.push('\n');
        output
    }

    fn host_helpers(&self) -> Option<String> {
        self.has_host_functions.then(|| {
            if self.has_async_functions {
                HOST_HELPERS.trim().to_owned()
            } else {
                remove_async_helper(HOST_HELPERS).trim().to_owned()
            }
        })
    }

    fn host_event_types(&self) -> Option<String> {
        if self.host_events.is_empty() {
            return None;
        }

        let events = self
            .host_events
            .iter()
            .map(|(event_name, payload_type)| format!("  {event_name:?}: {payload_type};"))
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!("type HostEvents = {{\n{events}\n}};"))
    }

    fn host_function_api(&self) -> Option<String> {
        if self.host_functions.is_empty() {
            return None;
        }

        Some(render_host_function_api(
            &self.host_functions,
            self.has_async_functions,
        ))
    }

    fn event_helpers(&self) -> Option<String> {
        (!self.host_events.is_empty()).then(|| EVENT_HELPERS.trim().to_owned())
    }
}

#[derive(Default)]
struct SchemaSection {
    sections: Vec<String>,
    emitted: BTreeSet<String>,
}

impl SchemaSection {
    fn push(&mut self, schema: &Schema) {
        if is_unknown_schema(schema) || !self.emitted.insert(schema.name.clone()) {
            return;
        }

        for dependency in &schema.dependencies {
            self.push(dependency);
        }

        self.sections.push(format!(
            "type {} = {};",
            schema.name,
            render_ts_type(&schema.ts_type)
        ));
    }

    fn finish(self) -> Vec<String> {
        self.sections
    }
}

struct SdkFunction {
    contract_name: String,
    input_type: String,
    output_type: String,
    takes_input: bool,
    execution: HostFunctionExecution,
}

struct SdkFunctionType {
    input_type: String,
    output_type: String,
    execution: HostFunctionExecution,
}

struct SdkEvent {
    event_name: String,
}

struct SdkContext {
    contract_name: String,
    ty: String,
}

fn render_function_exports(tree: &ObjectTree<SdkFunction>) -> Vec<String> {
    render_exported_tree(tree, render_function_binding)
}

fn render_context_exports(tree: &ObjectTree<SdkContext>) -> Vec<String> {
    tree.roots
        .iter()
        .filter_map(|(name, node)| render_context_export(name, node))
        .collect()
}

fn render_context_export(name: &str, node: &ObjectNode<SdkContext>) -> Option<String> {
    node.binding.as_ref().map(|context| {
        format!(
            "export const {} = (globalThis as unknown as Record<string, unknown>)[{:?}] as {};",
            identifier(name),
            context.contract_name,
            context.ty
        )
    })
}

fn render_events_export(tree: &ObjectTree<SdkEvent>) -> Option<String> {
    if tree.is_empty() {
        return None;
    }

    Some(render_named_object_export(
        "events",
        &tree.roots,
        render_event_binding,
    ))
}

fn render_sdk_aggregate(
    functions: &ObjectTree<SdkFunction>,
    events: &ObjectTree<SdkEvent>,
    contexts: &ObjectTree<SdkContext>,
) -> Option<String> {
    let mut entries = Vec::new();

    if !functions.is_empty() {
        entries.push(format!(
            "{}functions: {},",
            indent(1),
            render_export_reference_object(&functions.roots, 1)
        ));
        entries.push(format!("{}call,", indent(1)));
    }
    if !events.is_empty() {
        entries.push(format!("{}events,", indent(1)));
        entries.push(format!("{}ctx,", indent(1)));
    }
    if !contexts.is_empty() {
        entries.push(format!(
            "{}contexts: {},",
            indent(1),
            render_export_reference_object(&contexts.roots, 1)
        ));
    }

    if entries.is_empty() {
        return None;
    }

    Some(format!(
        "export const tsvmSdk = {{\n{}\n}};",
        entries.join("\n")
    ))
}

fn render_host_function_api(
    functions: &BTreeMap<String, SdkFunctionType>,
    has_async_functions: bool,
) -> String {
    let host_functions = render_host_function_type_map(functions);
    let modes = render_host_function_mode_map(functions);
    let body = render_host_function_call_body(has_async_functions);
    format!(
        "{host_functions}\n\n\
type HostFunctionInput<K extends keyof HostFunctions> = HostFunctions[K][\"input\"];\n\
type HostFunctionOutput<K extends keyof HostFunctions> = HostFunctions[K][\"output\"];\n\
type HostFunctionReturn<K extends keyof HostFunctions> = HostFunctions[K][\"async\"] extends true ? Promise<HostFunctionOutput<K>> : HostFunctionOutput<K>;\n\
type HostFunctionArgs<K extends keyof HostFunctions> = HostFunctionInput<K> extends void ? [] : [input: HostFunctionInput<K>];\n\n\
{modes}\n\n\
export function call<K extends keyof HostFunctions>(name: K, ...args: HostFunctionArgs<K>): HostFunctionReturn<K> {{\n\
  const input = args[0] as HostFunctionInput<K> | undefined;\n\
{body}\n\
}}"
    )
}

fn render_host_function_call_body(has_async_functions: bool) -> String {
    if has_async_functions {
        return String::from(
            "  if (__hostFunctionModes[name] === \"async\") {\n\
    return __hostCallAsync<HostFunctionOutput<K>>(name, input) as HostFunctionReturn<K>;\n\
  }\n\
  return __hostCall<HostFunctionOutput<K>>(name, input) as HostFunctionReturn<K>;",
        );
    }

    String::from(
        "  return __hostCall<HostFunctionOutput<K>>(name, input) as HostFunctionReturn<K>;",
    )
}

fn render_host_function_type_map(functions: &BTreeMap<String, SdkFunctionType>) -> String {
    let entries = functions
        .iter()
        .map(|(name, function)| {
            format!(
                "  {name:?}: {{ input: {}; output: {}; async: {}; }};",
                function.input_type,
                function.output_type,
                is_async_promise(function.execution)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("type HostFunctions = {{\n{entries}\n}};")
}

fn render_host_function_mode_map(functions: &BTreeMap<String, SdkFunctionType>) -> String {
    let entries = functions
        .iter()
        .map(|(name, function)| {
            let mode = if is_async_promise(function.execution) {
                "async"
            } else {
                "sync"
            };
            format!("  {name:?}: {mode:?},")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("const __hostFunctionModes = {{\n{entries}\n}} as const;")
}

fn is_async_promise(execution: HostFunctionExecution) -> bool {
    execution == HostFunctionExecution::AsyncPromise
}

fn remove_async_helper(source: &str) -> String {
    let Some(start) = source.find("\nasync function __hostCallAsync") else {
        return source.to_owned();
    };

    source[..start].to_owned()
}

fn render_export_reference_object<T>(
    roots: &BTreeMap<String, ObjectNode<T>>,
    depth: usize,
) -> String {
    let entries = roots
        .keys()
        .map(|name| {
            format!(
                "{}{}: {},",
                indent(depth + 1),
                property_name(name),
                identifier(name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    if entries.is_empty() {
        String::from("{}")
    } else {
        format!("{{\n{entries}\n{}}}", indent(depth))
    }
}

fn render_exported_tree<T>(
    tree: &ObjectTree<T>,
    render_binding: fn(&str, &T, usize) -> String,
) -> Vec<String> {
    tree.roots
        .iter()
        .map(|(name, node)| {
            let body = render_object_node(node, 1, render_binding);
            format!("export const {} = {body};", identifier(name))
        })
        .collect()
}

fn render_named_object_export<T>(
    export_name: &str,
    roots: &BTreeMap<String, ObjectNode<T>>,
    render_binding: fn(&str, &T, usize) -> String,
) -> String {
    let body = render_object_entries(roots, 1, render_binding);
    format!("export const {export_name} = {{\n{body}\n}};")
}

fn render_object_node<T>(
    node: &ObjectNode<T>,
    depth: usize,
    render_binding: fn(&str, &T, usize) -> String,
) -> String {
    let body = render_node_entries(node, depth, render_binding);
    if body.is_empty() {
        String::from("{}")
    } else {
        format!("{{\n{body}\n{}}}", indent(depth - 1))
    }
}

fn render_node_entries<T>(
    node: &ObjectNode<T>,
    depth: usize,
    render_binding: fn(&str, &T, usize) -> String,
) -> String {
    let mut entries = Vec::new();

    if let Some(binding) = &node.binding {
        entries.push(render_binding("default", binding, depth));
    }

    entries.extend(
        render_object_entries(&node.children, depth, render_binding)
            .lines()
            .map(str::to_owned),
    );
    entries.join("\n")
}

fn render_object_entries<T>(
    nodes: &BTreeMap<String, ObjectNode<T>>,
    depth: usize,
    render_binding: fn(&str, &T, usize) -> String,
) -> String {
    nodes
        .iter()
        .map(|(name, node)| {
            if let (Some(binding), true) = (&node.binding, node.children.is_empty()) {
                render_binding(name, binding, depth)
            } else {
                render_namespace_entry(name, node, depth, render_binding)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_namespace_entry<T>(
    name: &str,
    node: &ObjectNode<T>,
    depth: usize,
    render_binding: fn(&str, &T, usize) -> String,
) -> String {
    let child_body = render_node_entries(node, depth + 1, render_binding);
    format!(
        "{}{}: {{\n{}\n{}}},",
        indent(depth),
        property_name(name),
        child_body,
        indent(depth)
    )
}

fn render_function_binding(name: &str, function: &SdkFunction, depth: usize) -> String {
    let parameter = function_parameter(function);
    let return_type = function_return_type(function);
    let input_argument = function_input_argument(function);
    let call = match function.execution {
        HostFunctionExecution::Sync | HostFunctionExecution::AsyncBlockingJs => {
            format!(
                "return __hostCall<{}>({:?}{});",
                function.output_type, function.contract_name, input_argument
            )
        }
        HostFunctionExecution::AsyncPromise => {
            format!(
                "return __hostCallAsync<{}>({:?}{});",
                function.output_type, function.contract_name, input_argument
            )
        }
    };

    format!(
        "{}{}({}): {} {{\n{}{}\n{}}},",
        indent(depth),
        property_name(name),
        parameter,
        return_type,
        indent(depth + 1),
        call,
        indent(depth)
    )
}

fn function_parameter(function: &SdkFunction) -> String {
    if function.takes_input {
        format!("input: {}", function.input_type)
    } else {
        String::new()
    }
}

fn function_return_type(function: &SdkFunction) -> String {
    match function.execution {
        HostFunctionExecution::Sync | HostFunctionExecution::AsyncBlockingJs => {
            function.output_type.clone()
        }
        HostFunctionExecution::AsyncPromise => format!("Promise<{}>", function.output_type),
    }
}

fn function_input_argument(function: &SdkFunction) -> String {
    if function.takes_input {
        String::from(", input")
    } else {
        String::new()
    }
}

fn render_event_binding(name: &str, event: &SdkEvent, depth: usize) -> String {
    format!(
        "{}{}(handler: HostEventHandler<{:?}>): void {{\n{}return __ctx.on({:?}, handler);\n{}}},",
        indent(depth),
        property_name(name),
        event.event_name,
        indent(depth + 1),
        event.event_name,
        indent(depth)
    )
}

fn push_if_some(sections: &mut Vec<String>, section: Option<String>) {
    if let Some(section) = section
        && !section.trim().is_empty()
    {
        sections.push(section);
    }
}
