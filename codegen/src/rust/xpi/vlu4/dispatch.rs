use crate::prelude::*;
use vhl::ast::xpi_def::XpiRootDef;
use crate::dependencies::{Dependencies, Depends};

pub struct DispatchCall<'ast> {
    pub xpi_def: &'ast XpiRootDef
}

impl<'ast> ToTokens for DispatchCall<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {

        tokens.append_all(mquote!(rust r#"
            fn dispatch_call(mut uri: UriIter, call_type: DispatchCallType) -> Result<usize, FailReason>
            {
                use DispatchCallType::*;

                log_info!(=>T, "dispatch_call({})", uri);
                let at_root_level = match uri.next() {
                    Some(p) => p,
                    None => {
                        log_error!(=>T, "Expected root level");
                        return Err(FailReason::BadUri);
                    }
                };
                match at_root_level {
                    _ => {
                        log_error!(=>T, "Resource /{} doesn't exist", at_root_level);
                        Err(FailReason::BadUri)
                    }
                }
            }
        "#));
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