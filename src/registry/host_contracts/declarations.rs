use std::collections::BTreeSet;

use crate::contract::{
    DeliveryMode, HostContractAbi, HostContractDescriptor, HostFunctionExecution, Schema,
    TsEnumVariant, TsField, TsLiteral, TsRecordKey, TsType,
};

const DEFAULT_ENUM_TAG: &str = "type";

pub(crate) fn render_typescript_declarations(descriptors: &[HostContractDescriptor]) -> String {
    let mut declarations = DeclarationBuffer::default();

    for descriptor in descriptors {
        declarations.push_descriptor(descriptor);
    }

    declarations.finish()
}

#[derive(Default)]
struct DeclarationBuffer {
    sections: Vec<String>,
    host_events: Vec<String>,
    emitted_schemas: BTreeSet<String>,
}

impl DeclarationBuffer {
    fn push_descriptor(&mut self, descriptor: &HostContractDescriptor) {
        match &descriptor.abi {
            HostContractAbi::Function {
                input,
                output,
                execution,
            } => {
                self.push_function(&descriptor.name, input, output, *execution);
            }
            HostContractAbi::Callback {
                payload,
                delivery,
                hot,
            } => self.push_callback(&descriptor.name, payload, *delivery, *hot),
            HostContractAbi::Context { schema } => self.push_context(&descriptor.name, schema),
            HostContractAbi::Unknown => {}
        }
    }

    fn push_schema(&mut self, schema: &Schema) {
        if is_unknown_schema(schema) || !self.emitted_schemas.insert(schema.name.clone()) {
            return;
        }

        for dependency in &schema.dependencies {
            self.push_schema(dependency);
        }

        self.sections.push(format!(
            "type {} = {};",
            schema.name,
            render_ts_type(&schema.ts_type)
        ));
    }

    fn push_function(
        &mut self,
        name: &str,
        input: &Schema,
        output: &Schema,
        execution: HostFunctionExecution,
    ) {
        self.push_schema(input);
        self.push_schema(output);
        self.sections
            .push(render_function_declaration(name, input, output, execution));
    }

    fn push_callback(&mut self, name: &str, payload: &Schema, _delivery: DeliveryMode, hot: bool) {
        if !hot {
            return;
        }

        self.push_schema(payload);
        self.host_events
            .push(format!("  {name:?}: {};", schema_type_name(payload)));
    }

    fn push_context(&mut self, name: &str, schema: &Schema) {
        self.push_schema(schema);
        self.sections.push(format!(
            "declare const {name}: {};",
            schema_type_name(schema)
        ));
    }

    fn finish(mut self) -> String {
        self.push_host_events();
        if self.sections.is_empty() {
            return String::new();
        }

        let mut output = self.sections.join("\n\n");
        output.push('\n');
        output
    }

    fn push_host_events(&mut self) {
        if self.host_events.is_empty() {
            return;
        }

        self.host_events.sort();
        self.sections.push(format!(
            "type HostEvents = {{\n{}\n}};",
            self.host_events.join("\n")
        ));
        self.sections.push(String::from(
            "declare const ctx: {\n  on<K extends keyof HostEvents>(event: K, handler: (payload: HostEvents[K]) => void | Promise<void>): void;\n};",
        ));
    }
}

fn render_function_declaration(
    name: &str,
    input: &Schema,
    output: &Schema,
    execution: HostFunctionExecution,
) -> String {
    let name_parts = split_contract_name(name);
    let return_type = render_function_return_type(output, execution);
    let signature = format!(
        "function {}(input: {}): {};",
        name_parts.function_name,
        schema_type_name(input),
        return_type
    );

    if name_parts.namespaces.is_empty() {
        return format!("declare {signature}");
    }

    render_nested_namespace(&name_parts.namespaces, &signature)
}

fn render_function_return_type(output: &Schema, execution: HostFunctionExecution) -> String {
    let output_type = schema_type_name(output);
    match execution {
        HostFunctionExecution::Sync | HostFunctionExecution::AsyncBlockingJs => output_type,
        HostFunctionExecution::AsyncPromise => format!("Promise<{output_type}>"),
    }
}

struct ContractNameParts<'a> {
    namespaces: Vec<&'a str>,
    function_name: &'a str,
}

fn split_contract_name(name: &str) -> ContractNameParts<'_> {
    let mut segments = name.split('.').collect::<Vec<_>>();
    let function_name = segments.pop().unwrap_or(name);
    ContractNameParts {
        namespaces: segments,
        function_name,
    }
}

