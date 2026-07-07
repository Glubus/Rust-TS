//! Runtime validation for host contract schemas.

use serde_json::Value;

use super::{Schema, TsEnumVariant, TsField, TsLiteral, TsRecordKey, TsType};

pub(crate) fn validate_schema_with_options(
    schema: &Schema,
    value: &Value,
    options: SchemaValidationOptions,
) -> Result<(), String> {
    let mut context = ValidationContext::new(&schema.dependencies);
    validate_type(&schema.ts_type, value, "$", options, &mut context)
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SchemaValidationOptions {
    pub(crate) reject_unknown_fields: bool,
}

struct ValidationContext<'a> {
    dependencies: &'a [Schema],
    resolving_type_refs: Vec<String>,
}

impl<'a> ValidationContext<'a> {
    fn new(dependencies: &'a [Schema]) -> Self {
        Self {
            dependencies,
            resolving_type_refs: Vec::new(),
        }
    }
}

fn validate_type(
    ty: &TsType,
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    match ty {
        TsType::Unknown | TsType::Json => Ok(()),
        TsType::TypeRef(name) => validate_type_ref(name, value, path, options, context),
        TsType::Void | TsType::Null => validate_null(value, path),
        TsType::Boolean => validate_bool(value, path),
        TsType::Number => validate_number(value, path),
        TsType::String => validate_string(value, path),
        TsType::Uint8Array => validate_array(&TsType::Number, value, path, options, context),
        TsType::Literal(literal) => validate_literal(literal, value, path),
        TsType::Object(fields) => validate_object(fields, value, path, options, context),
        TsType::Array(item) => validate_array(item, value, path, options, context),
        TsType::Tuple(items) => validate_tuple(items, value, path, options, context),
        TsType::Enum { tag, variants } => {
            validate_enum(tag.as_deref(), variants, value, path, options, context)
        }
        TsType::Record { key, value: item } => {
            validate_record(*key, item, value, path, options, context)
        }
        TsType::Union(types) => validate_union(types, value, path, options, context),
        TsType::Optional(inner) => validate_optional(inner, value, path, options, context),
        TsType::Nullable(inner) => validate_nullable(inner, value, path, options, context),
    }
}

fn validate_type_ref(
    name: &str,
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    if context
        .resolving_type_refs
        .iter()
        .any(|resolving| resolving == name)
    {
        return Ok(());
    }

    let Some(schema) = find_schema_dependency(context.dependencies, name) else {
        return Ok(());
    };

    context.resolving_type_refs.push(name.to_owned());
    let result = validate_type(&schema.ts_type, value, path, options, context);
    context.resolving_type_refs.pop();
    result
}

fn find_schema_dependency<'a>(dependencies: &'a [Schema], name: &str) -> Option<&'a Schema> {
    for schema in dependencies {
        if schema.name == name {
            return Some(schema);
        }
        if let Some(schema) = find_schema_dependency(&schema.dependencies, name) {
            return Some(schema);
        }
    }
    None
}

fn validate_null(value: &Value, path: &str) -> Result<(), String> {
    value
        .is_null()
        .then_some(())
        .ok_or_else(|| expected(path, "null", value))
}

fn validate_bool(value: &Value, path: &str) -> Result<(), String> {
    value
        .is_boolean()
        .then_some(())
        .ok_or_else(|| expected(path, "boolean", value))
}

fn validate_number(value: &Value, path: &str) -> Result<(), String> {
    value
        .is_number()
        .then_some(())
        .ok_or_else(|| expected(path, "number", value))
}

fn validate_string(value: &Value, path: &str) -> Result<(), String> {
    value
        .is_string()
        .then_some(())
        .ok_or_else(|| expected(path, "string", value))
}

fn validate_literal(literal: &TsLiteral, value: &Value, path: &str) -> Result<(), String> {
    let valid = match literal {
        TsLiteral::String(expected) => value.as_str() == Some(expected.as_str()),
        TsLiteral::Number(expected) => literal_number_matches(expected, value),
        TsLiteral::Boolean(expected) => value.as_bool() == Some(*expected),
    };
    valid
        .then_some(())
        .ok_or_else(|| format!("{path}: expected literal {}", literal_label(literal)))
}

fn literal_number_matches(expected: &str, value: &Value) -> bool {
    value
        .as_f64()
        .and_then(|number| {
            expected
                .parse::<f64>()
                .ok()
                .map(|expected| number == expected)
        })
        .unwrap_or(false)
}

fn literal_label(literal: &TsLiteral) -> String {
    match literal {
        TsLiteral::String(value) => format!("{value:?}"),
        TsLiteral::Number(value) => value.clone(),
        TsLiteral::Boolean(value) => value.to_string(),
    }
}

fn validate_object(
    fields: &[TsField],
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    validate_object_with_allowed_fields(fields, value, path, options, context, &[])
}

