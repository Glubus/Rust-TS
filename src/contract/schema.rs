//! Rust type to TypeScript schema bridge.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::PathBuf,
};

use rquickjs::{Ctx, Result as JsResult, Value as JsValue};
use serde::{Serialize, de::DeserializeOwned};

use super::bridge::{js_value_to_json, json_to_js_value};
use super::{Schema, TsRecordKey, TsType};

thread_local! {
    static SCHEMA_DEPENDENCY_STACK: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// Contract schema emitted by a Rust type.
///
/// Future derive/proc-macro support should target this trait. Runtime host contracts
/// can then use `T::schema()` instead of hand-building [`Schema`] values.
pub trait TsSchema {
    /// Stable TypeScript schema name.
    ///
    /// Primitive and container implementations intentionally use `unknown` so the
    /// declaration renderer emits the inline type shape instead of a top-level alias.
    fn schema_name() -> &'static str {
        "unknown"
    }

    /// TypeScript type shape represented by this Rust type.
    fn ts_type() -> TsType;

    /// Named schemas referenced by this Rust type.
    fn schema_dependencies() -> Vec<Schema> {
        Vec::new()
    }

    /// Full schema metadata for this Rust type.
    fn schema() -> Schema {
        Schema::typed(Self::schema_name(), Self::ts_type())
            .with_dependencies(Self::schema_dependencies())
    }

    /// Validates one JSON value against this type's generated schema.
    ///
    /// Unknown object fields are allowed, matching the default host bridge
    /// validation policy.
    ///
    /// # Errors
    ///
    /// Returns a human-readable validation path and reason when the value does
    /// not match this schema.
    fn validate_json(value: &serde_json::Value) -> Result<(), String> {
        Self::schema().validate_json(value)
    }

    /// Validates one JSON value against this type's generated schema and rejects unknown fields.
    ///
    /// # Errors
    ///
    /// Returns a human-readable validation path and reason when the value does
    /// not match this schema or contains undeclared object fields.
    fn validate_json_strict(value: &serde_json::Value) -> Result<(), String> {
        Self::schema().validate_json_strict(value)
    }

    /// Converts a JavaScript value into this type for typed host bridge fast paths.
    #[doc(hidden)]
    fn __rustts_from_js_value<'js>(ctx: &Ctx<'js>, value: JsValue<'js>) -> JsResult<Self>
    where
        Self: Sized + DeserializeOwned,
    {
        let value = js_value_to_json(ctx, value)?;
        serde_json::from_value(value).map_err(|error| {
            rquickjs::Error::new_from_js_message("json", "rust", error.to_string())
        })
    }

    /// Converts this value into JavaScript for typed host bridge fast paths.
    #[doc(hidden)]
    fn __rustts_into_js_value<'js>(self, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>>
    where
        Self: Sized + Serialize,
    {
        let value = serde_json::to_value(self).map_err(|error| {
            rquickjs::Error::new_from_js_message("rust", "json", error.to_string())
        })?;
        json_to_js_value(ctx, value)
    }
}

/// Pushes the named schema for `T`, plus its own dependencies, when `T` has a stable name.
///
/// This helper is public for derive macro expansion but is not part of the intended
/// high-level embedding API.
#[doc(hidden)]
pub fn push_schema_dependency<T>(dependencies: &mut Vec<Schema>)
where
    T: TsSchema,
{
    let _guard = match SchemaDependencyGuard::enter::<T>() {
        DependencyGuardState::Entered(guard) => Some(guard),
        DependencyGuardState::Unnamed => None,
        DependencyGuardState::Recursive => return,
    };

    dependencies.extend(T::schema_dependencies());

    let schema = T::schema();
    if has_declaration_name(&schema) {
        dependencies.push(schema);
    }
}

enum DependencyGuardState {
    Entered(SchemaDependencyGuard),
    Unnamed,
    Recursive,
}

struct SchemaDependencyGuard {
    name: &'static str,
}

impl SchemaDependencyGuard {
    fn enter<T>() -> DependencyGuardState
    where
        T: TsSchema,
    {
        let name = T::schema_name();
        if !has_declaration_name_str(name) {
            return DependencyGuardState::Unnamed;
        }

        let entered = SCHEMA_DEPENDENCY_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.contains(&name) {
                return false;
            }
            stack.push(name);
            true
        });

        if entered {
            DependencyGuardState::Entered(Self { name })
        } else {
            DependencyGuardState::Recursive
        }
    }
}

impl Drop for SchemaDependencyGuard {
    fn drop(&mut self) {
        SCHEMA_DEPENDENCY_STACK.with(|stack| {
            let popped = stack.borrow_mut().pop();
            debug_assert_eq!(popped, Some(self.name));
        });
    }
}

