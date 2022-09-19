use parser::ast::ops::BinaryOp;
use vhl::ast::expr::Expr;
use crate::prelude::*;
use vhl::ast::xpi_def::{XpiDef, XpiKind};
use crate::dependencies::{Dependencies, Depends};

pub struct DispatchCall<'ast> {
    pub xpi_def: &'ast XpiDef,
}

impl<'ast> Codegen for DispatchCall<'ast> {
    type Error = CodegenError;

    fn codegen(&self) -> Result<TokenStream, Self::Error> {
        let mut tokens = TokenStream::new();
        let handle_methods = Self::handle_methods(
            &self.xpi_def,
            format!("{}", self.xpi_def.uri),
        )?;
        tokens.append_all(mquote!(rust r#"
            /// Dispatches a method call to a resource identified by uri.
            fn dispatch_call(mut uri: UriIter, call_type: DispatchCallType) -> Result<usize, FailReason>
            {
                use DispatchCallType::*;
                log_info◡!◡(=>T, "dispatch_call({})", uri);

                #handle_methods
            }
        "#));
        Ok(tokens)
    }
}

impl<'ast> DispatchCall<'ast> {
    fn handle_methods(xpi_def: &XpiDef, uri_base: String) -> Result<TokenStream, CodegenError> {
        let self_method = match &xpi_def.kind {
            XpiKind::Method { .. } => {
                Self::dispatch_one(xpi_def).map_err(|e| e.add_context("dispatch call"))?
            }
            _ => {
                mquote!(rust r#" return Err(FailReason::NotAMethod); "#)
            }
        };

        let no_methods_inside: Vec<u32> = xpi_def.children
            .iter()
            .filter(|c| !c.contains_methods())
            .map(|c| c.serial)
            .collect();

        let (child_serials, child_handle_methods) = xpi_def
            .children
            .iter()
            .filter(|c| !no_methods_inside.contains(&c.serial))
            .try_fold::<_, _, Result<_, CodegenError>>(
                (vec![], vec![]),
                |mut prev, c| {
                    prev.0.push(c.serial);
                    prev.1.push(
                        Self::handle_methods(
                            c,
                            format!("{}/{}", uri_base, c.serial),
                        )?
                    );
                    Ok(prev)
                },
            )?;

        let no_methods_inside = if no_methods_inside.is_empty() {
            TokenStream::new()
        } else {
            mquote!(rust r#"
                Some( #( #no_methods_inside )|* ) => {
                    return Err(FailReason::NotAMethod);
                }
            "#)
        };

        Ok(mquote!(rust r#"
            match uri.next() {
                /◡/ dispatch #uri_base◡()⏎
                None => {
                    #self_method
                }
                #(
                    Some \\( #child_serials \\) => \\{ #child_handle_methods \\}
                )*
                #no_methods_inside
                Some(_) => {
                     return Err(FailReason::BadUri);
                }
            }
        "#))
    }

    fn dispatch_one(xpi_def: &XpiDef) -> Result<TokenStream, CodegenError> {
        // println!("attrs: {:?}", xpi_def.attrs);
        let dispatch = xpi_def.attrs.get_one(vec!["dispatch"])?;
        let expr = dispatch.expect_expr()?;
        let (kind, args) = expr.expect_call()?;
        let flavor = args.0[0].expect_ident()?.symbols.clone();
        let path = if let Expr::ConsB(BinaryOp::Path, cons) = &args.0[1] {
            "todo".to_owned()
        } else {
            return Err(CodegenError::WrongAttributeSyntax("expected second argument to be a path".to_owned()));
        };
        let kind = kind.symbols.clone();
        //println!("{} {} {}", kind, flavor, path);
        match kind.as_str() {
            "sync" => {
                Ok(mquote!(rust r#"
                    // sync
                    sync()
                "#))
            }
            "rtic" => {
                Ok(mquote!(rust r#"
                    // rtic spawn, TODO: count spawn errors
                    crate::app::task::spawn();
                "#))
            }
            k => {
                Err(CodegenError::UnsupportedDispatchType(k.to_owned()))
            }
        }
    }
}

impl<'ast> Depends for DispatchCall<'ast> {
    fn dependencies(&self) -> Dependencies {
        let depends = vec![Package::RustCrate(
            RustCrateSource::Crates("vhl-stdlib".to_string()),
            VersionReq::parse("0.1.0").unwrap(),
        )];

        use Import::*;
        let uses = vec![
            Submodule("vhl_stdlib", vec![
                Submodule("serdes", vec![
                    Submodule("xpi_vlu4", vec![
                        Submodule("uri", vec![
                            Entity("UriIter")
                        ]),
                        Submodule("error", vec![
                            Entity("FailReason")
                        ])
                    ]),
                    Submodule("buf", vec![
                        Entity("Buf"),
                        Entity("BufMut"),
                        EntityAs("Error", "BufError")
                    ]),
                ],
            )],
        )];

        Dependencies { depends, uses }
    }
}