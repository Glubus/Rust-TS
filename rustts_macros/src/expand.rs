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
    let bridge_directions = BridgeDirections {
        from_js: has_derive(input, "Deserialize"),
        into_js: has_derive(input, "Serialize"),
    };
    let schema_name = attrs.schema_name.unwrap_or_else(|| input.ident.to_string());
    let (ts_type, dependencies, bridge_methods) = match &input.data {
        Data::Struct(data) => {
            let ts_type = expand_struct_type(&data.fields, attrs.rename_all, attrs.transparent)?;
            let dependencies = expand_struct_dependencies(&data.fields, attrs.transparent)?;
            let bridge_methods = expand_struct_bridge_methods(
                &data.fields,
                attrs.rename_all,
                attrs.transparent,
                bridge_directions,
            )?;
            (ts_type, dependencies, bridge_methods)
        }
        Data::Enum(data) => {
            if attrs.transparent {
                return Err(Error::new_spanned(
                    input,
                    "serde transparent is only supported for structs",
                ));
            }
            let ts_type = expand_enum_type(
                data,
                attrs.rename_all,
                attrs.enum_tag.as_deref(),
                attrs.enum_content.as_deref(),
                attrs.untagged,
            )?;
            let dependencies = expand_enum_dependencies(data)?;
            (ts_type, dependencies, Vec::new())
        }
        Data::Union(_) => {
            return Err(Error::new_spanned(
                input,
                "TsSchema cannot be derived for unions",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics ::rustts::TsSchema for #ident #ty_generics #where_clause {
            fn schema_name() -> &'static str {
                #schema_name
            }

            fn ts_type() -> ::rustts::TsType {
                #ts_type
            }

            fn schema_dependencies() -> Vec<::rustts::Schema> {
                let mut dependencies = Vec::new();
                #(#dependencies)*
                dependencies
            }

            #(#bridge_methods)*
        }
    })
}

#[derive(Clone, Copy)]
struct BridgeDirections {
    from_js: bool,
    into_js: bool,
}

fn has_derive(input: &DeriveInput, name: &str) -> bool {
    input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("derive"))
        .any(|attr| {
            attr.parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .map(|paths| {
                    paths.iter().any(|path| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == name)
                    })
                })
                .unwrap_or(false)
        })
}

