use crate::ast::docs::Docs;
use crate::ast::generics::Generics;
use crate::ast::ty::Type;
use crate::ast::util::Version;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::Expr;

#[derive(Clone, Debug)]
pub(crate) struct ItemStruct {
    pub(crate) docs: Docs,
    // pub(crate) ident: Ident,
    pub(crate) fields: Vec<Field>,
    /// `#[display("..")]` override (DER-12), takes precedence over the first doc line.
    pub(crate) display: Option<String>,
    /// `sparse` (DER-13): the `OptionAnyVec` slot of each field, increasing.
    pub(crate) sparse_slots: Option<Vec<u32>>,
    /// `struct S { a: u8 }`, `struct S(u8);` or `struct S;` (SW-59).
    pub(crate) kind: StructKind,
    /// `struct Timed<T> { .. }`, `struct Buf<const N: usize>` (DER-14): parameters and where clause as written.
    pub(crate) generics: Generics,
}

/// How a struct's fields are written in Rust (SW-59). The wire format is the same: fields in order. A tuple struct's
/// fields are named `_0`, `_1`, ... wherever a name is needed (locals, consts, C names, error messages).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum StructKind {
    Named,
    Tuple,
    Unit,
}

impl StructKind {
    pub(crate) fn from_syn(fields: &syn::Fields) -> Self {
        match fields {
            syn::Fields::Named(_) => StructKind::Named,
            syn::Fields::Unnamed(_) => StructKind::Tuple,
            syn::Fields::Unit => StructKind::Unit,
        }
    }

    /// How generated code reaches field `ident`: `ident` in a named struct, its position (`_1` is `1`) in a tuple
    /// struct, for `self.#member` and `Self { #member: .. }`.
    pub(crate) fn member(self, ident: &Ident) -> TokenStream {
        if self == StructKind::Tuple
            && let Some(idx) = ident
                .to_string()
                .strip_prefix('_')
                .and_then(|i| i.parse::<usize>().ok())
        {
            let idx = syn::Index::from(idx);
            return quote! { #idx };
        }
        quote! { #ident }
    }

    /// `ident` (shorthand) in a named struct, `1: _1` in a tuple struct, for `Self { .. }` from locals named after
    /// the fields.
    pub(crate) fn field_init(self, ident: &Ident) -> TokenStream {
        match self {
            StructKind::Tuple => {
                let member = self.member(ident);
                quote! { #member: #ident }
            }
            _ => quote! { #ident },
        }
    }
}

impl ItemStruct {
    /// The `display` (DER-12) message: the `#[display("..")]` override, or the first doc line. `None` when
    /// neither is present.
    pub(crate) fn display_message(&self) -> Option<String> {
        self.display.clone().or_else(|| self.docs.message_line())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub(crate) docs: Docs,
    pub(crate) _id: u32,
    pub(crate) ident: Ident,
    pub(crate) ty: Type,
    pub(crate) _since: Option<Version>,
    pub(crate) default: Option<Expr>,
    /// `#[series(opaque)]` (SW-60): in a `Series`, an opaque run of the value's own bytes instead of its columns.
    pub(crate) series_opaque: bool,
}

impl ItemStruct {
    pub(crate) fn make_owned(&mut self) {
        for f in &mut self.fields {
            f.ty.make_owned();
        }
    }
}
