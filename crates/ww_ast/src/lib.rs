pub mod data;
pub mod file;
pub mod item;
pub mod ty;
pub mod version;

pub use file::File;

// pub(crate) struct ConversionResult<T> {
//     pub(crate) warnings: Vec<>,
//     pub(crate) result: Result<T, Vec<>>
// }

// #[derive(Debug)]
// pub struct Span {
//     pub byte_range: Range<usize>
// }

#[derive(Debug)]
pub struct Ident {
    pub sym: String,
    // pub span: Span,
}

impl From<syn::Ident> for Ident {
    fn from(value: syn::Ident) -> Self {
        Ident {
            sym: value.to_string(),
            // span: Span {
            //     byte_range: value.span().byte_range()
            // }
        }
    }
}

impl From<&Ident> for syn::Ident {
    fn from(value: &Ident) -> Self {
        let ident = value.sym.as_str();
        syn::Ident::new(ident, proc_macro2::Span::call_site())
    }
}
