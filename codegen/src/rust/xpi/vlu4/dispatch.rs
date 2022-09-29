use crate::prelude::*;
use vhl::ast::xpi_def::{XpiDef, XpiKind};
use crate::dependencies::{Dependencies, Depends};
use crate::rust::path::PathCG;
use itertools::Itertools;
use vhl::ast::fn_def::FnArguments;
use vhl::ast::ty::{Ty, TyKind};
use crate::rust::identifier::CGIdentifier;
use crate::rust::serdes::buf::size::size_in_buf;
use crate::rust::serdes::buf::struct_def::{StructDesField, StructSerField};
use crate::rust::serdes::size::SerDesSizeCG;
use crate::rust::ty::CGTy;

pub struct DispatchCall<'ast> {
    pub xpi_def: &'ast XpiDef,
}

impl<'ast> Codegen for DispatchCall<'ast> {
    type Error = CodegenError;

    fn codegen(&self) -> Result<TokenStream, Self::Error> {
        let mut tokens = TokenStream::new();
        let handle_methods = Self::handle_methods(
            &self.xpi_def,
            format!("{}", self.xpi_def.uri_segment),
        )?;
        tokens.append_all(mquote!(rust r#"
            /// Dispatches a method call to a resource identified by uri.
            fn dispatch_call(mut uri: UriIter, call_type: DispatchCallType) -> Result<SerDesSize, FailReason>
            {
                use DispatchCallType::*;
                log_info◡!◡(=>T, "dispatch_call({})", uri);

                Λhandle_methods
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

        let (not_methods_serials, not_methods_names): (Vec<u32>, Vec<String>) = xpi_def.children
            .iter()
            .filter(|c| !c.contains_methods())
            .map(|c| (c.serial, format!("{}", c.uri_segment)))
            .unzip();

        let (child_serials, child_handle_methods) = xpi_def
            .children
            .iter()
            .filter(|c| !not_methods_serials.contains(&c.serial))
            .try_fold::<_, _, Result<_, CodegenError>>(
                (vec![], vec![]),
                |mut prev, c| {
                    prev.0.push(c.serial);
                    prev.1.push(
                        Self::handle_methods(
                            c,
                            format!("{}{}", uri_base, c.uri_segment),
                        )?
                    );
                    Ok(prev)
                },
            )?;

        let no_methods_recursively = if not_methods_serials.is_empty() {
            TokenStream::new()
        } else {
            mquote!(rust r#"
                ⏎/◡/ ⸨ ∀not_methods_names ⸩,* : not a method and contains no children that are ⏎
                Some( ⸨ ∀not_methods_serials ⸩|* ) => Err(FailReason::NotAMethod),
            "#)
        };

        let wildcard_comment = if xpi_def.children.is_empty() {
            "has no child resources"
        } else {
            "all defined resources are handled"
        };

        Ok(mquote!(rust r#"
            match uri.next() {
                /◡/ dispatch Λuri_base◡()⏎
                None => {
                    Λself_method
                }
                ⸨ Some ( ∀child_serials ) => { ∀child_handle_methods } ⸩*
                Λno_methods_recursively
                ⏎/◡/ Λuri_base : Λwildcard_comment⏎
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

        let (args, ret_ty) = xpi_def.expect_method_kind().expect("dispatch_method() must be called only for methods");
        let (des_args, arg_names) = Self::des_args_buf_reader(&args)?;
        let (ser_ret_stmt, ser_ret_buf) = Self::ser_ret_buf_writer(&ret_ty)?;
        let ret_ty_size = SerDesSizeCG { inner: size_in_buf(&ret_ty) };

        let real_run = match kind.as_str() {
            "sync" => {
                Ok(mquote!(rust r#"
                    // syncronous call
                    Λdes_args
                    Λser_ret_stmt Λpath(Λarg_names);
                    Λser_ret_buf
                "#))
            }
            "rtic" => {
                Ok(mquote!(rust r#"
                    // rtic spawn, TODO: count spawn errors
                    Λdes_args
                    Λser_ret_stmt Λpath::spawn(Λarg_names);
                    Λser_ret_buf
                "#))
            }
            k => {
                Err(CodegenError::UnsupportedDispatchType(k.to_owned()))
            }
        }?;
        Ok(mquote!(rust r#"
            match call_type {
                DispatchCallType::DryRun => {
                    Ok(Λret_ty_size)
                }
                DispatchCallType::RealRun(args, result) => {
                    Λreal_run
                }
            }
        "#))
    }

    fn des_args_buf_reader(args: &FnArguments) -> Result<(TokenStream, TokenStream), CodegenError> {
        let arg_names = args
            .args
            .iter()
            .map(|arg| CGIdentifier { inner: &arg.name });
        let arg_des_methods = args
            .args
            .iter()
            .map(|f| StructDesField {
                ty: CGTy { inner: &f.ty },
            });
        let arg_names_clone = arg_names.clone();
        Ok((mquote!(rust r#"
            let mut rd = Buf::new(args);
            ⸨ let ∀arg_names = rd.∀arg_des_methods()?; ⸩*
            if !rd.is_at_end() {
                log_warn◡!◡(=>T, "Unused {} bytes left after deserializing arguments", rd.bytes_left());
            }
        "#),
            mquote!(rust r#" ⸨ ∀arg_names_clone ⸩,* "#)
        ))
    }

    fn ser_ret_buf_writer(ret_ty: &Ty) -> Result<(TokenStream, TokenStream), CodegenError> {
        if ret_ty.kind == TyKind::Unit {
            Ok((TokenStream::new(), mquote!(rust r#" Ok(0) "#)))
        } else {
            let ser_method = StructSerField { ty: CGTy { inner: &ret_ty } };
            Ok((mquote!(rust r#" let ret = "#), mquote!(rust r#"
                let mut wr = BufMut::new(result);
                wr.Λser_method(ret)?;
                Ok(wr.bytes_pos())
            "#)))
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
                    Submodule("xwfd", vec![
                        Submodule("uri", vec![
                            Entity("UriIter")
                        ]),
                        Submodule("error", vec![
                            Entity("FailReason")
                        ]),
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