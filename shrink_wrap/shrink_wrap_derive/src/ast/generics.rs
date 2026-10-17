use proc_macro2::TokenStream;
use quote::quote;
use syn::{GenericParam, TypeParam, WhereClause};

/// Type and const parameters of a derived struct or enum and its where clause (DER-14), as written. Lifetimes are
/// the type's own business: the borrowed side has `'i`, the owned side none.
#[derive(Clone, Debug, Default)]
pub(crate) struct Generics {
    /// `T: Bound = Default` and `const N: usize = 4`, in the order written, attributes removed.
    pub(crate) params: Vec<GenericParam>,
    pub(crate) where_clause: Option<WhereClause>,
}

impl Generics {
    pub(crate) fn from_syn(generics: &syn::Generics) -> Self {
        let params = generics
            .params
            .iter()
            .filter_map(|param| {
                let mut param = param.clone();
                match &mut param {
                    GenericParam::Type(ty) => ty.attrs.clear(),
                    GenericParam::Const(c) => c.attrs.clear(),
                    GenericParam::Lifetime(_) => return None,
                }
                Some(param)
            })
            .collect();
        Generics {
            params,
            where_clause: generics.where_clause.clone(),
        }
    }

    /// No parameters and no where clause: the code is the same as before DER-14.
    pub(crate) fn is_empty(&self) -> bool {
        self.params.is_empty() && self.where_clause.is_none()
    }

    pub(crate) fn type_params(&self) -> impl Iterator<Item = &TypeParam> {
        self.params.iter().filter_map(|p| match p {
            GenericParam::Type(ty) => Some(ty),
            _ => None,
        })
    }

    /// `<'i, T: Bound = Default, const N: usize>` of the definition, `lifetime` on the borrowed side.
    pub(crate) fn def(&self, lifetime: bool) -> TokenStream {
        let params = &self.params;
        match (lifetime, params.is_empty()) {
            (false, true) => quote! {},
            (true, true) => quote! { <'i> },
            (false, false) => quote! { <#(#params),*> },
            (true, false) => quote! { <'i, #(#params),*> },
        }
    }

    /// `<'i, T, N>` after the type's name.
    pub(crate) fn ty(&self, lifetime: bool) -> TokenStream {
        let idents = self.params.iter().map(|p| match p {
            GenericParam::Type(ty) => &ty.ident,
            GenericParam::Const(c) => &c.ident,
            GenericParam::Lifetime(_) => unreachable!("lifetimes are not kept"),
        });
        let idents: Vec<_> = idents.collect();
        match (lifetime, idents.is_empty()) {
            (false, true) => quote! {},
            (true, true) => quote! { <'i> },
            (false, false) => quote! { <#(#idents),*> },
            (true, false) => quote! { <'i, #(#idents),*> },
        }
    }

    /// `T: Own + #bound, const N: usize,` for an `impl<..>`: each type parameter with its own bounds and `bound`
    /// (none when `None`), each const parameter without its default; with a trailing comma, empty without
    /// parameters.
    pub(crate) fn impl_params(&self, bound: Option<TokenStream>) -> TokenStream {
        let params = self.params.iter().map(|p| match p {
            GenericParam::Type(ty) => {
                let ident = &ty.ident;
                let bounds = ty
                    .bounds
                    .iter()
                    .map(|b| quote! { #b })
                    .chain(bound.clone())
                    .collect::<Vec<_>>();
                if bounds.is_empty() {
                    quote! { #ident, }
                } else {
                    quote! { #ident: #(#bounds)+*, }
                }
            }
            GenericParam::Const(c) => {
                let ident = &c.ident;
                let ty = &c.ty;
                quote! { const #ident: #ty, }
            }
            GenericParam::Lifetime(_) => unreachable!("lifetimes are not kept"),
        });
        quote! { #(#params)* }
    }

    /// `where ..`: the type's own predicates, then `extra` ones of an impl; empty when there are none.
    pub(crate) fn where_clause(&self, extra: &[TokenStream]) -> TokenStream {
        let own = self.where_clause.iter().flat_map(|w| w.predicates.iter());
        let preds: Vec<TokenStream> = own
            .map(|p| quote! { #p })
            .chain(extra.iter().cloned())
            .collect();
        if preds.is_empty() {
            quote! {}
        } else {
            quote! { where #(#preds),* }
        }
    }

    /// The where clause of an `impl Error`: `Error` needs `Debug`, which a derive gives a generic type only for
    /// some parameters.
    pub(crate) fn error_where(&self) -> TokenStream {
        if self.params.is_empty() {
            self.where_clause(&[])
        } else {
            self.where_clause(&[quote! { Self: ::core::fmt::Debug }])
        }
    }
}
