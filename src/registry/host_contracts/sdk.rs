mod ident;
mod tree;

use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{
    HostContractAbi, HostContractDescriptor, HostFunctionExecution, Schema, TsField, TsLiteral,
    TsRecordKey, TsType,
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
        let (schema_sections, models) = self.schemas.finish();
        let mut sections = schema_sections;
        push_if_some(&mut sections, render_model_runtime_helpers(&models));
        sections.extend(render_model_classes(&models));
        push_if_some(&mut sections, render_model_helpers(&models));
        push_if_some(&mut sections, host_helpers);
        push_if_some(&mut sections, host_function_api);
        push_if_some(&mut sections, host_event_types);
        push_if_some(&mut sections, event_helpers);
        sections.extend(render_context_exports(&self.contexts));
        sections.extend(render_domain_exports(&self.functions, &self.events));
        push_if_some(&mut sections, render_events_export(&self.events));
        push_if_some(
            &mut sections,
            render_sdk_aggregate(&self.functions, &self.events, &self.contexts, &models),
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
    models: BTreeMap<String, TsType>,
}

impl SchemaSection {
    fn push(&mut self, schema: &Schema) {
        if is_unknown_schema(schema) || !self.emitted.insert(schema.name.clone()) {
            return;
        }

        for dependency in &schema.dependencies {
            self.push(dependency);
        }

        if matches!(schema.ts_type, TsType::Object(_)) {
            self.models
                .insert(schema.name.clone(), schema.ts_type.clone());
        }

        self.sections.push(format!(
            "type {} = {};",
            schema.name,
            render_ts_type(&schema.ts_type)
        ));
    }

    fn finish(self) -> (Vec<String>, BTreeMap<String, TsType>) {
        (self.sections, self.models)
    }
}

#[derive(Clone)]
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

#[derive(Clone)]
struct SdkEventAlias {
    event_name: String,
}

enum SdkDomainBinding {
    Function(SdkFunction),
    EventAlias(SdkEventAlias),
}

struct SdkContext {
    contract_name: String,
    ty: String,
}

fn render_domain_exports(
    functions: &ObjectTree<SdkFunction>,
    events: &ObjectTree<SdkEvent>,
) -> Vec<String> {
    let mut tree = ObjectTree::default();
    push_function_domain_bindings(&mut tree, &functions.roots, String::new());
    push_event_alias_domain_bindings(&mut tree, events);
    render_exported_tree(&tree, render_domain_binding)
}

fn push_function_domain_bindings(
    output: &mut ObjectTree<SdkDomainBinding>,
    nodes: &BTreeMap<String, ObjectNode<SdkFunction>>,
    prefix: String,
) {
    for (name, node) in nodes {
        let path = child_path(&prefix, name);
        if let Some(function) = &node.binding {
            output.insert(&path, SdkDomainBinding::Function(function.clone()));
        }
        push_function_domain_bindings(output, &node.children, path);
    }
}

fn push_event_alias_domain_bindings(
    output: &mut ObjectTree<SdkDomainBinding>,
    events: &ObjectTree<SdkEvent>,
) {
    for (root, node) in &events.roots {
        push_event_aliases_for_root(output, root, node, Vec::new());
    }
}

fn push_event_aliases_for_root(
    output: &mut ObjectTree<SdkDomainBinding>,
    root: &str,
    node: &ObjectNode<SdkEvent>,
    segments: Vec<String>,
) {
    if let Some(event) = &node.binding
        && let Some(alias) = event_alias_path(root, &segments)
    {
        output.insert(
            &alias,
            SdkDomainBinding::EventAlias(SdkEventAlias {
                event_name: event.event_name.clone(),
            }),
        );
    }

    for (name, child) in &node.children {
        let mut child_segments = segments.clone();
        child_segments.push(name.clone());
        push_event_aliases_for_root(output, root, child, child_segments);
    }
}

fn event_alias_path(root: &str, segments: &[String]) -> Option<String> {
    if segments.is_empty() {
        return None;
    }

    Some(format!("{root}.on{}", pascal_case(segments)))
}

fn child_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}.{name}")
    }
}

fn pascal_case(segments: &[String]) -> String {
    segments
        .iter()
        .map(|segment| {
            segment
                .split(['-', '_', ' '])
                .filter(|part| !part.is_empty())
                .map(capitalize_identifier_part)
                .collect::<String>()
        })
        .collect::<String>()
}

