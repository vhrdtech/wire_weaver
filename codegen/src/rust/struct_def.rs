use std::io::Read;
use std::iter;
use itertools::Itertools;
use crate::dependencies::{Dependencies, Depends};
use crate::prelude::*;
use crate::rust::identifier::CGIdentifier;
use crate::rust::ty::CGTy;
use ast::{Span, StructDef};
use ast::ty::TyTraits;
use crate::file::CGPiece;

#[derive(Clone)]
pub struct CGStructDef<'ast> {
    pub typename: CGIdentifier<'ast>,
    pub inner: &'ast StructDef,
}

impl<'ast> CGStructDef<'ast> {
    pub fn new(struct_def: &'ast StructDef) -> Self {
        CGStructDef {
            typename: CGIdentifier {
                inner: &struct_def.typename,
            },
            inner: struct_def,
        }
    }

    pub fn codegen(&self, try_to_derive: &Vec<String>) -> Result<CGPiece, CodegenError> {
        let mut piece = CGPiece {
            ts: TokenStream::new(),
            deps: self.dependencies(),
            from: self.inner.span.clone(),
        };

        let field_names = self
            .inner
            .fields
            .iter()
            .map(|f| CGIdentifier { inner: &f.name });
        let field_types = self.inner.fields.iter().map(|f| CGTy { inner: &f.ty });

        let mut ty_traits = TyTraits::default();
        let derive_passthrough: Vec<String> = try_to_derive.iter().cloned().filter(|d| {
            match d.as_str() {
                "Copy" => {
                    ty_traits.is_copy = true;
                    false
                },
                "Clone" => {
                    ty_traits.is_clone = true;
                    false
                },
                "Eq" => {
                    ty_traits.is_eq = true;
                    false
                },
                "PartialEq" => {
                    ty_traits.is_partial_eq = true;
                    false
                },
                _ => {
                    true // pass through all other traits as asked
                }
            }
        }).collect();
        for field in &self.inner.fields {
            ty_traits = ty_traits & field.ty.ty_traits();
        }
        let ty_traits = ty_traits_to_string(ty_traits);
        let supported_derives: String = if ty_traits.is_empty() {
            derive_passthrough.iter().map(|s| s.as_ref()).intersperse(",").collect()
        } else {
            iter::once(ty_traits).chain(derive_passthrough).intersperse(",".to_owned()).collect()
        };

        piece.ts.append_all(mquote!(rust r#"
            #[derive(Λsupported_derives)]
            pub struct Λ{self.typename} {
                ⸨ pub ∀field_names : ∀field_types ⸩,*
            }
        "#));

        Ok(piece)
    }
}

fn ty_traits_to_string(ty_traits: TyTraits) -> String {
    let ty_traits = [ty_traits.is_copy, ty_traits.is_clone, ty_traits.is_eq, ty_traits.is_partial_eq];
    let names = ["Copy", "Clone", "Eq", "PartialEq"];
    ty_traits.iter().enumerate().filter(|(_, is_impl)| **is_impl).map(|(idx, _)| names[idx]).intersperse(",").collect()
}

impl<'ast> Depends for CGStructDef<'ast> {
    fn dependencies(&self) -> Dependencies {
        Dependencies {
            depends: vec![],
            uses: vec![],
        }
    }
}

#[cfg(test)]
mod test {
    use ast::{Definition, Identifier, SourceOrigin, SpanOrigin};
    use mquote::mquote;
    use parser::ast::file::FileParse;

    #[test]
    fn struct_def() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast = FileParse::parse(vhl_input, origin).unwrap();
        match &ast.ast_file.defs[&Identifier::new("Point")] {
            Definition::Struct(struct_def) => {
                let cg_struct_def = super::CGStructDef::new(struct_def);
                let ts = mquote!(rust r#" Λcg_struct_def "#);
                let ts_should_be = mquote!(rust r#"
                    #[derive(Copy, Clone, Eq, PartialEq, Debug)]
                    pub struct Point {
                        pub x: u16◡,
                        pub y: u16
                    }
                "#);

                assert_eq!(format!("{}", ts), format!("{}", ts_should_be));
            }
            _ => panic!("Expected struct definition"),
        }
    }
}
