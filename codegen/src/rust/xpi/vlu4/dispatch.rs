use vhl::ast::fn_def::FnArguments;
use vhl::ast::ty::Ty;
use crate::prelude::*;
use vhl::ast::xpi_def::{XpiDef, XpiKind, XpiRootDef};
use crate::dependencies::{Dependencies, Depends};

pub struct DispatchCall<'ast> {
    pub xpi_root_def: &'ast XpiRootDef,
}

impl<'ast> ToTokens for DispatchCall<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let handle_methods = Self::handle_methods(
            &XpiKind::Group,
            &self.xpi_root_def.children,
            format!("/{:-}", self.xpi_root_def.id),
        );
        tokens.append_all(mquote!(rust r#"
            /// Dispatches a method call to a resource identified by uri.
            fn dispatch_call(mut uri: UriIter, call_type: DispatchCallType) -> Result<usize, FailReason>
            {
                use DispatchCallType::*;
                log_info◡!◡(=>T, "dispatch_call({})", uri);

                #handle_methods
            }
        "#));
    }
}

impl<'ast> DispatchCall<'ast> {
    fn handle_methods(self_kind: &XpiKind, children: &Vec<XpiDef>, uri_base: String) -> TokenStream {
        let self_method = match self_kind {
            XpiKind::Method { args, ret_ty } => {
                Self::dispatch_one(args, ret_ty)
            }
            _ => {
                mquote!(rust r#" return Err(FailReason::NotAMethod); "#)
            }
        };

        let no_methods_inside: Vec<u32> = children
            .iter()
            .filter(|c| !c.contains_methods())
            .map(|c| c.serial)
            .collect();

        let (child_serial, child_handle_methods): (Vec<u32>, Vec<TokenStream>) = children
            .iter()
            .filter(|c| !no_methods_inside.contains(&c.serial))
            .map(|c| (
                c.serial,
                Self::handle_methods(
                    &c.kind,
                    &c.children,
                    format!("{}/{}", uri_base, c.serial),
                )
            ))
            .unzip();
        // let (child_serial, child_handle_methods) = (child_serial.into_iter(), child_handle_methods.into_iter());

        let no_methods_inside = if no_methods_inside.is_empty() {
            TokenStream::new()
        } else {
            mquote!(rust r#"
                Some( #( #no_methods_inside )|* ) => {
                    return Err(FailReason::NotAMethod);
                }
            "#)
        };

        mquote!(rust r#"
            match uri.next() {
                /◡/ dispatch #uri_base◡()⏎
                None => {
                    #self_method
                }
                #(
                    /◡/ syntetic ⏎
                    // regular
                    Some \\( #child_serial \\) => \\{ #child_handle_methods \\}
                )*
                #no_methods_inside
                Some(_) => {
                     return Err(FailReason::BadUri);
                }
            }
        "#)
    }

    fn dispatch_one(args: &FnArguments, ret_ty: &Ty) -> TokenStream {
        mquote!(rust r#" dispatch_one_here "#)
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