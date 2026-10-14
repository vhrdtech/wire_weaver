use crate::ast::docs::Docs;
use crate::ast::item_struct::Field;
use crate::ast::ty::Type;
use crate::ast::util::Version;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

#[derive(Clone, Debug)]
pub(crate) struct ItemEnum {
    pub(crate) docs: Docs,
    // pub(crate) ident: Ident,
    pub(crate) variants: Vec<Variant>,
    /// The `#[future_version]` variant, not in `variants`: it has no discriminant of its own on the wire (SW-16).
    pub(crate) future_version: Option<FutureVersion>,
}

/// `#[future_version] Unknown(u32)`: holds a discriminant this version doesn't know, read from a newer version.
#[derive(Clone, Debug)]
pub(crate) struct FutureVersion {
    pub(crate) docs: Docs,
    pub(crate) ident: Ident,
}

#[derive(Clone, Debug)]
pub(crate) enum Fields {
    Named(Vec<Field>),
    Unnamed(Vec<Type>),
    Unit,
}

impl Fields {
    /// Field types in wire order, flags included.
    pub(crate) fn types(&self) -> Vec<&Type> {
        match self {
            Fields::Named(named) => named.iter().map(|f| &f.ty).collect(),
            Fields::Unnamed(unnamed) => unnamed.iter().collect(),
            Fields::Unit => vec![],
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Variant {
    pub(crate) docs: Docs,
    pub(crate) ident: Ident,
    pub(crate) fields: Fields,
    pub(crate) discriminant: u32,
    pub(crate) _since: Option<Version>,
    /// Predicates of the `#[cfg(..)]` attributes on the variant, passed through to everything generated for it.
    /// The discriminant does not depend on them: a variant keeps its number in every configuration.
    pub(crate) cfg: Vec<TokenStream>,
    /// `#[display("..")]` override (DER-12), takes precedence over the first doc line.
    pub(crate) display: Option<String>,
    /// `#[common] Common(CommonError<'i>)`: gets `From<CommonError>` and `HasCommon` (STD-16).
    pub(crate) common: bool,
}

impl Variant {
    /// `#[cfg(..)]` attributes of the variant, empty when it has none.
    pub(crate) fn cfg_attrs(&self) -> TokenStream {
        let cfg = &self.cfg;
        quote! { #(#[cfg(#cfg)])* }
    }

    /// `#[cfg(not(..))]`: the configurations the variant is compiled out of. None when it has no `#[cfg]`.
    pub(crate) fn not_cfg_attr(&self) -> Option<TokenStream> {
        let cfg = &self.cfg;
        (!cfg.is_empty()).then(|| quote! { #[cfg(not(all(#(#cfg),*)))] })
    }

    /// The `display` (DER-12) message: the `#[display("..")]` override, or the first doc line. `None` when
    /// neither is present.
    pub(crate) fn display_message(&self) -> Option<String> {
        self.display.clone().or_else(|| self.docs.message_line())
    }
}

impl ItemEnum {
    pub(crate) fn make_discriminants(&mut self) {
        for v in &mut self.variants {
            v.fields = Fields::Unit;
        }
    }

    pub(crate) fn make_owned(&mut self) {
        for v in &mut self.variants {
            match &mut v.fields {
                Fields::Named(named) => {
                    for f in named {
                        f.ty.make_owned();
                    }
                }
                Fields::Unnamed(unnamed) => {
                    for f in unnamed {
                        f.make_owned();
                    }
                }
                Fields::Unit => {}
            }
        }
    }
}
