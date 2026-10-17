use proc_macro2::{Ident, TokenStream, TokenTree};
use quote::{ToTokens, quote};
use std::collections::HashSet;
use syn::{GenericArgument, GenericParam, PathArguments, Type, TypeParam, WhereClause};

/// Type and const parameters of a derived struct or enum and its where clause (DER-14), as written. Lifetimes are
/// the type's own business: the borrowed side has `'i`, the owned side none.
#[derive(Clone, Debug, Default)]
pub(crate) struct Generics {
    /// `T: Bound = Default` and `const N: usize = 4`, in the order written, attributes removed.
    pub(crate) params: Vec<GenericParam>,
    pub(crate) where_clause: Option<WhereClause>,
    /// Type parameters that the fields only use inside `PhantomData<..>` (DER-15): a unit marker needs no serdes or
    /// `SeriesColumns`, so the impls don't bound them by their trait.
    pub(crate) phantom_only: HashSet<Ident>,
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
            phantom_only: HashSet::new(),
        }
    }

    /// Marks the type parameters that the field types `tys` use only inside `PhantomData<..>` (DER-15). A use the
    /// walk doesn't look into (a macro, a qualified path, a function pointer, ..) counts as a real one.
    pub(crate) fn with_field_types<'a>(mut self, tys: impl IntoIterator<Item = &'a Type>) -> Self {
        let mut used = HashSet::new();
        for ty in tys {
            collect_used_idents(ty, &mut used);
        }
        self.phantom_only = self
            .type_params()
            .map(|p| p.ident.clone())
            .filter(|ident| !used.contains(ident))
            .collect();
        self
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
                let bound = bound.clone().filter(|_| !self.phantom_only.contains(ident));
                let bounds = ty
                    .bounds
                    .iter()
                    .map(|b| quote! { #b })
                    .chain(bound)
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

/// Every identifier of `ty` that can name a type parameter, leaving out the arguments of `PhantomData<..>`.
fn collect_used_idents(ty: &Type, used: &mut HashSet<Ident>) {
    match ty {
        Type::Path(p) if p.qself.is_none() => {
            let Some(last) = p.path.segments.last() else {
                return;
            };
            if last.ident == "PhantomData" {
                return;
            }
            for segment in &p.path.segments {
                used.insert(segment.ident.clone());
                match &segment.arguments {
                    PathArguments::None => {}
                    PathArguments::AngleBracketed(args) => {
                        for arg in &args.args {
                            match arg {
                                GenericArgument::Type(ty) => collect_used_idents(ty, used),
                                GenericArgument::Lifetime(_) => {}
                                other => collect_token_idents(other.to_token_stream(), used),
                            }
                        }
                    }
                    other => collect_token_idents(other.to_token_stream(), used),
                }
            }
        }
        Type::Array(a) => {
            collect_used_idents(&a.elem, used);
            collect_token_idents(a.len.to_token_stream(), used);
        }
        Type::Slice(s) => collect_used_idents(&s.elem, used),
        Type::Reference(r) => collect_used_idents(&r.elem, used),
        Type::Ptr(p) => collect_used_idents(&p.elem, used),
        Type::Paren(p) => collect_used_idents(&p.elem, used),
        Type::Group(g) => collect_used_idents(&g.elem, used),
        Type::Tuple(t) => t.elems.iter().for_each(|ty| collect_used_idents(ty, used)),
        other => collect_token_idents(other.to_token_stream(), used),
    }
}

fn collect_token_idents(tokens: TokenStream, used: &mut HashSet<Ident>) {
    for tt in tokens {
        match tt {
            TokenTree::Ident(ident) => {
                used.insert(ident);
            }
            TokenTree::Group(g) => collect_token_idents(g.stream(), used),
            _ => {}
        }
    }
}