fn validate_object_with_allowed_fields(
    fields: &[TsField],
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
    allowed_extra_fields: &[&str],
) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Err(expected(path, "object", value));
    };

    if options.reject_unknown_fields {
        reject_unknown_object_fields(fields, object, path, allowed_extra_fields)?;
    }

    for field in fields {
        let field_path = child_path(path, &field.name);
        match object.get(&field.name) {
            Some(field_value) => {
                validate_type(&field.ty, field_value, &field_path, options, context)?;
            }
            None if field.optional => {}
            None => return Err(format!("{field_path}: missing required field")),
        }
    }
    Ok(())
}

fn reject_unknown_object_fields(
    fields: &[TsField],
    object: &serde_json::Map<String, Value>,
    path: &str,
    allowed_extra_fields: &[&str],
) -> Result<(), String> {
    for field in object.keys() {
        if !is_declared_field(fields, field) && !allowed_extra_fields.contains(&field.as_str()) {
            return Err(format!("{}: unknown field", child_path(path, field)));
        }
    }
    Ok(())
}

fn is_declared_field(fields: &[TsField], field: &str) -> bool {
    fields.iter().any(|declared| declared.name == field)
}

fn validate_array(
    item: &TsType,
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    let Some(items) = value.as_array() else {
        return Err(expected(path, "array", value));
    };

    for (index, item_value) in items.iter().enumerate() {
        validate_type(item, item_value, &index_path(path, index), options, context)?;
    }
    Ok(())
}

fn validate_tuple(
    items: &[TsType],
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    let Some(values) = value.as_array() else {
        return Err(expected(path, "tuple array", value));
    };
    if values.len() != items.len() {
        return Err(format!(
            "{path}: expected tuple length {}, got {}",
            items.len(),
            values.len()
        ));
    }

    for (index, (ty, item_value)) in items.iter().zip(values.iter()).enumerate() {
        validate_type(ty, item_value, &index_path(path, index), options, context)?;
    }
    Ok(())
}

fn validate_enum(
    tag: Option<&str>,
    variants: &[TsEnumVariant],
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    match tag {
        Some(tag) => validate_tagged_enum(tag, variants, value, path, options, context),
        None => validate_unit_enum(variants, value, path),
    }
}

fn validate_unit_enum(variants: &[TsEnumVariant], value: &Value, path: &str) -> Result<(), String> {
    let Some(value) = value.as_str() else {
        return Err(expected(path, "enum string literal", value));
    };
    variants
        .iter()
        .any(|variant| variant.name == value)
        .then_some(())
        .ok_or_else(|| format!("{path}: unknown enum variant {value:?}"))
}

fn validate_tagged_enum(
    tag: &str,
    variants: &[TsEnumVariant],
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Err(expected(path, "tagged enum object", value));
    };
    let tag_path = child_path(path, tag);
    let Some(tag_value) = object.get(tag).and_then(Value::as_str) else {
        return Err(format!("{tag_path}: missing enum tag string"));
    };
    let Some(variant) = variants.iter().find(|variant| variant.name == tag_value) else {
        return Err(format!("{tag_path}: unknown enum variant {tag_value:?}"));
    };

    validate_object_with_allowed_fields(&variant.fields, value, path, options, context, &[tag])
}

fn validate_record(
    key: TsRecordKey,
    item: &TsType,
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Err(expected(path, "record object", value));
    };

    for (field, field_value) in object {
        validate_record_key(key, field, path)?;
        validate_type(
            item,
            field_value,
            &child_path(path, field),
            options,
            context,
        )?;
    }
    Ok(())
}

fn validate_record_key(key: TsRecordKey, field: &str, path: &str) -> Result<(), String> {
    match key {
        TsRecordKey::String => Ok(()),
        TsRecordKey::Number => validate_numeric_record_key(field)
            .then_some(())
            .ok_or_else(|| format!("{path}: expected numeric record key, got {field:?}")),
    }
}

fn validate_numeric_record_key(field: &str) -> bool {
    field
        .parse::<f64>()
        .map(|number| number.is_finite())
        .unwrap_or(false)
}

fn validate_union(
    types: &[TsType],
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    if types
        .iter()
        .any(|ty| validate_type(ty, value, path, options, context).is_ok())
    {
        return Ok(());
    }

    Err(format!("{path}: value did not match any union member"))
}

fn validate_optional(
    inner: &TsType,
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    validate_type(inner, value, path, options, context)
}

fn validate_nullable(
    inner: &TsType,
    value: &Value,
    path: &str,
    options: SchemaValidationOptions,
    context: &mut ValidationContext<'_>,
) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    validate_type(inner, value, path, options, context)
}

fn child_path(parent: &str, child: &str) -> String {
    format!("{parent}.{child}")
}

fn index_path(parent: &str, index: usize) -> String {
    format!("{parent}[{index}]")
}

