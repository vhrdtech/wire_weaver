use crate::prelude::*;
use vhl::ast::xpi_def::{XpiDef, XpiKind};
use crate::dependencies::{Dependencies, Depends};
use crate::rust::path::PathCG;
use itertools::Itertools;

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
                Self::dispatch_method(xpi_def).map_err(|e| e.add_context("dispatch call"))?
            }
            _ => {
                mquote!(rust r#" Err(FailReason::NotAMethod) "#)
            }
        };

        let (no_methods_inside, no_methods_inside_names): (Vec<u32>, Vec<String>) = xpi_def.children
            .iter()
            .filter(|c| !c.contains_methods())
            .map(|c| (c.serial, format!("{}", c.uri)))
            .unzip();

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
                            format!("{}{}", uri_base, c.uri),
                        )?
                    );
                    Ok(prev)
                },
            )?;
        let child_serials = child_serials.into_iter();

        let no_methods_inside = if no_methods_inside.is_empty() {
            TokenStream::new()
        } else {
            let comment = Itertools::intersperse(
                no_methods_inside_names.iter().map(|i| format!("{}{}", uri_base, i)),
                ",".to_owned(),
            );
            let no_methods_inside = no_methods_inside.iter();
            mquote!(rust r#"
                ⏎/◡/ #( #comment )* : not a method and contains no children that are ⏎
                Some( #( #no_methods_inside )|* ) => Err(FailReason::NotAMethod),
            "#)
        };

        let wildcard_comment = if xpi_def.children.is_empty() {
            "has no child resources"
        } else {
            "all defined resources are handled"
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
                ⏎/◡/ #uri_base : #wildcard_comment⏎
                Some(_) => Err(FailReason::BadUri),
            }
        "#))
    }

    fn dispatch_method(xpi_def: &XpiDef) -> Result<TokenStream, CodegenError> {
        // println!("attrs: {:?}", xpi_def.attrs);
        let dispatch = xpi_def.attrs.get_unique(vec!["dispatch"])?;
        let expr = dispatch.expect_expr()?;
        let (kind, args) = expr.expect_call()?;
        // let flavor = args.0[0].expect_ident()?.symbols.clone();
        let path = args.0[0].expect_path()?;
        let path = PathCG { inner: &path };
        let kind = kind.symbols.clone();
        match kind.as_str() {
            "sync" => {
                Ok(mquote!(rust r#"
                    // syncronous call
                    #path();
                    Ok(0)
                "#))
            }
            "rtic" => {
                Ok(mquote!(rust r#"
                    // rtic spawn, TODO: count spawn errors
                    #path::spawn();
                    Ok(0)
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