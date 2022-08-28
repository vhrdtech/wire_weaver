use vhl::ast::ty::TyKind;
use crate::prelude::*;
use crate::rust::identifier::Identifier;
use crate::rust::struct_def::StructDef;
use crate::rust::ty::Ty;

pub struct SerDesStruct {
    pub inner: StructDef
}

struct StructSerDesField<'ast> {
    name: &'ast Identifier,
    ty: &'ast Ty,
}

impl<'ast> ToTokens for StructSerDesField<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.ty.inner.kind {
            TyKind::Boolean => {
                tokens.append_all(mquote!(rust r#"
                    wr.put_bool(self.#{self.name})?;
                "#));
            }
            TyKind::Discrete(discrete) => {
                tokens.append_all(mquote!(rust r#"
                    wr.put_u16_le(self.#{self.name})?;
                "#));
            }
        }
    }
}

impl ToTokens for SerDesStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let fields = self.inner.fields
            .iter()
            .map(|f| StructSerDesField {
                name: &f.name,
                ty: &f.ty
            });
        tokens.append_all(mquote!(rust r#"
            impl SerializeBytes for #{self.inner.typename} {
                type Error = BufError;

                fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error> {
                    #( #fields )*
                    Ok(())
                }
            }
        "#));
    }
}

#[cfg(test)]
mod test {
    use mquote::mquote;
    use vhl::ast::file::Definition;
    use vhl::span::{SourceOrigin, SpanOrigin};
    use mtoken::ToTokens;
    use mtoken::ext::TokenStreamExt;
    use crate::prelude::{Span, IdentFlavor, Rc};

    #[test]
    fn struct_serdes_buf() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast_parser = parser::ast::file::File::parse(vhl_input).unwrap();
        let ast_core = vhl::ast::file::File::from_parser_ast(ast_parser, origin);
        match &ast_core.items[0] {
            Definition::Struct(struct_def) => {
                let cg_struct_def = super::StructDef::new(struct_def);
                let cg_struct_serdes = super::SerDesStruct { inner: cg_struct_def };
                let ts = mquote!(rust r#" #cg_struct_serdes "#);

                let ts_should_be = mquote!(rust r#"
                    impl SerializeBytes for Point {
                        type Error = BufError;
                        fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error> {
                            wr.put_u16_le(self.x)?;
                            wr.put_u16_le(self.y)?;
                            Ok(())
                        }
                    }
                "#);
                assert_eq!(format!("{}", ts), format!("{}", ts_should_be));
            }
            _ => panic!("Expected struct definition")
        }
    }
}