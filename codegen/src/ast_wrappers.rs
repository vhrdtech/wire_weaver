//
// impl ToTokens for Identifier {
//     fn to_tokens<Rust, AnyMod, AnySubMod>(&self, tokens: &mut TokenStream) {
//         tokens.append(Ident::new(
//             Rc::clone(&self.inner.symbols),
//             IdentFlavor::DartAutoRaw,
//             self.inner.span.clone()
//         ));
//     }
// }

// #[cfg(test)]
// mod test {
//     use vhl::ast::identifier::IdentifierContext;
//     use vhl::span::Span;
//     use super::*;
//
//
// }
