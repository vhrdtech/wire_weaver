use crate::prelude::*;
use crate::rust::identifier::Identifier;
use crate::rust::ty::Ty;

pub struct StructDef {
    pub typename: Identifier,
    pub fields: Vec<StructField>,
}

impl StructDef {
    pub fn new(struct_def: &vhl::ast::struct_def::StructDef) -> Self {
        StructDef {
            typename: struct_def.typename.clone().into(),
            fields: struct_def.fields.fields.iter()
                .map(|item| item.clone().into())
                .collect()
        }
    }
}

#[derive(Clone)]
pub struct StructField {
    pub name: Identifier,
    pub ty: Ty,
}

impl From<vhl::ast::struct_def::StructField> for StructField {
    fn from(field: vhl::ast::struct_def::StructField) -> Self {
        StructField {
            name: field.name.into(),
            ty: field.ty.into(),
        }
    }
}

impl ToTokens for StructDef {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let fields = self.fields.clone();
        tokens.append_all(mquote!(rust r#"
            struct #{self.typename} {
                #( #fields ),*
            }
        "#));
    }
}

impl ToTokens for StructField {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(mquote!(rust r#"
            pub #{self.name}: #{self.ty}
        "#));
    }
}

#[cfg(test)]
mod test {
    use mquote::mquote;
    use vhl::ast::file::Definition;
    use vhl::span::{SourceOrigin, SpanOrigin};
    use mtoken::ToTokens;

    #[test]
    fn struct_def() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast_parser = parser::ast::file::File::parse(vhl_input).unwrap();
        let ast_core = vhl::ast::file::File::from_parser_ast(ast_parser, origin);
        match &ast_core.items[0] {
            Definition::Struct(struct_def) => {
                let cg_struct_def = super::StructDef::new(struct_def);
                let ts = mquote!(rust r#" #cg_struct_def "#);
                println!("{}", ts);
            }
            _ => panic!("Expected struct definition")
        }
    }
}