/// Returns a TypeScript type reference when `T` has a stable schema name, otherwise its inline type.
///
/// This helper is public for derive macro expansion but is not part of the intended
/// high-level embedding API.
#[doc(hidden)]
pub fn schema_type_ref<T>() -> TsType
where
    T: TsSchema,
{
    let name = T::schema_name();
    if has_declaration_name_str(name) {
        TsType::TypeRef(name.to_owned())
    } else {
        T::ts_type()
    }
}

fn has_declaration_name(schema: &Schema) -> bool {
    has_declaration_name_str(&schema.name)
}

fn has_declaration_name_str(name: &str) -> bool {
    !name.trim().is_empty() && name != "unknown"
}

impl TsSchema for () {
    fn ts_type() -> TsType {
        TsType::Void
    }
}

impl TsSchema for bool {
    fn ts_type() -> TsType {
        TsType::Boolean
    }
}

impl TsSchema for String {
    fn ts_type() -> TsType {
        TsType::String
    }
}

impl TsSchema for PathBuf {
    fn ts_type() -> TsType {
        TsType::String
    }
}

impl TsSchema for IpAddr {
    fn ts_type() -> TsType {
        TsType::String
    }
}

impl TsSchema for Ipv4Addr {
    fn ts_type() -> TsType {
        TsType::String
    }
}

impl TsSchema for Ipv6Addr {
    fn ts_type() -> TsType {
        TsType::String
    }
}

impl TsSchema for serde_json::Value {
    fn ts_type() -> TsType {
        TsType::Json
    }
}

impl<T> TsSchema for Option<T>
where
    T: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Nullable(Box::new(schema_type_ref::<T>()))
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<T>(&mut dependencies);
        dependencies
    }
}

impl<T> TsSchema for Vec<T>
where
    T: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Array(Box::new(schema_type_ref::<T>()))
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<T>(&mut dependencies);
        dependencies
    }
}

impl<T> TsSchema for HashSet<T>
where
    T: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Array(Box::new(schema_type_ref::<T>()))
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<T>(&mut dependencies);
        dependencies
    }
}

impl<T> TsSchema for BTreeSet<T>
where
    T: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Array(Box::new(schema_type_ref::<T>()))
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<T>(&mut dependencies);
        dependencies
    }
}

impl<T, const N: usize> TsSchema for [T; N]
where
    T: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Array(Box::new(schema_type_ref::<T>()))
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<T>(&mut dependencies);
        dependencies
    }
}

impl<T> TsSchema for Box<T>
where
    T: TsSchema,
{
    fn schema_name() -> &'static str {
        T::schema_name()
    }

    fn ts_type() -> TsType {
        T::ts_type()
    }

    fn schema_dependencies() -> Vec<Schema> {
        T::schema_dependencies()
    }
}

impl<T> TsSchema for HashMap<String, T>
where
    T: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Record {
            key: TsRecordKey::String,
            value: Box::new(schema_type_ref::<T>()),
        }
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<T>(&mut dependencies);
        dependencies
    }
}

macro_rules! impl_numeric_map_schema {
    ($($key:ty),+ $(,)?) => {
        $(
            impl<T> TsSchema for HashMap<$key, T>
            where
                T: TsSchema,
            {
                fn ts_type() -> TsType {
                    TsType::Record {
                        key: TsRecordKey::Number,
                        value: Box::new(schema_type_ref::<T>()),
                    }
                }

                fn schema_dependencies() -> Vec<Schema> {
                    let mut dependencies = Vec::new();
                    push_schema_dependency::<T>(&mut dependencies);
                    dependencies
                }
            }

            impl<T> TsSchema for BTreeMap<$key, T>
            where
                T: TsSchema,
            {
                fn ts_type() -> TsType {
                    TsType::Record {
                        key: TsRecordKey::Number,
                        value: Box::new(schema_type_ref::<T>()),
                    }
                }

                fn schema_dependencies() -> Vec<Schema> {
                    let mut dependencies = Vec::new();
                    push_schema_dependency::<T>(&mut dependencies);
                    dependencies
                }
            }
        )+
    };
}

impl_numeric_map_schema!(u8, u16, u32, u64, u128, usize);
impl_numeric_map_schema!(i8, i16, i32, i64, i128, isize);

impl<T> TsSchema for BTreeMap<String, T>
where
    T: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Record {
            key: TsRecordKey::String,
            value: Box::new(schema_type_ref::<T>()),
        }
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<T>(&mut dependencies);
        dependencies
    }
}