fn expected(path: &str, expected: &str, value: &Value) -> String {
    format!("{path}: expected {expected}, got {}", value_kind(value))
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_nested_object_shape() {
        let schema = Schema::typed(
            "Input",
            TsType::Object(vec![
                TsField::required("id", TsType::Number),
                TsField::optional("tag", TsType::String),
            ]),
        );

        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "id": 1 }),
                SchemaValidationOptions::default(),
            )
            .is_ok()
        );
        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "tag": "x" }),
                SchemaValidationOptions::default(),
            )
            .is_err()
        );
        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "id": "1" }),
                SchemaValidationOptions::default(),
            )
            .is_err()
        );
    }

    #[test]
    fn strict_validation_rejects_unknown_object_fields() {
        let schema = Schema::typed(
            "Input",
            TsType::Object(vec![
                TsField::required("id", TsType::Number),
                TsField::required(
                    "profile",
                    TsType::Object(vec![TsField::required("name", TsType::String)]),
                ),
            ]),
        );
        let options = SchemaValidationOptions {
            reject_unknown_fields: true,
        };

        let error = validate_schema_with_options(
            &schema,
            &json!({
                "id": 1,
                "profile": {
                    "name": "Ada",
                    "role": "admin"
                }
            }),
            options,
        )
        .expect_err("unknown nested field should fail");

        assert_eq!(error, "$.profile.role: unknown field");
    }

    #[test]
    fn strict_validation_allows_tagged_enum_discriminant() {
        let schema = Schema::typed(
            "Event",
            TsType::Enum {
                tag: Some(String::from("type")),
                variants: vec![TsEnumVariant::payload(
                    "damage",
                    vec![TsField::required("amount", TsType::Number)],
                )],
            },
        );
        let options = SchemaValidationOptions {
            reject_unknown_fields: true,
        };

        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "type": "damage", "amount": 12 }),
                options,
            )
            .is_ok()
        );
        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "type": "damage", "amount": 12, "debug": true }),
                options,
            )
            .is_err()
        );
    }

    #[test]
    fn validates_union_and_nullable_shapes() {
        let schema = Schema::typed(
            "Input",
            TsType::Union(vec![
                TsType::String,
                TsType::Nullable(Box::new(TsType::Number)),
            ]),
        );

        assert!(
            validate_schema_with_options(&schema, &json!("ok"), SchemaValidationOptions::default())
                .is_ok()
        );
        assert!(
            validate_schema_with_options(&schema, &json!(42), SchemaValidationOptions::default())
                .is_ok()
        );
        assert!(
            validate_schema_with_options(&schema, &Value::Null, SchemaValidationOptions::default())
                .is_ok()
        );
        assert!(
            validate_schema_with_options(&schema, &json!(true), SchemaValidationOptions::default())
                .is_err()
        );
    }

    #[test]
    fn numeric_record_key_validation_rejects_non_finite_numbers() {
        let schema = Schema::typed(
            "Scores",
            TsType::Record {
                key: TsRecordKey::Number,
                value: Box::new(TsType::String),
            },
        );

        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "42": "ok", "-1.5": "ok", "1e3": "ok" }),
                SchemaValidationOptions::default(),
            )
            .is_ok()
        );

        let error = validate_schema_with_options(
            &schema,
            &json!({ "NaN": "bad" }),
            SchemaValidationOptions::default(),
        )
        .expect_err("non-finite numeric record key should fail");

        assert_eq!(error, "$: expected numeric record key, got \"NaN\"");
    }

    #[test]
    fn validates_resolved_type_ref_dependencies() {
        let schema = Schema::typed(
            "Input",
            TsType::Object(vec![TsField::required(
                "user_id",
                TsType::TypeRef(String::from("UserId")),
            )]),
        )
        .with_dependencies(vec![Schema::typed("UserId", TsType::Number)]);

        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "user_id": 42 }),
                SchemaValidationOptions::default(),
            )
            .is_ok()
        );

        let error = validate_schema_with_options(
            &schema,
            &json!({ "user_id": "42" }),
            SchemaValidationOptions::default(),
        )
        .expect_err("resolved TypeRef should validate against its dependency");

        assert_eq!(error, "$.user_id: expected number, got string");
    }

    #[test]
    fn unresolved_type_ref_remains_permissive_for_external_aliases() {
        let schema = Schema::typed("Input", TsType::TypeRef(String::from("ExternalAlias")));

        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "anything": true }),
                SchemaValidationOptions::default(),
            )
            .is_ok()
        );
    }

    #[test]
    fn cyclic_type_ref_dependencies_do_not_recurse_forever() {
        let schema =
            Schema::typed("Input", TsType::TypeRef(String::from("Alias"))).with_dependencies(vec![
                Schema::typed("Alias", TsType::TypeRef(String::from("Alias"))),
            ]);

        assert!(
            validate_schema_with_options(
                &schema,
                &json!("anything"),
                SchemaValidationOptions::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn recursive_object_type_ref_dependencies_do_not_recurse_forever() {
        let schema = Schema::typed("Input", TsType::TypeRef(String::from("Node")))
            .with_dependencies(vec![Schema::typed(
                "Node",
                TsType::Object(vec![TsField::optional(
                    "child",
                    TsType::TypeRef(String::from("Node")),
                )]),
            )]);

        assert!(
            validate_schema_with_options(
                &schema,
                &json!({ "child": { "child": "not validated recursively" } }),
                SchemaValidationOptions::default(),
            )
            .is_ok()
        );
    }
}