fn add_ts_schema_bounds(mut generics: syn::Generics) -> syn::Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(type_param) = param {
            type_param.bounds.push(parse_quote!(::rustts::TsSchema));
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
            ::rustts::TsType::Object(Vec::new())
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

fn expand_struct_bridge_methods(
    fields: &Fields,
    rename_all: Option<RenameRule>,
    transparent: bool,
    directions: BridgeDirections,
) -> Result<Vec<proc_macro2::TokenStream>> {
    if !directions.from_js && !directions.into_js {
        return Ok(Vec::new());
    }

    if transparent {
        return expand_transparent_struct_bridge_methods(fields, directions);
    }

    let Fields::Named(fields) = fields else {
        return Ok(Vec::new());
    };

    if fields
        .named
        .iter()
        .map(FieldAttrs::from_field)
        .collect::<Result<Vec<_>>>()?
        .iter()
        .any(|attrs| attrs.skip || attrs.flatten)
    {
        return Ok(Vec::new());
    }

    expand_named_struct_bridge_methods(&fields.named, rename_all, directions)
}

fn expand_transparent_struct_bridge_methods(
    fields: &Fields,
    directions: BridgeDirections,
) -> Result<Vec<proc_macro2::TokenStream>> {
    let field = transparent_struct_field(fields)?;
    let ty = &field.ty;

    let from_body = match &field.ident {
        Some(ident) => quote! {
            Ok(Self {
                #ident: <#ty as ::rustts::TsSchema>::__rustts_from_js_value(ctx, value)?,
            })
        },
        None => quote! {
            Ok(Self(<#ty as ::rustts::TsSchema>::__rustts_from_js_value(ctx, value)?))
        },
    };

    let into_value = match &field.ident {
        Some(ident) => quote! { self.#ident },
        None => quote! { self.0 },
    };

    let from_method = directions.from_js.then(|| {
        quote! {
            fn __rustts_from_js_value<'js>(
                ctx: &::rustts::__rquickjs::Ctx<'js>,
                value: ::rustts::__rquickjs::Value<'js>,
            ) -> ::rustts::__rquickjs::Result<Self>
            where
                Self: Sized + ::rustts::__serde::de::DeserializeOwned,
            {
                #from_body
            }
        }
    });

    let into_method = directions.into_js.then(|| {
        quote! {
            fn __rustts_into_js_value<'js>(
                self,
                ctx: &::rustts::__rquickjs::Ctx<'js>,
            ) -> ::rustts::__rquickjs::Result<::rustts::__rquickjs::Value<'js>>
            where
                Self: Sized + ::rustts::__serde::Serialize,
            {
                <#ty as ::rustts::TsSchema>::__rustts_into_js_value(#into_value, ctx)
            }
        }
    });

    Ok(vec![quote! {
        #from_method
        #into_method
    }])
}

fn expand_named_struct_bridge_methods(
    fields: &Punctuated<syn::Field, Token![,]>,
    rename_all: Option<RenameRule>,
    directions: BridgeDirections,
) -> Result<Vec<proc_macro2::TokenStream>> {
    let mut from_fields = Vec::new();
    let mut into_fields = Vec::new();

    for field in fields {
        let ident = field
            .ident
            .as_ref()
            .ok_or_else(|| Error::new_spanned(field, "TsSchema requires named fields"))?;
        let attrs = FieldAttrs::from_field(field)?;
        let field_name = attrs
            .rename
            .unwrap_or_else(|| rename_field(ident, rename_all));
        let ty = &field.ty;

        from_fields.push(quote! {
            #ident: <#ty as ::rustts::TsSchema>::__rustts_from_js_value(ctx, object.get(#field_name)?)?
        });
        into_fields.push(quote! {
            object.set(#field_name, <#ty as ::rustts::TsSchema>::__rustts_into_js_value(self.#ident, ctx)?)?;
        });
    }

    let from_method = directions.from_js.then(|| quote! {
        fn __rustts_from_js_value<'js>(
            ctx: &::rustts::__rquickjs::Ctx<'js>,
            value: ::rustts::__rquickjs::Value<'js>,
        ) -> ::rustts::__rquickjs::Result<Self>
        where
            Self: Sized + ::rustts::__serde::de::DeserializeOwned,
        {
            let object = <::rustts::__rquickjs::Object<'js> as ::rustts::__rquickjs::FromJs<'js>>::from_js(ctx, value)?;
            Ok(Self {
                #(#from_fields,)*
            })
        }
    });

    let into_method = directions.into_js.then(|| {
        quote! {
            fn __rustts_into_js_value<'js>(
                self,
                ctx: &::rustts::__rquickjs::Ctx<'js>,
            ) -> ::rustts::__rquickjs::Result<::rustts::__rquickjs::Value<'js>>
            where
                Self: Sized + ::rustts::__serde::Serialize,
            {
                let object = ::rustts::__rquickjs::Object::new(ctx.clone())?;
                #(#into_fields)*
                Ok(object.into_value())
            }
        }
    });

    Ok(vec![quote! {
        #from_method
        #into_method
    }])
}

fn expand_transparent_struct_type(fields: &Fields) -> Result<proc_macro2::TokenStream> {
    let field = transparent_struct_field(fields)?;
    let ty = &field.ty;

    Ok(quote! {
        ::rustts::schema_type_ref::<#ty>()
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
        ::rustts::push_schema_dependency::<#ty>(&mut dependencies);
    })
}

fn expand_named_fields_object(
    fields: &Punctuated<syn::Field, Token![,]>,
    rename_all: Option<RenameRule>,
) -> Result<proc_macro2::TokenStream> {
    let field_steps = fields
        .iter()
        .filter_map(|field| expand_named_struct_field_step(field, rename_all).transpose())
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        {
            let mut fields = Vec::new();
            #(#field_steps)*
            ::rustts::TsType::Object(fields)
        }
    })
}

