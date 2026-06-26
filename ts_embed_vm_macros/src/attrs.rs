use syn::{
    Attribute, DeriveInput, Error, Expr, Lit, LitStr, Meta, Result, Token, punctuated::Punctuated,
};

use crate::rename::RenameRule;

#[derive(Default)]
pub(super) struct ContainerAttrs {
    pub(super) schema_name: Option<String>,
    pub(super) rename_all: Option<RenameRule>,
    pub(super) untagged: bool,
    pub(super) transparent: bool,
}

impl ContainerAttrs {
    pub(super) fn from_input(input: &DeriveInput) -> Result<Self> {
        let mut attrs = Self::default();
        attrs.apply_tsvm_attrs(&input.attrs)?;
        attrs.apply_serde_attrs(&input.attrs)?;
        Ok(attrs)
    }

    fn apply_tsvm_attrs(&mut self, attrs: &[Attribute]) -> Result<()> {
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("tsvm")) {
            for meta in parse_tsvm_attr(attr)? {
                match meta {
                    Meta::NameValue(value) if value.path.is_ident("name") => {
                        self.schema_name = Some(string_lit_value(&value.value)?);
                    }
                    Meta::NameValue(value) if value.path.is_ident("rename") => {
                        self.schema_name = Some(string_lit_value(&value.value)?);
                    }
                    other => {
                        return Err(Error::new_spanned(other, "unsupported tsvm type attribute"));
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_serde_attrs(&mut self, attrs: &[Attribute]) -> Result<()> {
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
            let serde_attrs = SerdeAttrs::parse(attr)?;
            if let Some(rename_all) = serde_attrs.rename_all {
                self.rename_all = Some(rename_all);
            }
            self.untagged |= serde_attrs.untagged;
            self.transparent |= serde_attrs.transparent;
        }
        Ok(())
    }
}

#[derive(Default)]
struct SerdeAttrs {
    rename: Option<String>,
    rename_all: Option<RenameRule>,
    default: bool,
    skip: bool,
    untagged: bool,
    transparent: bool,
}

impl SerdeAttrs {
    fn parse(attr: &Attribute) -> Result<Self> {
        let mut attrs = Self::default();
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value = meta.value()?;
                let lit: LitStr = value.parse()?;
                attrs.rename = Some(lit.value());
            } else if meta.path.is_ident("rename_all") {
                let value = meta.value()?;
                let lit: LitStr = value.parse()?;
                attrs.rename_all = Some(RenameRule::parse(&lit)?);
            } else if meta.path.is_ident("skip") {
                attrs.skip = true;
            } else if meta.path.is_ident("untagged") {
                attrs.untagged = true;
            } else if meta.path.is_ident("transparent") {
                attrs.transparent = true;
            } else if meta.path.is_ident("default") {
                attrs.default = true;
                if meta.input.peek(Token![=]) {
                    let _ = meta.value()?.parse::<Expr>()?;
                }
            } else if meta.input.peek(Token![=]) {
                let _ = meta.value()?.parse::<Expr>()?;
            }
            Ok(())
        })?;
        Ok(attrs)
    }
}

#[derive(Default)]
pub(super) struct FieldAttrs {
    pub(super) rename: Option<String>,
    pub(super) optional: bool,
    pub(super) skip: bool,
}

impl FieldAttrs {
    pub(super) fn from_field(field: &syn::Field) -> Result<Self> {
        let mut attrs = Self::default();
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("tsvm"))
        {
            for meta in parse_tsvm_attr(attr)? {
                match meta {
                    Meta::Path(path) if path.is_ident("optional") => attrs.optional = true,
                    Meta::NameValue(value) if value.path.is_ident("rename") => {
                        attrs.rename = Some(string_lit_value(&value.value)?);
                    }
                    other => {
                        return Err(Error::new_spanned(
                            other,
                            "unsupported tsvm field attribute",
                        ));
                    }
                }
            }
        }
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("serde"))
        {
            let serde_attrs = SerdeAttrs::parse(attr)?;
            if attrs.rename.is_none() {
                attrs.rename = serde_attrs.rename;
            }
            attrs.optional |= serde_attrs.default;
            attrs.skip |= serde_attrs.skip;
        }
        Ok(attrs)
    }
}

#[derive(Default)]
pub(super) struct VariantAttrs {
    pub(super) rename: Option<String>,
}

impl VariantAttrs {
    pub(super) fn from_variant(variant: &syn::Variant) -> Result<Self> {
        let mut attrs = Self {
            rename: parse_tsvm_variant_rename(&variant.attrs)?,
        };
        for attr in variant
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("serde"))
        {
            let serde_attrs = SerdeAttrs::parse(attr)?;
            if attrs.rename.is_none() {
                attrs.rename = serde_attrs.rename;
            }
            if serde_attrs.rename_all.is_some() {
                return Err(Error::new_spanned(
                    attr,
                    "serde rename_all is not supported on enum variants",
                ));
            }
        }
        Ok(attrs)
    }
}

fn parse_tsvm_variant_rename(attrs: &[Attribute]) -> Result<Option<String>> {
    let mut rename = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("tsvm")) {
        for meta in parse_tsvm_attr(attr)? {
            match meta {
                Meta::NameValue(value) if value.path.is_ident("rename") => {
                    rename = Some(string_lit_value(&value.value)?);
                }
                other => {
                    return Err(Error::new_spanned(
                        other,
                        "unsupported tsvm enum variant attribute",
                    ));
                }
            }
        }
    }
    Ok(rename)
}

fn parse_tsvm_attr(attr: &syn::Attribute) -> Result<Punctuated<Meta, Token![,]>> {
    attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
}

fn string_lit_value(expr: &Expr) -> Result<String> {
    match expr {
        Expr::Lit(expr) => match &expr.lit {
            Lit::Str(value) => Ok(value.value()),
            _ => Err(Error::new_spanned(expr, "expected string literal")),
        },
        _ => Err(Error::new_spanned(expr, "expected string literal")),
    }
}
