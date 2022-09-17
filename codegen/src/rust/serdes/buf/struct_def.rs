use crate::prelude::*;
use crate::rust::identifier::CGIdentifier;
use crate::rust::struct_def::CGStructDef;
use crate::rust::ty::CGTy;
use semver::VersionReq;
use vhl::ast::ty::TyKind;

pub struct StructSer<'ast> {
    pub inner: CGStructDef<'ast>,
}

impl<'ast> StructSer<'ast> {
    pub fn len_bytes(&self) -> Option<usize> {
        let mut len = 0;
        for f in &self.inner.inner.fields {
            if !f.ty.is_sized() {
                return None;
            }
            len += match &f.ty.kind {
                TyKind::Unit => 0,
                TyKind::Boolean => 1,
                TyKind::Discrete(discrete) => {
                    (discrete.bits / 8 + if discrete.bits % 2 != 0 { 1 } else { 0 }) as usize
                }
                _ => unimplemented!(), // ?

            };
        }
        Some(len)
    }
}

pub struct StructDes<'ast> {
    pub inner: CGStructDef<'ast>,
}

struct StructSerField<'ast> {
    ty: CGTy<'ast>,
}

impl<'ast> ToTokens for StructSerField<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.ty.inner.kind {
            TyKind::Unit => {},
            TyKind::Boolean => {
                tokens.append_all(mquote!(rust r#"
                    wr.put_bool
                "#));
            }
            TyKind::Discrete(discrete) => {
                if discrete.is_standard() {
                    let sign = if discrete.is_signed { 'i' } else { 'u' };
                    let is_le = if discrete.bits == 8 { "" } else { "_le" };
                    let method = format!("put_{}{}{}", sign, discrete.bits, is_le);
                    tokens.append_all(mquote!(rust r#"
                        wr.#method
                    "#));
                } else {
                    // Ix / Ux / UxSpy / UxSny / IxSpy / IxSny, use generic ser<T: SerializeBuf>()
                    tokens.append_all(mquote!(rust r#"
                        wr.ser
                    "#));
                }
            }
            _ => unimplemented!()
        }
    }
}

struct StructDesField<'ast> {
    ty: CGTy<'ast>,
}

impl<'ast> ToTokens for StructDesField<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.ty.inner.kind {
            TyKind::Unit => {},
            TyKind::Boolean => {
                tokens.append_all(mquote!(rust r#"
                    rdr.get_bool()?
                "#));
            }
            TyKind::Discrete(discrete) => {
                if discrete.is_standard() {
                    let sign = if discrete.is_signed { 'i' } else { 'u' };
                    let is_le = if discrete.bits == 8 { "" } else { "_le" };
                    let method = format!("get_{}{}{}", sign, discrete.bits, is_le);
                    tokens.append_all(mquote!(rust r#"
                        rdr.#method()?
                    "#));
                } else {
                    // Ix / Ux / UxSpy / UxSny / IxSpy / IxSny, use generic des<T: DeserializeBuf>()
                    tokens.append_all(mquote!(rust r#"
                        rdr.des_bytes()?
                    "#));
                }
            }
            _ => unimplemented!()
        }
    }
}

impl<'ast> ToTokens for StructSer<'ast> {
    fn to_tokens(&self, ts: &mut TokenStream) {
        let field_names = self
            .inner
            .inner
            .fields
            .iter()
            .map(|field| CGIdentifier { inner: &field.name });
        let field_ser_methods = self
            .inner
            .inner
            .fields
            .iter()
            .map(|f| StructSerField {
                ty: CGTy { inner: &f.ty },
            });
        let len_bytes = self.len_bytes().unwrap();
        ts.append_all(mquote!(rust r#"
            impl SerializeBytes for #{self.inner.typename} {
                ȸtype Error = BufError;

                fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error> {
                    #( #field_ser_methods \\( self.#field_names \\) ?; )*
                    Ok(())
                }

                fn len_bytes(&self) -> usize {
                    #len_bytes
                }
            }
        "#));
    }
}

impl<'ast> Depends for StructSer<'ast> {
    fn dependencies(&self) -> Dependencies {
        let depends = vec![Package::RustCrate(
            RustCrateSource::Crates("vhl-stdlib".to_string()),
            VersionReq::parse("0.1.0").unwrap(),
        )];

        use Import::*;
        let uses = vec![Submodule(
            "vhl_stdlib",
            vec![Submodule(
                "serdes",
                vec![
                    Submodule("traits", vec![Entity("SerializeBytes")]),
                    Submodule("buf", vec![Entity("BufMut"), EntityAs("Error", "BufError")]),
                ],
            )],
        )];

        Dependencies { depends, uses }
    }
}

impl<'ast> ToTokens for StructDes<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let field_names = self
            .inner
            .inner
            .fields
            .iter()
            .map(|field| CGIdentifier { inner: &field.name });
        let field_des_methods = self
            .inner
            .inner
            .fields
            .iter()
            .map(|f| StructDesField {
                ty: CGTy { inner: &f.ty },
            });
        tokens.append_all(mquote!(rust r#"
            impl<'i> DeserializeBytes<'i> for #{self.inner.typename} {
                ȸtype Error = BufError;

                fn des_bytes<'di>(rdr: &'di mut Buf<'i>) -> Result<Self, Self::Error> {
                    Ok(#{self.inner.typename} {
                        #( #field_names : #field_des_methods ),*
                    })
                }
            }
        "#));
    }
}

impl<'ast> Depends for StructDes<'ast> {
    fn dependencies(&self) -> Dependencies {
        let depends = vec![Package::RustCrate(
            RustCrateSource::Crates("vhl-stdlib".to_string()),
            VersionReq::parse("0.1.0").unwrap(),
        )];
        use Import::*;
        let uses = vec![Submodule(
            "vhl_stdlib",
            vec![Submodule(
                "serdes",
                vec![
                    Submodule("traits", vec![Entity("DeserializeBytes")]),
                    Submodule("buf", vec![Entity("Buf"), EntityAs("Error", "BufError")]),
                ],
            )],
        )];

        Dependencies { depends, uses }
    }
}

#[cfg(test)]
mod test {
    use mquote::mquote;
    use mtoken::ToTokens;
    use parser::span::{SourceOrigin, SpanOrigin};
    use vhl::ast::file::Definition;

    #[test]
    fn struct_ser_buf() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast_parser = parser::ast::file::File::parse(vhl_input, origin).unwrap();
        let ast_core = vhl::ast::file::File::from_parser_ast(ast_parser);
        match &ast_core.items[0] {
            Definition::Struct(struct_def) => {
                let cg_struct_def = super::CGStructDef::new(struct_def);
                let cg_struct_serdes = super::StructSer {
                    inner: cg_struct_def,
                };
                let ts = mquote!(rust r#" #cg_struct_serdes "#);

                let ts_should_be = mquote!(rust r#"
                    impl SerializeBytes for Point {
                        ȸtype Error = BufError;

                        fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error> {
                            wr.put_u16_le(self.x)?;
                            wr.put_u16_le(self.y)?;
                            Ok(())
                        }

                        fn len_bytes(&self) -> usize {
                            4
                        }
                    }
                "#);

                assert_eq!(format!("{}", ts), format!("{}", ts_should_be));
            }
            _ => panic!("Expected struct definition")
        }
    }

    #[test]
    fn struct_des_buf() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast_parser = parser::ast::file::File::parse(vhl_input, origin).unwrap();
        let ast_core = vhl::ast::file::File::from_parser_ast(ast_parser);
        match &ast_core.items[0] {
            Definition::Struct(struct_def) => {
                let cg_struct_def = super::CGStructDef::new(struct_def);
                let cg_struct_serdes = super::StructDes {
                    inner: cg_struct_def,
                };
                let ts = mquote!(rust r#" #cg_struct_serdes "#);

                let ts_should_be = mquote!(rust r#"
                    impl<'i> DeserializeBytes<'i> for Point {
                        ȸtype Error = BufError;

                        fn des_bytes<'di>(rdr: &'di mut Buf<'i>) -> Result<Self, Self::Error> {
                            Ok(Point {
                                x: rdr.get_u16_le()?,
                                y: rdr.get_u16_le()?
                            })
                        }
                    }
                "#);
                assert_eq!(format!("{}", ts), format!("{}", ts_should_be));
            }
            _ => panic!("Expected struct definition")
        }
    }
}
