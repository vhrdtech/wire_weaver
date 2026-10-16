use crate::ast::docs::Docs;
use crate::ast::ty::Type;
use crate::ast::util::Version;
use proc_macro2::Ident;
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
    /// `#[omit_if_default]`: a trailing `#[default]` field that is not written while it has its default value, so
    /// adding it doesn't change the bytes of existing values. Set on the auto-created flag of such an `Option` or
    /// `Result` as well.
    pub(crate) omit_if_default: bool,
}

impl ItemStruct {
    pub(crate) fn make_owned(&mut self) {
        for f in &mut self.fields {
            f.ty.make_owned();
        }
    }
}