fn render_nested_namespace(namespaces: &[&str], signature: &str) -> String {
    let mut output = String::new();

    for (depth, namespace) in namespaces.iter().enumerate() {
        output.push_str(&indent(depth));
        if depth == 0 {
            output.push_str("declare ");
        }
        output.push_str("namespace ");
        output.push_str(namespace);
        output.push_str(" {\n");
    }

    output.push_str(&indent(namespaces.len()));
    output.push_str("export ");
    output.push_str(signature);
    output.push('\n');

    for depth in (0..namespaces.len()).rev() {
        output.push_str(&indent(depth));
        output.push('}');
        if depth > 0 {
            output.push('\n');
        }
    }

    output
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

pub(super) fn schema_type_name(schema: &Schema) -> String {
    if schema.name.trim().is_empty() || schema.name == "unknown" {
        render_ts_type(&schema.ts_type)
    } else {
        schema.name.clone()
    }
}

pub(super) fn render_ts_type(ty: &TsType) -> String {
    match ty {
        TsType::Unknown => String::from("unknown"),
        TsType::Void => String::from("void"),
        TsType::Boolean => String::from("boolean"),
        TsType::Number => String::from("number"),
        TsType::String => String::from("string"),
        TsType::Json => String::from("unknown"),
        TsType::Uint8Array => String::from("Uint8Array"),
        TsType::Null => String::from("null"),
        TsType::TypeRef(name) => name.clone(),
        TsType::Literal(literal) => render_literal(literal),
        TsType::Object(fields) => render_object_type(fields),
        TsType::Array(item) => format!("{}[]", render_wrapped_array_type(item)),
        TsType::Tuple(items) => render_tuple_type(items),
        TsType::Enum { tag, variants } => render_enum_type(tag.as_deref(), variants),
        TsType::Record { key, value } => render_record_type(*key, value),
        TsType::Union(types) => render_union_type(types),
        TsType::Optional(inner) => format!("{} | undefined", render_ts_type(inner)),
        TsType::Nullable(inner) => format!("{} | null", render_ts_type(inner)),
    }
}

fn render_literal(literal: &TsLiteral) -> String {
    match literal {
        TsLiteral::String(value) => render_string_literal(value),
        TsLiteral::Number(value) => value.clone(),
        TsLiteral::Boolean(value) => value.to_string(),
    }
}

fn render_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn render_object_type(fields: &[TsField]) -> String {
    if fields.is_empty() {
        return String::from("{}");
    }

    let rendered_fields = fields
        .iter()
        .map(render_field)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{{ {rendered_fields} }}")
}

fn render_field(field: &TsField) -> String {
    let optional = if field.optional { "?" } else { "" };
    format!("{}{}: {};", field.name, optional, render_ts_type(&field.ty))
}

fn render_tuple_type(items: &[TsType]) -> String {
    let rendered_items = items
        .iter()
        .map(render_ts_type)
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered_items}]")
}

fn render_enum_type(tag: Option<&str>, variants: &[TsEnumVariant]) -> String {
    if variants.is_empty() {
        return String::from("never");
    }

    if variants.iter().all(|variant| variant.fields.is_empty()) {
        return variants
            .iter()
            .map(|variant| render_string_literal(&variant.name))
            .collect::<Vec<_>>()
            .join(" | ");
    }

    let tag = tag.unwrap_or(DEFAULT_ENUM_TAG);
    variants
        .iter()
        .map(|variant| render_enum_variant_type(tag, variant))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn render_enum_variant_type(tag: &str, variant: &TsEnumVariant) -> String {
    let discriminant = TsField::required(
        tag,
        TsType::Literal(TsLiteral::String(variant.name.clone())),
    );
    let fields = std::iter::once(&discriminant)
        .chain(variant.fields.iter())
        .map(render_field)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{{ {fields} }}")
}

fn render_record_type(key: TsRecordKey, value: &TsType) -> String {
    let key_type = match key {
        TsRecordKey::String => "string",
        TsRecordKey::Number => "number",
    };
    format!("Record<{key_type}, {}>", render_ts_type(value))
}

fn render_union_type(types: &[TsType]) -> String {
    if types.is_empty() {
        return String::from("never");
    }

    types
        .iter()
        .map(render_ts_type)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn render_wrapped_array_type(item: &TsType) -> String {
    match item {
        TsType::Object(_)
        | TsType::Enum { .. }
        | TsType::Optional(_)
        | TsType::Nullable(_)
        | TsType::Union(_)
        | TsType::Record { .. } => format!("({})", render_ts_type(item)),
        _ => render_ts_type(item),
    }
}

pub(super) fn is_unknown_schema(schema: &Schema) -> bool {
    schema.name.trim().is_empty() || schema.name == "unknown"
}
