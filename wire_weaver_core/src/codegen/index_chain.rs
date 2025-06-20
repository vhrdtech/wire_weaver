use proc_macro2::TokenStream;
use quote::quote;

#[derive(Copy, Clone)]
pub struct IndexChain {
    len: usize,
}

impl IndexChain {
    pub fn new() -> Self {
        IndexChain { len: 0 }
    }

    pub fn fun_argument_def(&self) -> TokenStream {
        let len = self.len;
        if len == 0 {
            quote! {}
        } else {
            quote! { index_chain: [u32; #len] , }
        }
    }

    pub fn fun_argument_call(&self) -> TokenStream {
        let len = self.len;
        if len == 0 {
            quote! {}
        } else {
            quote! { index_chain , }
        }
    }

    pub fn array_indices(&self) -> TokenStream {
        let len = self.len;
        if len == 0 {
            quote! {}
        } else {
            let indices = (0..len).map(|i| quote! { index_chain[#i] });
            quote! { [ #(#indices),* ] }
        }
    }

    pub fn push_back(&mut self, expr: TokenStream) -> TokenStream {
        let prev_len = self.len;
        self.len += 1;
        let len = self.len;
        if prev_len == 0 {
            quote! {
                let index_chain: [u32; 1] = [#expr];
            }
        } else {
            let copy_previous = (0..prev_len).map(|i| quote! { index_chain[#i] });
            quote! {
                let index_chain: [u32; #len] = [#(#copy_previous),* #expr];
            }
        }
    }
}
