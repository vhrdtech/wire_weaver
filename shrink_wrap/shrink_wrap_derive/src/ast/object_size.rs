use crate::codegen::util::CratePath;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{LitInt, LitStr};

/// Object size from shrink_wrap crate, copied here to decouple the two. Generated code refers to the shrink_wrap one.
/// Extensive description is in shrink_wrap.
#[derive(Clone, Debug)]
pub(crate) enum ObjectSize {
    Unsized,
    UnsizedFinalStructure,
    SelfDescribing,
    /// `size_bits` is the part known to the macro; `symbolic` holds terms it can't evaluate (an array length
    /// given as a const path, a `TailSize<N>` width), emitted as `size_bits + term + ..` for the compiler to fold.
    Sized {
        size_bits: usize,
        symbolic: Vec<TokenStream>,
    },
}

impl ObjectSize {
    pub(crate) fn sized(size_bits: usize) -> Self {
        ObjectSize::Sized {
            size_bits,
            symbolic: Vec::new(),
        }
    }

    pub(crate) fn sized_symbolic(term: TokenStream) -> Self {
        ObjectSize::Sized {
            size_bits: 0,
            symbolic: vec![term],
        }
    }

    /// `ElementSize::..` expression naming the `shrink_wrap` type through `cp`.
    pub(crate) fn tokens(&self, cp: &CratePath) -> TokenStream {
        let element_size = cp.item(quote! { ElementSize });
        match self {
            ObjectSize::Unsized => quote! { #element_size::Unsized },
            ObjectSize::UnsizedFinalStructure => quote! { #element_size::UnsizedFinalStructure },
            ObjectSize::SelfDescribing => quote! { #element_size::SelfDescribing },
            ObjectSize::Sized {
                size_bits,
                symbolic,
            } => {
                let size_bits = LitInt::new(format!("{size_bits}").as_str(), Span::call_site());
                quote! { #element_size::Sized { size_bits: #size_bits #(+ #symbolic)* } }
            }
        }
    }

    pub(crate) fn sum_recursively(
        &self,
        sizes: Vec<Ident>,
        is_ref: bool,
        cp: &CratePath,
    ) -> TokenStream {
        let this = self.tokens(cp);
        if sizes.is_empty() {
            this
        } else {
            let sizes = sum_unknown(sizes, is_ref, cp);
            quote! { #this.add(#sizes) }
        }
    }

    pub(crate) fn assert_element_size(
        &self,
        ident: &Ident,
        cfg: Option<&TokenStream>,
        is_ref: bool,
        cp: &CratePath,
    ) -> TokenStream {
        let element_size = cp.item(quote! { ElementSize });
        let size_ts = match self {
            ObjectSize::Unsized => quote! { #element_size::Unsized },
            ObjectSize::UnsizedFinalStructure => quote! { #element_size::UnsizedFinalStructure },
            ObjectSize::SelfDescribing => quote! { #element_size::SelfDescribing },
            ObjectSize::Sized { .. } => quote! { #element_size::Sized { .. } },
        };
        let size = match self {
            ObjectSize::Unsized => "Unsized",
            ObjectSize::UnsizedFinalStructure => "UnsizedFinalStructure",
            ObjectSize::SelfDescribing => "SelfDescribing",
            ObjectSize::Sized { .. } => "Sized",
        };
        let err_msg = format!("{} must be {size}", ident);
        let err_msg = LitStr::new(&err_msg, Span::call_site());
        let cfg = cfg.map(|cond| quote! { #[cfg(#cond)] });
        let (ser_trait, des_trait) = cp.serdes_traits(is_ref);
        quote! {
            #cfg
            const _: () = assert!(
                matches!(<#ident as #ser_trait>::ELEMENT_SIZE, #size_ts),
                #err_msg
            );

            #cfg
            const _: () = assert!(
                matches!(<#ident as #des_trait>::ELEMENT_SIZE, #size_ts),
                #err_msg
            );
        }
    }

    /// IMPORTANT: this method must be a copy of the one in shrink_wrap
    pub(crate) fn add(&self, other: ObjectSize) -> ObjectSize {
        // Order is very important here, size requirement is bumped from Sized to SelfDescribing to Unsized.
        // UFS is a bit tricky, it is "contagious", so that Vec<T> with T Unsized is UFS.
        // Note that structs and enums cannot accidentally become UFS, because by default they are Unsized, and no sum operations are
        // performed, otherwise it would have been a compatibility problem.
        match (self, other) {
            (ObjectSize::UnsizedFinalStructure, _) => ObjectSize::UnsizedFinalStructure,
            (_, ObjectSize::UnsizedFinalStructure) => ObjectSize::UnsizedFinalStructure,
            (ObjectSize::Unsized, _) => ObjectSize::Unsized,
            (_, ObjectSize::Unsized) => ObjectSize::Unsized,
            (ObjectSize::SelfDescribing, _) => ObjectSize::SelfDescribing,
            (_, ObjectSize::SelfDescribing) => ObjectSize::SelfDescribing,
            (
                ObjectSize::Sized {
                    size_bits: size_a,
                    symbolic: symbolic_a,
                },
                ObjectSize::Sized {
                    size_bits: size_b,
                    symbolic: symbolic_b,
                },
            ) => ObjectSize::Sized {
                size_bits: *size_a + size_b,
                symbolic: symbolic_a.iter().cloned().chain(symbolic_b).collect(),
            },
        }
    }

    pub(crate) fn is_unsized(&self) -> bool {
        matches!(self, ObjectSize::Unsized)
    }
}

fn sum_unknown(mut sizes: Vec<Ident>, is_ref: bool, cp: &CratePath) -> TokenStream {
    let (ser_trait, _) = cp.serdes_traits(is_ref);
    if let Some(ident) = sizes.pop() {
        let inner = sum_unknown(sizes, is_ref, cp);
        if inner.is_empty() {
            quote! { <#ident as #ser_trait>::ELEMENT_SIZE }
        } else {
            quote! { <#ident as #ser_trait>::ELEMENT_SIZE.add(#inner) }
        }
    } else {
        TokenStream::new()
    }
}
