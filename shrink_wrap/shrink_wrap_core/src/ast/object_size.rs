use crate::ast::util::Cfg;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{LitInt, LitStr};

/// Object size from shrink_wrap crate, copied here to decouple the two. Generated code refers to the shrink_wrap one.
/// Extensive description is in shrink_wrap.
#[derive(Copy, Clone, Debug)]
pub enum ObjectSize {
    Unsized,
    UnsizedFinalStructure,
    SelfDescribing,
    Sized { size_bits: usize },
}

impl ToTokens for ObjectSize {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ts = match self {
            ObjectSize::Unsized => quote! { ElementSize::Unsized },
            ObjectSize::UnsizedFinalStructure => quote! { ElementSize::UnsizedFinalStructure },
            ObjectSize::SelfDescribing => quote! { ElementSize::SelfDescribing },
            ObjectSize::Sized { size_bits } => {
                let size_bits = LitInt::new(format!("{size_bits}").as_str(), Span::call_site());
                quote! { ElementSize::Sized { size_bits: #size_bits } }
            }
        };
        tokens.extend(ts);
    }
}

impl ObjectSize {
    pub fn sum_recursively(&self, sizes: Vec<Ident>) -> TokenStream {
        if sizes.is_empty() {
            quote! { #self }
        } else {
            let sizes = sum_unknown(sizes);
            quote! { #self.add(#sizes) }
        }
    }

    pub fn assert_element_size(&self, ident: &Ident, cfg: &Option<Cfg>) -> TokenStream {
        let size_ts = match self {
            ObjectSize::Unsized => quote! { Unsized },
            ObjectSize::UnsizedFinalStructure => quote! { UnsizedFinalStructure },
            ObjectSize::SelfDescribing => quote! { SelfDescribing },
            ObjectSize::Sized { .. } => quote! { Sized { .. } },
        };
        let size = match self {
            ObjectSize::Unsized => "Unsized",
            ObjectSize::UnsizedFinalStructure => "UnsizedFinalStructure",
            ObjectSize::SelfDescribing => "SelfDescribing",
            ObjectSize::Sized { .. } => "Sized",
        };
        let err_msg = format!("{} must be {size}", ident);
        let err_msg = LitStr::new(&err_msg, Span::call_site());
        quote! {
            #cfg
            const _: () = assert!(
                matches!(<#ident as SerializeShrinkWrap>::ELEMENT_SIZE, ElementSize::#size_ts),
                #err_msg
            );

            #cfg
            const _: () = assert!(
                matches!(<#ident as DeserializeShrinkWrap>::ELEMENT_SIZE, ElementSize::#size_ts),
                #err_msg
            );
        }
    }

    /// IMPORTANT: this method must be a copy of the one in shrink_wrap
    pub fn add(&self, other: ObjectSize) -> ObjectSize {
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
            (ObjectSize::Sized { size_bits: size_a }, ObjectSize::Sized { size_bits: size_b }) => {
                ObjectSize::Sized {
                    size_bits: *size_a + size_b,
                }
            }
        }
    }

    pub fn is_unsized(&self) -> bool {
        matches!(self, ObjectSize::Unsized)
    }
}

fn sum_unknown(mut sizes: Vec<Ident>) -> TokenStream {
    if let Some(ident) = sizes.pop() {
        let inner = sum_unknown(sizes);
        if inner.is_empty() {
            quote! { <#ident as SerializeShrinkWrap>::ELEMENT_SIZE }
        } else {
            quote! { <#ident as SerializeShrinkWrap>::ELEMENT_SIZE.add(#inner) }
        }
    } else {
        TokenStream::new()
    }
}
