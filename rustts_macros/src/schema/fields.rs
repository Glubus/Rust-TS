//! Schema of struct-like shapes: which fields appear, under which key, and whether the
//! TypeScript field is optional.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;

use crate::model::{Container, Field, Missing, Shape, Style};

/// Whether a field appears in the single schema shared by input and output.
#[derive(PartialEq, Eq)]
enum Presence {
    Omitted,
    Optional,
    Required,
}

/// TypeScript type of a shape: `null`, the inner type, a tuple or an object.
pub(super) fn shape_type(
    container: &Container<'_>,
    shape: &Shape<'_>,
    container_default: bool,
) -> TokenStream {
    match shape.style {
        Style::Unit => quote!(::rustts::TsType::Null),
        Style::Newtype => type_ref(&shape.fields[0]),
        Style::Tuple => {
            let items = shape
                .fields
                .iter()
                .filter(|field| presence(container, field, false) != Presence::Omitted)
                .map(type_ref);
            quote!(::rustts::TsType::Tuple(::std::vec![#(#items),*]))
        }
        Style::Struct => {
            let fields = object_fields(container, shape, container_default);
            quote!(::rustts::TsType::Object(#fields))
        }
    }
}

/// `Vec<TsField>` expression for a struct-like shape, flattened fields included.
pub(super) fn object_fields(
    container: &Container<'_>,
    shape: &Shape<'_>,
    container_default: bool,
) -> TokenStream {
    let steps: Vec<TokenStream> = shape
        .fields
        .iter()
        .filter_map(|field| field_step(container, field, container_default))
        .collect();
    if steps.is_empty() {
        return quote!(::std::vec::Vec::new());
    }
    quote!({
        let mut fields = ::std::vec::Vec::new();
        #(#steps)*
        fields
    })
}

/// Statements appending the object fields of `ty`'s schema to `fields`, as serde merges
/// a flattened struct or an internally tagged newtype payload.
pub(super) fn merge_object_schema(ty: &Type, all_optional: bool, requirement: &str) -> TokenStream {
    let mark_optional =
        all_optional.then(|| quote!(let field = ::rustts::TsField { optional: true, ..field };));
    quote! {
        match <#ty as ::rustts::TsSchema>::ts_type() {
            ::rustts::TsType::Object(merged) => {
                for field in merged {
                    #mark_optional
                    if fields.iter().any(|existing: &::rustts::TsField| existing.name == field.name) {
                        panic!("serde flatten produced duplicate TypeScript field `{}`", field.name);
                    }
                    fields.push(field);
                }
            }
            ::rustts::TsType::Null | ::rustts::TsType::Void => {}
            _ => panic!(#requirement),
        }
    }
}

/// Type of a field: its `#[rustts(type)]` text or its `TsSchema` reference.
pub(super) fn type_ref(field: &Field<'_>) -> TokenStream {
    match &field.attrs.ts_type {
        Some(text) => quote!(::rustts::TsType::TypeRef(::std::string::String::from(#text))),
        None => {
            let ty = field.ty;
            quote!(::rustts::schema_type_ref::<#ty>())
        }
    }
}

/// Dependency push for a field whose type comes from `TsSchema`.
pub(super) fn dependency(field: &Field<'_>) -> Option<TokenStream> {
    let ty = field.ty;
    field
        .attrs
        .ts_type
        .is_none()
        .then(|| quote!(::rustts::push_schema_dependency::<#ty>(&mut dependencies);))
}

pub(super) fn shape_dependencies(
    container: &Container<'_>,
    shape: &Shape<'_>,
    container_default: bool,
) -> Vec<TokenStream> {
    shape
        .fields
        .iter()
        .filter(|field| presence(container, field, container_default) != Presence::Omitted)
        .filter_map(dependency)
        .collect()
}

fn field_step(
    container: &Container<'_>,
    field: &Field<'_>,
    container_default: bool,
) -> Option<TokenStream> {
    let presence = presence(container, field, container_default);
    if presence == Presence::Omitted {
        return None;
    }
    if field.attrs.flatten {
        let (ty, all_optional) = match field.option_inner() {
            Some(inner) => (inner, true),
            None => (field.ty, false),
        };
        return Some(merge_object_schema(
            ty,
            all_optional,
            "serde flatten requires a TsSchema object type",
        ));
    }
    let key = field
        .attrs
        .schema_rename
        .as_deref()
        .unwrap_or_else(|| container.schema_key(&field.name));
    let ty = type_ref(field);
    Some(match presence {
        Presence::Required => quote!(fields.push(::rustts::TsField::required(#key, #ty));),
        _ => quote!(fields.push(::rustts::TsField::optional(#key, #ty));),
    })
}

/// Combines the field's presence in each described direction: omitted when no
/// direction carries it, required when every direction always does.
fn presence(container: &Container<'_>, field: &Field<'_>, container_default: bool) -> Presence {
    let directions = container.schema_directions();
    let described = [
        directions.encode.then(|| output_presence(field)),
        directions
            .decode
            .then(|| input_presence(field, container_default)),
    ];
    let mut described = described.iter().flatten();
    if described.clone().all(Option::is_none) {
        Presence::Omitted
    } else if described.all(|always| *always == Some(true)) {
        Presence::Required
    } else {
        Presence::Optional
    }
}

/// `None` when serialization never writes the field, else whether it always does.
fn output_presence(field: &Field<'_>) -> Option<bool> {
    let attrs = &field.attrs;
    if attrs.skip_serializing {
        return None;
    }
    Some(attrs.skip_serializing_if.is_none() && !is_optional_type(field))
}

/// `None` when deserialization ignores the field, else whether input must provide it.
fn input_presence(field: &Field<'_>, container_default: bool) -> Option<bool> {
    if field.attrs.skip_deserializing {
        return None;
    }
    let defaulted = !matches!(field.missing(container_default), Missing::Required);
    Some(!defaulted && !is_optional_type(field))
}

/// `Option<T>` and `#[rustts(optional)]` fields are optional in the schema.
fn is_optional_type(field: &Field<'_>) -> bool {
    field.attrs.schema_optional || field.option_inner().is_some()
}
