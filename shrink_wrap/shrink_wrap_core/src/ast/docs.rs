use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use std::fmt::{Display, Formatter};
use syn::LitStr;

#[derive(Clone, Debug)]
pub struct Docs {
    docs: Vec<LitStr>,
}

impl Docs {
    pub fn empty() -> Docs {
        Docs { docs: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn push(&mut self, s: LitStr) {
        self.docs.push(LitStr::new(s.value().trim(), s.span()));
    }

    pub fn push_str(&mut self, s: impl AsRef<str>) {
        self.docs
            .push(LitStr::new(s.as_ref().trim(), Span::call_site()));
    }

    pub fn first_line(&self) -> Option<String> {
        self.docs.first().map(|s| s.value())
    }
}

impl ToTokens for Docs {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for doc in &self.docs {
            tokens.extend(quote!(#[doc = #doc]));
        }
    }
}

impl Display for Docs {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (i, doc) in self.docs.iter().enumerate() {
            write!(f, "{}", doc.value())?;
            if i + 1 < self.docs.len() {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}