fn capitalize_identifier_part(part: &str) -> String {
    let mut chars = part.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
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
    models: &BTreeMap<String, TsType>,
) -> Option<String> {
    let mut entries = Vec::new();

    if !models.is_empty() {
        entries.push(format!("{}models,", indent(1)));
    }
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
        "export const rusttsSdk = {{\n{}\n}};",
        entries.join("\n")
    ))
}

fn render_model_helpers(models: &BTreeMap<String, TsType>) -> Option<String> {
    if models.is_empty() {
        return None;
    }

    let entries = models
        .iter()
        .map(|(name, ty)| render_model_helper_entry(name, ty, 1))
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!("export const models = {{\n{entries}\n}};"))
}

fn render_model_classes(models: &BTreeMap<String, TsType>) -> Vec<String> {
    models
        .iter()
        .map(|(name, ty)| render_model_class(name, ty))
        .collect()
}

fn render_model_runtime_helpers(models: &BTreeMap<String, TsType>) -> Option<String> {
    if models.is_empty() {
        return None;
    }

    let predicates = models
        .iter()
        .map(|(name, ty)| render_model_predicate_function(name, ty))
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(format!(
        "function __isRecord(value: unknown): value is Record<string, unknown> {{\n\
  return typeof value === \"object\" && value !== null && !Array.isArray(value);\n\
}}\n\n\
function __field(value: unknown, key: string): unknown {{\n\
  return __isRecord(value) ? value[key] : undefined;\n\
}}\n\n\
function __isArrayOf(value: unknown, predicate: (item: unknown) => boolean): boolean {{\n\
  return Array.isArray(value) && value.every(predicate);\n\
}}\n\n\
function __isRecordOf(value: unknown, predicate: (item: unknown, key: string) => boolean): boolean {{\n\
  return __isRecord(value) && Object.entries(value).every(([key, item]) => predicate(item, key));\n\
}}\n\n\
{predicates}"
    ))
}

fn render_model_predicate_function(name: &str, ty: &TsType) -> String {
    let predicate_name = model_predicate_name(name);
    let predicate = render_value_predicate(ty, "value");
    format!(
        "function {predicate_name}(value: unknown): value is {name} {{\n\
  return {predicate};\n\
}}"
    )
}

fn render_model_class(name: &str, _ty: &TsType) -> String {
    let class_name = model_class_name(name);
    let predicate_name = model_predicate_name(name);
    format!(
        "export class {class_name} {{\n\
  constructor(public readonly value: {name}) {{}}\n\n\
  static create(value: {name}): {name} {{\n\
    return value;\n\
  }}\n\n\
  static is(value: unknown): value is {name} {{\n\
    return {predicate_name}(value);\n\
  }}\n\n\
  static wrap(value: {name}): {class_name} {{\n\
    return new {class_name}(value);\n\
  }}\n\n\
  toJSON(): {name} {{\n\
    return this.value;\n\
  }}\n\n\
  valueOf(): {name} {{\n\
    return this.value;\n\
  }}\n\
}}"
    )
}

fn render_model_helper_entry(name: &str, _ty: &TsType, depth: usize) -> String {
    let class_name = model_class_name(name);
    let predicate_name = model_predicate_name(name);
    format!(
        "{}{}: {{\n{}create(value: {name}): {name} {{\n{}return {class_name}.create(value);\n{}}},\n{}is(value: unknown): value is {name} {{\n{}return {predicate_name}(value);\n{}}},\n{}wrap(value: {name}): {class_name} {{\n{}return {class_name}.wrap(value);\n{}}},\n{}}},",
        indent(depth),
        property_name(name),
        indent(depth + 1),
        indent(depth + 2),
        indent(depth + 1),
        indent(depth + 1),
        indent(depth + 2),
        indent(depth + 1),
        indent(depth + 1),
        indent(depth + 2),
        indent(depth + 1),
        indent(depth)
    )
}

fn model_predicate_name(name: &str) -> String {
    format!("__is{}", identifier(name))
}

fn model_class_name(name: &str) -> String {
    format!("{name}Model")
}

