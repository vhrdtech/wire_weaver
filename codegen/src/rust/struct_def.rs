use crate::dependencies::{Dependencies, Depends};
use crate::prelude::*;
use crate::rust::identifier::CGIdentifier;
use crate::rust::ty::CGTy;
use vhl::ast::struct_def::StructDef;

#[derive(Clone)]
pub struct CGStructDef<'ast> {
    pub typename: CGIdentifier<'ast>,
    pub inner: &'ast StructDef, // pub fields: Vec<CGStructField>,
}

impl<'ast> CGStructDef<'ast> {
    pub fn new(struct_def: &'ast vhl::ast::struct_def::StructDef) -> Self {
        CGStructDef {
            typename: CGIdentifier {
                inner: &struct_def.typename,
            },
            inner: struct_def, // fields: struct_def.fields.fields.iter()
                               //     .map(|item| item.clone().into())
                               //     .collect()
        }
    }
}

// #[derive(Clone)]
// pub struct CGStructField {
//     pub name: Identifier,
//     pub ty: Ty,
// }
//
// impl From<vhl::ast::struct_def::StructField> for CGStructField {
//     fn from(field: vhl::ast::struct_def::StructField) -> Self {
//         CGStructField {
//             name: field.name.into(),
//             ty: field.ty.into(),
//         }
//     }
// }

impl<'ast> ToTokens for CGStructDef<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let field_names = self
            .inner
            .fields
            .iter()
            .map(|f| CGIdentifier { inner: &f.name });
        let field_types = self
            .inner
            .fields
            .iter()
            .map(|f| CGTy { inner: &f.ty });
        let derives = mquote!(rust " #[derive(Copy, Clone, Eq, PartialEq, Debug)] "); // TODO: make automatic and configurable
        tokens.append_all(mquote!(rust r#"
            Λderives
            pub struct Λ{self.typename} {
                ⸨ pub ∀field_names : ∀field_types ⸩,*
            }
        "#));
    }
}

impl<'ast> Depends for CGStructDef<'ast> {
    fn dependencies(&self) -> Dependencies {
        Dependencies {
            depends: vec![],
            uses: vec![],
        }
    }
}

// impl ToTokens for CGStructField {
//     fn to_tokens(&self, tokens: &mut TokenStream) {
//         tokens.append_all(mquote!(rust r#"
//             pub #{self.name}: #{self.ty}
//         "#));
//     }
// }

#[cfg(test)]
mod test {
    use mquote::mquote;
    use parser::span::{SourceOrigin, SpanOrigin};
    use vhl::ast::file::Definition;

    #[test]
    fn struct_def() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast_parser = parser::ast::file::File::parse(vhl_input, origin).unwrap();
        let ast_core = vhl::ast::file::File::from_parser_ast(ast_parser);
        match &ast_core.items[0] {
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
            _ => panic!("Expected struct definition")
        }
    }
}