fn expand_tuple_fields(
    fields: &Punctuated<syn::Field, Token![,]>,
) -> Result<proc_macro2::TokenStream> {
    let items = fields
        .iter()
        .map(|field| {
            reject_flatten_field(field)?;
            let ty = &field.ty;
            Ok(quote! {
                ::rustts::schema_type_ref::<#ty>()
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        ::rustts::TsType::Tuple(vec![#(#items),*])
    })
}

fn expand_named_struct_field_step(
    field: &syn::Field,
    rename_all: Option<RenameRule>,
) -> Result<Option<proc_macro2::TokenStream>> {
    let Some(field) = expand_named_struct_field(field, rename_all)? else {
        return Ok(None);
    };

    Ok(Some(match field {
        NamedStructField::Regular(field) => quote! {
            fields.push(#field);
        },
        NamedStructField::Flatten { ty } => quote! {
            match <#ty as ::rustts::TsSchema>::ts_type() {
                ::rustts::TsType::Object(flattened_fields) => {
                    for flattened_field in flattened_fields {
                        if fields.iter().any(|field: &::rustts::TsField| field.name == flattened_field.name) {
                            panic!("serde flatten produced duplicate TypeScript field `{}`", flattened_field.name);
                        }
                        fields.push(flattened_field);
                    }
                }
                _ => panic!("serde flatten requires a TsSchema object type"),
            }
        },
    }))
}

enum NamedStructField<'a> {
    Regular(proc_macro2::TokenStream),
    Flatten { ty: &'a Type },
}

fn expand_named_struct_field(
    field: &syn::Field,
    rename_all: Option<RenameRule>,
) -> Result<Option<NamedStructField<'_>>> {
    let ident = field
        .ident
        .as_ref()
        .ok_or_else(|| Error::new_spanned(field, "TsSchema requires named fields"))?;
    let attrs = FieldAttrs::from_field(field)?;
    if attrs.skip {
        return Ok(None);
    }
    if attrs.flatten {
        return Ok(Some(NamedStructField::Flatten { ty: &field.ty }));
    }

    let field_name = attrs
        .rename
        .unwrap_or_else(|| rename_field(ident, rename_all));
    let ty = &field.ty;

    if attrs.optional || is_option_type(ty) {
        Ok(Some(NamedStructField::Regular(quote! {
            ::rustts::TsField::optional(
                #field_name,
                ::rustts::schema_type_ref::<#ty>(),
            )
        })))
    } else {
        Ok(Some(NamedStructField::Regular(quote! {
            ::rustts::TsField::required(
                #field_name,
                ::rustts::schema_type_ref::<#ty>(),
            )
        })))
    }
}

fn reject_flatten_field(field: &syn::Field) -> Result<()> {
    let attrs = FieldAttrs::from_field(field)?;
    if attrs.flatten {
        return Err(Error::new_spanned(
            field,
            "serde flatten is only supported on named struct fields",
        ));
    }
    Ok(())
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
    tag: Option<&str>,
    content: Option<&str>,
    untagged: bool,
) -> Result<proc_macro2::TokenStream> {
    if untagged {
        return expand_untagged_enum_type(data);
    }

    if let Some(content) = content {
        return expand_adjacently_tagged_enum_type(data, rename_all, tag, content);
    }

    let explicit_internal_tag = tag.is_some();
    let variants = data
        .variants
        .iter()
        .map(|variant| expand_enum_variant(variant, rename_all, explicit_internal_tag))
        .collect::<Result<Vec<_>>>()?;
    let tag = tag.unwrap_or("type");

    Ok(quote! {
        ::rustts::TsType::Enum {
            tag: Some(String::from(#tag)),
            variants: vec![#(#variants),*],
        }
    })
}

fn expand_adjacently_tagged_enum_type(
    data: &syn::DataEnum,
    rename_all: Option<RenameRule>,
    tag: Option<&str>,
    content: &str,
) -> Result<proc_macro2::TokenStream> {
    let variants = data
        .variants
        .iter()
        .map(|variant| expand_adjacently_tagged_enum_variant(variant, rename_all, content))
        .collect::<Result<Vec<_>>>()?;
    let tag = tag.unwrap_or("type");

    Ok(quote! {
        ::rustts::TsType::Enum {
            tag: Some(String::from(#tag)),
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
        ::rustts::TsType::Union(vec![#(#variants),*])
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
    explicit_internal_tag: bool,
) -> Result<proc_macro2::TokenStream> {
    let attrs = VariantAttrs::from_variant(variant)?;
    let name = attrs
        .rename
        .unwrap_or_else(|| rename_variant(&variant.ident, rename_all));
    match &variant.fields {
        Fields::Unit => Ok(quote! {
            ::rustts::TsEnumVariant::unit(#name)
        }),
        Fields::Named(fields) => {
            let fields = fields
                .named
                .iter()
                .filter_map(|field| expand_enum_payload_field(field).transpose())
                .collect::<Result<Vec<_>>>()?;
            Ok(quote! {
                ::rustts::TsEnumVariant::payload(#name, vec![#(#fields),*])
            })
        }
        Fields::Unnamed(fields) => {
            if explicit_internal_tag {
                return Err(Error::new_spanned(
                    variant,
                    "serde internally tagged tuple enum variants are not supported",
                ));
            }
            let field = tuple_variant_payload_field(&fields.unnamed)?;
            Ok(quote! {
                ::rustts::TsEnumVariant::payload(#name, vec![#field])
            })
        }
    }
}

fn expand_adjacently_tagged_enum_variant(
    variant: &syn::Variant,
    rename_all: Option<RenameRule>,
    content: &str,
) -> Result<proc_macro2::TokenStream> {
    let attrs = VariantAttrs::from_variant(variant)?;
    let name = attrs
        .rename
        .unwrap_or_else(|| rename_variant(&variant.ident, rename_all));
    match &variant.fields {
        Fields::Unit => Ok(quote! {
            ::rustts::TsEnumVariant::unit(#name)
        }),
        Fields::Named(fields) => {
            let payload = expand_named_fields_object(&fields.named, None)?;
            Ok(quote! {
                ::rustts::TsEnumVariant::payload(
                    #name,
                    vec![::rustts::TsField::required(#content, #payload)],
                )
            })
        }
        Fields::Unnamed(fields) => {
            let payload = expand_tuple_payload_type(&fields.unnamed)?;
            Ok(quote! {
                ::rustts::TsEnumVariant::payload(
                    #name,
                    vec![::rustts::TsField::required(#content, #payload)],
                )
            })
        }
    }
}

fn expand_enum_payload_field(field: &syn::Field) -> Result<Option<proc_macro2::TokenStream>> {
    reject_flatten_field(field)?;
    expand_named_struct_field(field, None).map(|field| {
        field.map(|field| match field {
            NamedStructField::Regular(field) => field,
            NamedStructField::Flatten { .. } => unreachable!("flatten rejected before expansion"),
        })
    })
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
        ::rustts::TsField::required(#field_name, #ty)
    })
}

fn expand_tuple_payload_type(
    fields: &Punctuated<syn::Field, Token![,]>,
) -> Result<proc_macro2::TokenStream> {
    if fields.len() == 1 {
        let ty = &fields.first().expect("checked len").ty;
        return Ok(quote! {
            ::rustts::schema_type_ref::<#ty>()
        });
    }

    let items = fields
        .iter()
        .map(|field| {
            reject_flatten_field(field)?;
            let ty = &field.ty;
            Ok(quote! {
                ::rustts::schema_type_ref::<#ty>()
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        ::rustts::TsType::Tuple(vec![#(#items),*])
    })
}
