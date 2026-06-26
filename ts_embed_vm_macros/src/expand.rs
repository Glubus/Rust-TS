use quote::quote;
use syn::{
    Data, DeriveInput, Error, Fields, GenericParam, PathArguments, Result, Token, Type,
    parse_quote, punctuated::Punctuated,
};

use crate::attrs::{ContainerAttrs, FieldAttrs, VariantAttrs};
use crate::rename::{RenameRule, rename_field, rename_variant};

pub(super) fn expand_ts_schema(input: &DeriveInput) -> Result<proc_macro2::TokenStream> {
    let ident = &input.ident;
    let generics = add_ts_schema_bounds(input.generics.clone());
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let attrs = ContainerAttrs::from_input(input)?;
    let schema_name = attrs.schema_name.unwrap_or_else(|| input.ident.to_string());
    let (ts_type, dependencies) = match &input.data {
        Data::Struct(data) => {
            let ts_type = expand_struct_type(&data.fields, attrs.rename_all, attrs.transparent)?;
            let dependencies = expand_struct_dependencies(&data.fields, attrs.transparent)?;
            (ts_type, dependencies)
        }
        Data::Enum(data) => {
            if attrs.transparent {
                return Err(Error::new_spanned(
                    input,
                    "serde transparent is only supported for structs",
                ));
            }
            let ts_type = expand_enum_type(data, attrs.rename_all, attrs.untagged)?;
            let dependencies = expand_enum_dependencies(data)?;
            (ts_type, dependencies)
        }
        Data::Union(_) => {
            return Err(Error::new_spanned(
                input,
                "TsSchema cannot be derived for unions",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics ::ts_embed_vm::TsSchema for #ident #ty_generics #where_clause {
            fn schema_name() -> &'static str {
                #schema_name
            }

            fn ts_type() -> ::ts_embed_vm::TsType {
                #ts_type
            }

            fn schema_dependencies() -> Vec<::ts_embed_vm::Schema> {
                let mut dependencies = Vec::new();
                #(#dependencies)*
                dependencies
            }
        }
    })
}

fn add_ts_schema_bounds(mut generics: syn::Generics) -> syn::Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(type_param) = param {
            type_param
                .bounds
                .push(parse_quote!(::ts_embed_vm::TsSchema));
        }
    }
    generics
}

fn expand_struct_type(
    fields: &Fields,
    rename_all: Option<RenameRule>,
    transparent: bool,
) -> Result<proc_macro2::TokenStream> {
    if transparent {
        return expand_transparent_struct_type(fields);
    }

    match fields {
        Fields::Named(fields) => expand_named_fields_object(&fields.named, rename_all),
        Fields::Unnamed(fields) => expand_tuple_fields(&fields.unnamed),
        Fields::Unit => Ok(quote! {
            ::ts_embed_vm::TsType::Object(Vec::new())
        }),
    }
}

fn expand_struct_dependencies(
    fields: &Fields,
    transparent: bool,
) -> Result<Vec<proc_macro2::TokenStream>> {
    if transparent {
        let field = transparent_struct_field(fields)?;
        return Ok(vec![expand_field_dependency(field)?]);
    }

    expand_fields_dependencies(fields)
}

fn expand_transparent_struct_type(fields: &Fields) -> Result<proc_macro2::TokenStream> {
    let field = transparent_struct_field(fields)?;
    let ty = &field.ty;

    Ok(quote! {
        ::ts_embed_vm::schema_type_ref::<#ty>()
    })
}

fn transparent_struct_field(fields: &Fields) -> Result<&syn::Field> {
    match fields {
        Fields::Named(fields) if fields.named.len() == 1 => fields.named.first().ok_or_else(|| {
            Error::new_spanned(fields, "serde transparent struct must contain one field")
        }),
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            fields.unnamed.first().ok_or_else(|| {
                Error::new_spanned(fields, "serde transparent struct must contain one field")
            })
        }
        _ => Err(Error::new_spanned(
            fields,
            "serde transparent struct must contain exactly one field",
        )),
    }
}

fn expand_fields_dependencies(fields: &Fields) -> Result<Vec<proc_macro2::TokenStream>> {
    match fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .filter_map(|field| expand_dependency_if_not_skipped(field).transpose())
            .collect(),
        Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .filter_map(|field| expand_dependency_if_not_skipped(field).transpose())
            .collect(),
        Fields::Unit => Ok(Vec::new()),
    }
}

fn expand_dependency_if_not_skipped(
    field: &syn::Field,
) -> Result<Option<proc_macro2::TokenStream>> {
    let attrs = FieldAttrs::from_field(field)?;
    if attrs.skip {
        return Ok(None);
    }

    Ok(Some(expand_field_dependency(field)?))
}

fn expand_field_dependency(field: &syn::Field) -> Result<proc_macro2::TokenStream> {
    let ty = &field.ty;
    Ok(quote! {
        ::ts_embed_vm::push_schema_dependency::<#ty>(&mut dependencies);
    })
}

fn expand_named_fields_object(
    fields: &Punctuated<syn::Field, Token![,]>,
    rename_all: Option<RenameRule>,
) -> Result<proc_macro2::TokenStream> {
    let fields = fields
        .iter()
        .filter_map(|field| expand_named_struct_field(field, rename_all).transpose())
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        ::ts_embed_vm::TsType::Object(vec![#(#fields),*])
    })
}