impl<A, B> TsSchema for (A, B)
where
    A: TsSchema,
    B: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Tuple(vec![schema_type_ref::<A>(), schema_type_ref::<B>()])
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<A>(&mut dependencies);
        push_schema_dependency::<B>(&mut dependencies);
        dependencies
    }
}

impl<A, B, C> TsSchema for (A, B, C)
where
    A: TsSchema,
    B: TsSchema,
    C: TsSchema,
{
    fn ts_type() -> TsType {
        TsType::Tuple(vec![
            schema_type_ref::<A>(),
            schema_type_ref::<B>(),
            schema_type_ref::<C>(),
        ])
    }

    fn schema_dependencies() -> Vec<Schema> {
        let mut dependencies = Vec::new();
        push_schema_dependency::<A>(&mut dependencies);
        push_schema_dependency::<B>(&mut dependencies);
        push_schema_dependency::<C>(&mut dependencies);
        dependencies
    }
}

macro_rules! impl_number_schema {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TsSchema for $ty {
                fn ts_type() -> TsType {
                    TsType::Number
                }
            }
        )+
    };
}

impl_number_schema!(u8, u16, u32, u64, u128, usize);
impl_number_schema!(i8, i16, i32, i64, i128, isize);
impl_number_schema!(f32, f64);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::TsField;
    use serde_json::json;

    struct NamedDependencyBomb;

    impl TsSchema for NamedDependencyBomb {
        fn schema_name() -> &'static str {
            "NamedDependencyBomb"
        }

        fn ts_type() -> TsType {
            TsType::Number
        }

        fn schema_dependencies() -> Vec<Schema> {
            panic!("schema_type_ref must not materialize dependencies")
        }
    }

    #[test]
    fn option_schema_is_nullable_value_not_optional_field() {
        assert_eq!(
            Option::<String>::ts_type(),
            TsType::Nullable(Box::new(TsType::String))
        );
    }

    #[test]
    fn collection_schemas_emit_arrays_records_and_tuples() {
        assert_eq!(
            Vec::<u64>::ts_type(),
            TsType::Array(Box::new(TsType::Number))
        );
        assert_eq!(
            HashSet::<String>::ts_type(),
            TsType::Array(Box::new(TsType::String))
        );
        assert_eq!(
            BTreeSet::<bool>::ts_type(),
            TsType::Array(Box::new(TsType::Boolean))
        );
        assert_eq!(
            <[u8; 16]>::ts_type(),
            TsType::Array(Box::new(TsType::Number))
        );
        assert_eq!(
            HashMap::<String, bool>::ts_type(),
            TsType::Record {
                key: TsRecordKey::String,
                value: Box::new(TsType::Boolean),
            }
        );
        assert_eq!(
            HashMap::<u64, String>::ts_type(),
            TsType::Record {
                key: TsRecordKey::Number,
                value: Box::new(TsType::String),
            }
        );
        assert_eq!(
            BTreeMap::<i32, bool>::ts_type(),
            TsType::Record {
                key: TsRecordKey::Number,
                value: Box::new(TsType::Boolean),
            }
        );
        assert_eq!(
            <(String, u32, bool)>::ts_type(),
            TsType::Tuple(vec![TsType::String, TsType::Number, TsType::Boolean])
        );
    }

    #[test]
    fn std_string_like_schemas_emit_string() {
        assert_eq!(PathBuf::ts_type(), TsType::String);
        assert_eq!(IpAddr::ts_type(), TsType::String);
        assert_eq!(Ipv4Addr::ts_type(), TsType::String);
        assert_eq!(Ipv6Addr::ts_type(), TsType::String);
    }

    #[test]
    fn schema_type_ref_uses_name_without_materializing_dependencies() {
        assert_eq!(
            schema_type_ref::<NamedDependencyBomb>(),
            TsType::TypeRef(String::from("NamedDependencyBomb"))
        );
    }

    #[test]
    fn schema_validation_helpers_validate_json_values() {
        let schema = Schema::typed(
            "DemoInput",
            TsType::Object(vec![TsField::required("id", TsType::Number)]),
        );

        schema
            .validate_json(&json!({ "id": 1, "extra": true }))
            .expect("unknown fields are allowed by default");
        schema
            .validate_json_strict(&json!({ "id": 1 }))
            .expect("declared fields pass strict validation");

        let unknown_error = schema
            .validate_json_strict(&json!({ "id": 1, "extra": true }))
            .expect_err("strict validation rejects unknown fields");
        let type_error = schema
            .validate_json(&json!({ "id": "bad" }))
            .expect_err("type validation rejects mismatches");

        assert!(unknown_error.contains("$.extra: unknown field"));
        assert!(type_error.contains("$.id: expected number, got string"));
    }
}
