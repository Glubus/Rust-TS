//! Object schemas assembled field by field, `#[serde(flatten)]` fields included.

use crate::contract::{TsEnumVariant, TsField, TsRecordKey, TsType};

/// Object schema assembled by `#[derive(TsSchema)]`: declared fields in order, plus the
/// value type of undeclared keys once a flattened map opens the object.
///
/// This helper is public for derive macro expansion but is not part of the intended
/// high-level embedding API.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct ObjectSchema {
    fields: Vec<TsField>,
    rest: Option<TsType>,
}

impl ObjectSchema {
    /// Creates an empty, closed object schema.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a declared field.
    pub fn field(&mut self, field: TsField) {
        self.fields.push(field);
    }

    /// Merges `ty`, the schema of a flattened value, as serde merges it into the
    /// surrounding object: object fields join the declared fields (optional when
    /// `all_optional`), a string-keyed map opens the object, `null` adds nothing.
    ///
    /// # Panics
    ///
    /// Panics with `requirement` when `ty` is neither an object nor a map, and when the
    /// merge cannot be described (duplicate field, numeric map keys, two maps with
    /// different value types).
    pub fn flatten(&mut self, ty: TsType, all_optional: bool, requirement: &str) {
        match ty {
            TsType::Object(fields) => self.merge_fields(fields, all_optional),
            TsType::OpenObject { fields, rest } => {
                self.merge_fields(fields, all_optional);
                self.merge_rest(*rest);
            }
            TsType::Record {
                key: TsRecordKey::String,
                value,
            } => self.merge_rest(*value),
            TsType::Record {
                key: TsRecordKey::Number,
                ..
            } => panic!("serde flatten cannot read numeric map keys; use a string-keyed map"),
            TsType::Json => self.merge_rest(TsType::Json),
            TsType::Null | TsType::Void => {}
            _ => panic!("{requirement}"),
        }
    }

    /// Object type with the collected fields, open when a map was flattened.
    #[must_use]
    pub fn into_type(self) -> TsType {
        TsType::object(self.fields, self.rest)
    }

    /// Enum variant `name` whose payload is the collected object.
    #[must_use]
    pub fn into_variant(self, name: &str) -> TsEnumVariant {
        TsEnumVariant::payload(name, self.fields).with_rest(self.rest)
    }

    fn merge_fields(&mut self, fields: Vec<TsField>, all_optional: bool) {
        for field in fields {
            assert!(
                !self
                    .fields
                    .iter()
                    .any(|existing| existing.name == field.name),
                "serde flatten produced duplicate TypeScript field `{}`",
                field.name
            );
            self.fields.push(TsField {
                optional: field.optional || all_optional,
                ..field
            });
        }
    }

    /// Every flattened map sees all undeclared keys, so several maps must agree.
    fn merge_rest(&mut self, rest: TsType) {
        match &self.rest {
            Some(existing) => assert!(
                *existing == rest,
                "serde flatten of maps with different value types cannot be described"
            ),
            None => self.rest = Some(rest),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_record(value: TsType) -> TsType {
        TsType::Record {
            key: TsRecordKey::String,
            value: Box::new(value),
        }
    }

    #[test]
    fn flattened_map_opens_the_object_after_declared_fields() {
        let mut object = ObjectSchema::new();
        object.field(TsField::required("name", TsType::String));
        object.flatten(string_record(TsType::Number), false, "object or map");

        assert_eq!(
            object.into_type(),
            TsType::OpenObject {
                fields: vec![TsField::required("name", TsType::String)],
                rest: Box::new(TsType::Number),
            }
        );
    }

    #[test]
    fn flattened_open_object_keeps_its_rest_and_fields() {
        let inner = TsType::OpenObject {
            fields: vec![TsField::required("id", TsType::Number)],
            rest: Box::new(TsType::String),
        };
        let mut object = ObjectSchema::new();
        object.flatten(inner, true, "object or map");
        object.flatten(string_record(TsType::String), false, "object or map");

        assert_eq!(
            object.into_variant("Tagged"),
            TsEnumVariant::payload("Tagged", vec![TsField::optional("id", TsType::Number)])
                .with_rest(Some(TsType::String))
        );
    }

    #[test]
    #[should_panic(expected = "maps with different value types")]
    fn flattened_maps_with_different_values_are_rejected() {
        let mut object = ObjectSchema::new();
        object.flatten(string_record(TsType::String), false, "object or map");
        object.flatten(string_record(TsType::Number), false, "object or map");
    }

    #[test]
    #[should_panic(expected = "numeric map keys")]
    fn flattened_numeric_keyed_map_is_rejected() {
        let mut object = ObjectSchema::new();
        object.flatten(
            TsType::Record {
                key: TsRecordKey::Number,
                value: Box::new(TsType::String),
            },
            false,
            "object or map",
        );
    }
}