fn expand_tuple_fields(
    fields: &Punctuated<syn::Field, Token![,]>,
) -> Result<proc_macro2::TokenStream> {
    let items = fields
        .iter()
        .map(|field| {
            let ty = &field.ty;
            Ok(quote! {
                ::ts_embed_vm::schema_type_ref::<#ty>()
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        ::ts_embed_vm::TsType::Tuple(vec![#(#items),*])
    })
}

fn expand_named_struct_field(
    field: &syn::Field,
    rename_all: Option<RenameRule>,
) -> Result<Option<proc_macro2::TokenStream>> {
    let ident = field
        .ident
        .as_ref()
        .ok_or_else(|| Error::new_spanned(field, "TsSchema requires named fields"))?;
    let attrs = FieldAttrs::from_field(field)?;
    if attrs.skip {
        return Ok(None);
    }

    let field_name = attrs
        .rename
        .unwrap_or_else(|| rename_field(ident, rename_all));
    let ty = &field.ty;

    if attrs.optional || is_option_type(ty) {
        Ok(Some(quote! {
            ::ts_embed_vm::TsField::optional(
                #field_name,
                ::ts_embed_vm::schema_type_ref::<#ty>(),
            )
        }))
    } else {
        Ok(Some(quote! {
            ::ts_embed_vm::TsField::required(
                #field_name,
                ::ts_embed_vm::schema_type_ref::<#ty>(),
            )
        }))
    }
}

fn is_option_type(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    type_path.path.segments.last().is_some_and(|segment| {
        segment.ident == "Option" && matches!(segment.arguments, PathArguments::AngleBracketed(_))
    })
}

fn expand_enum_type(
    data: &syn::DataEnum,
    rename_all: Option<RenameRule>,
    untagged: bool,
) -> Result<proc_macro2::TokenStream> {
    if untagged {
        return expand_untagged_enum_type(data);
    }

    let variants = data
        .variants
        .iter()
        .map(|variant| expand_enum_variant(variant, rename_all))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        ::ts_embed_vm::TsType::Enum {
            tag: Some(String::from("type")),
            variants: vec![#(#variants),*],
        }
    })
}

fn expand_enum_dependencies(data: &syn::DataEnum) -> Result<Vec<proc_macro2::TokenStream>> {
    data.variants
        .iter()
        .map(|variant| expand_fields_dependencies(&variant.fields))
        .collect::<Result<Vec<_>>>()
        .map(|dependencies| dependencies.into_iter().flatten().collect())
}

fn expand_untagged_enum_type(data: &syn::DataEnum) -> Result<proc_macro2::TokenStream> {
    let variants = data
        .variants
        .iter()
        .map(expand_untagged_enum_variant)
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        ::ts_embed_vm::TsType::Union(vec![#(#variants),*])
    })
}

fn expand_untagged_enum_variant(variant: &syn::Variant) -> Result<proc_macro2::TokenStream> {
    match &variant.fields {
        Fields::Unit => Err(Error::new_spanned(
            variant,
            "serde untagged unit enum variants are not supported",
        )),
        Fields::Named(fields) => expand_named_fields_object(&fields.named, None),
        Fields::Unnamed(fields) => expand_tuple_payload_type(&fields.unnamed),
    }
}

fn expand_enum_variant(
    variant: &syn::Variant,
    rename_all: Option<RenameRule>,
) -> Result<proc_macro2::TokenStream> {
    let attrs = VariantAttrs::from_variant(variant)?;
    let name = attrs
        .rename
        .unwrap_or_else(|| rename_variant(&variant.ident, rename_all));
    match &variant.fields {
        Fields::Unit => Ok(quote! {
            ::ts_embed_vm::TsEnumVariant::unit(#name)
        }),
        Fields::Named(fields) => {
            let fields = fields
                .named
                .iter()
                .filter_map(|field| expand_named_struct_field(field, None).transpose())
                .collect::<Result<Vec<_>>>()?;
            Ok(quote! {
                ::ts_embed_vm::TsEnumVariant::payload(#name, vec![#(#fields),*])
            })
        }
        Fields::Unnamed(fields) => {
            let field = tuple_variant_payload_field(&fields.unnamed)?;
            Ok(quote! {
                ::ts_embed_vm::TsEnumVariant::payload(#name, vec![#field])
            })
        }
    }
}

fn tuple_variant_payload_field(
    fields: &Punctuated<syn::Field, Token![,]>,
) -> Result<proc_macro2::TokenStream> {
    if fields.is_empty() {
        return Err(Error::new_spanned(
            fields,
            "tuple enum variant must contain at least one field",
        ));
    }

    let field_name = if fields.len() == 1 { "value" } else { "items" };
    let ty = expand_tuple_payload_type(fields)?;
    Ok(quote! {
        ::ts_embed_vm::TsField::required(#field_name, #ty)
    })
}

fn expand_tuple_payload_type(
    fields: &Punctuated<syn::Field, Token![,]>,
) -> Result<proc_macro2::TokenStream> {
    if fields.len() == 1 {
        let ty = &fields.first().expect("checked len").ty;
        return Ok(quote! {
            ::ts_embed_vm::schema_type_ref::<#ty>()
        });
    }

    let items = fields
        .iter()
        .map(|field| {
            let ty = &field.ty;
            Ok(quote! {
                ::ts_embed_vm::schema_type_ref::<#ty>()
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        ::ts_embed_vm::TsType::Tuple(vec![#(#items),*])
    })
}