fn render_value_predicate(ty: &TsType, expression: &str) -> String {
    match ty {
        TsType::Unknown | TsType::Json | TsType::TypeRef(_) => String::from("true"),
        TsType::Void => format!("{expression} === undefined"),
        TsType::Boolean => format!("typeof {expression} === \"boolean\""),
        TsType::Number => {
            format!("typeof {expression} === \"number\" && Number.isFinite({expression})")
        }
        TsType::String => format!("typeof {expression} === \"string\""),
        TsType::Uint8Array => format!("{expression} instanceof Uint8Array"),
        TsType::Null => format!("{expression} === null"),
        TsType::Literal(literal) => render_literal_predicate(literal, expression),
        TsType::Object(fields) => render_object_predicate(fields, expression),
        TsType::Array(item) => {
            let item_predicate = render_value_predicate(item, "item");
            format!("__isArrayOf({expression}, item => {item_predicate})")
        }
        TsType::Tuple(items) => render_tuple_predicate(items, expression),
        TsType::Enum { tag, variants } => {
            render_enum_predicate(tag.as_deref(), variants, expression)
        }
        TsType::Record { key, value } => render_record_predicate(*key, value, expression),
        TsType::Union(types) => {
            let predicates = types
                .iter()
                .map(|ty| render_value_predicate(ty, expression))
                .collect::<Vec<_>>();
            join_predicates(predicates, " || ")
        }
        TsType::Optional(inner) => {
            format!(
                "{expression} === undefined || ({})",
                render_value_predicate(inner, expression)
            )
        }
        TsType::Nullable(inner) => {
            format!(
                "{expression} === null || ({})",
                render_value_predicate(inner, expression)
            )
        }
    }
}

fn render_literal_predicate(literal: &TsLiteral, expression: &str) -> String {
    match literal {
        TsLiteral::String(value) => format!("{expression} === {value:?}"),
        TsLiteral::Number(value) => format!("{expression} === {value}"),
        TsLiteral::Boolean(value) => format!("{expression} === {value}"),
    }
}

fn render_object_predicate(fields: &[TsField], expression: &str) -> String {
    let base = format!("__isRecord({expression})");
    let field_predicates = fields
        .iter()
        .map(|field| render_field_predicate(field, expression))
        .collect::<Vec<_>>();
    join_predicates(
        std::iter::once(base).chain(field_predicates).collect(),
        " && ",
    )
}

fn render_field_predicate(field: &TsField, expression: &str) -> String {
    let access = format!("__field({expression}, {:?})", field.name);
    let predicate = render_value_predicate(&field.ty, &access);
    if field.optional {
        format!("{access} === undefined || ({predicate})")
    } else {
        predicate
    }
}

fn render_tuple_predicate(items: &[TsType], expression: &str) -> String {
    let base = format!(
        "Array.isArray({expression}) && {expression}.length === {}",
        items.len()
    );
    let item_predicates = items
        .iter()
        .enumerate()
        .map(|(index, ty)| {
            render_value_predicate(ty, &format!("({expression} as unknown[])[{index}]"))
        })
        .collect::<Vec<_>>();
    join_predicates(
        std::iter::once(base).chain(item_predicates).collect(),
        " && ",
    )
}

fn render_enum_predicate(
    tag: Option<&str>,
    variants: &[crate::contract::TsEnumVariant],
    expression: &str,
) -> String {
    if variants.iter().all(|variant| variant.fields.is_empty()) {
        let predicates = variants
            .iter()
            .map(|variant| format!("{expression} === {:?}", variant.name))
            .collect::<Vec<_>>();
        return join_predicates(predicates, " || ");
    }

    let tag = tag.unwrap_or("type");
    let variant_predicates = variants
        .iter()
        .map(|variant| {
            let tag_access = format!("__field({expression}, {tag:?})");
            let mut predicates = vec![format!("{tag_access} === {:?}", variant.name)];
            predicates.extend(
                variant
                    .fields
                    .iter()
                    .map(|field| render_field_predicate(field, expression)),
            );
            format!("({})", join_predicates(predicates, " && "))
        })
        .collect::<Vec<_>>();
    format!(
        "{} && ({})",
        render_object_predicate(&[], expression),
        join_predicates(variant_predicates, " || ")
    )
}

fn render_record_predicate(key: TsRecordKey, value: &TsType, expression: &str) -> String {
    let value_predicate = render_value_predicate(value, "item");
    let key_predicate = match key {
        TsRecordKey::String => String::from("true"),
        TsRecordKey::Number => String::from("Number.isFinite(Number(key))"),
    };
    format!("__isRecordOf({expression}, (item, key) => {key_predicate} && ({value_predicate}))")
}

fn join_predicates(predicates: Vec<String>, separator: &str) -> String {
    if predicates.is_empty() {
        return String::from("true");
    }
    predicates.join(separator)
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

fn render_domain_binding(name: &str, binding: &SdkDomainBinding, depth: usize) -> String {
    match binding {
        SdkDomainBinding::Function(function) => render_function_binding(name, function, depth),
        SdkDomainBinding::EventAlias(event) => render_event_alias_binding(name, event, depth),
    }
}

fn render_event_alias_binding(name: &str, event: &SdkEventAlias, depth: usize) -> String {
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
