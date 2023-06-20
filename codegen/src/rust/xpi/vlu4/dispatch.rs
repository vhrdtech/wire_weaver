use crate::dependencies::Dependencies;
use crate::prelude::*;
use crate::rust::identifier::CGIdentifier;
use crate::rust::path::PathCG;
use crate::rust::serdes::buf::size::size_in_byte_buf;
use crate::rust::serdes::buf::struct_def::{StructDesField, StructSerField};
use crate::rust::serdes::size::SerDesSizeCG;
use crate::rust::ty::CGTy;
use crate::CGPiece;
use ast::xpi_def::XpiKind;
use ast::{make_path, FnArguments, Path, Span, Ty, TyKind};
use vhl_core::project::Project;

pub struct DispatchCall<'i> {
    pub project: &'i Project,
    pub xpi_def_path: Path,
}

impl<'i> Codegen for DispatchCall<'i> {
    type Error = CodegenError;

    fn codegen(&self) -> Result<CGPiece, Self::Error> {
        let mut piece = CGPiece {
            ts: TokenStream::new(),
            deps: dependencies(),
            from: Span::call_site(),
        };
        let handle_methods = Self::handle_methods(self.project, &self.xpi_def_path)?;
        piece.ts.append_all(mquote!(rust r#"
            /// Dispatches a method call to a resource identified by uri.
            fn dispatch_call(mut uri: UriIter, call_type: DispatchCallType) -> Result<SerDesSize, FailReason>
            {
                use DispatchCallType::*;
                log_info◡!◡(=>T, "dispatch_call({})", uri);

                Λhandle_methods
            }
        "#));
        Ok(piece)
    }
}

impl<'ast> DispatchCall<'ast> {
    fn handle_methods(project: &Project, xpi_def_path: &Path) -> Result<TokenStream, CodegenError> {
        let xpi_def = project.find_xpi_def(xpi_def_path.clone())?;
        let self_method = match &xpi_def.kind {
            XpiKind::Method { .. } => Self::dispatch_method(project, xpi_def_path)?,
            _ => {
                mquote!(rust r#" Err(FailReason::NotAMethod) "#)
            }
        };

        let (not_methods_serials, not_methods_names): (Vec<u32>, Vec<String>) = xpi_def
            .children
            .iter()
            .filter(|c| !c.contains_methods())
            .map(|c| (c.serial.unwrap_or(u32::MAX), format!("{}", c.uri_segment)))
            .unzip();

        let (child_serials, child_handle_methods) = xpi_def
            .children
            .iter()
            .filter(|c| !not_methods_serials.contains(&c.serial.unwrap_or(u32::MAX)))
            .try_fold::<_, _, Result<_, CodegenError>>((vec![], vec![]), |mut prev, c| {
                prev.0.push(c.serial.unwrap_or(u32::MAX));
                let mut path = xpi_def_path.clone();
                path.append(c.uri_segment.expect_resolved().unwrap());
                prev.1.push(Self::handle_methods(
                    project,
                    &path,
                    // c,
                    // format!("{}/{}", uri_base, c.uri_segment),
                    // project
                )?);
                Ok(prev)
            })?;

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

        let xpi_def_path = PathCG {
            inner: xpi_def_path,
        };
        Ok(mquote!(rust r#"
            match uri.next() {
                /◡/ dispatch Λxpi_def_path◡()⏎
                None => {
                    Λself_method
                }
                ⸨ Some ( ∀child_serials ) => { ∀child_handle_methods } ⸩*
                Λno_methods_recursively
                ⏎/◡/ Λxpi_def_path : Λwildcard_comment⏎
                Some(_) => Err(FailReason::BadUri),
            }
        "#))
    }

    fn dispatch_method(
        project: &Project,
        xpi_def_path: &Path,
    ) -> Result<TokenStream, CodegenError> {
        let xpi_def = project.find_xpi_def(xpi_def_path.clone())?;
        // println!("attrs: {}", xpi_def.attrs);
        let dispatch = xpi_def
            .attrs
            .get_unique(make_path!(dispatch))
            .ok_or_else(|| CodegenError::Dispatch("expected dispatch attr".to_owned()))?;
        let expr = dispatch
            .expect_expr()
            .ok_or_else(|| CodegenError::Dispatch("expected expr".to_owned()))?;
        let (kind, args) = expr
            .expect_call()
            .ok_or_else(|| CodegenError::Dispatch("expected call".to_owned()))?;
        // let flavor = args.0[0].expect_ident()?.symbols.clone();
        // println!("args0: {}", args.0[0]);
        let path = args.0[0]
            .expect_ref()
            .ok_or_else(|| CodegenError::Dispatch("expected path to user method".to_owned()))?;
        let path = PathCG { inner: &path };

        let (args, ret_ty) = xpi_def
            .expect_method_kind()
            .expect("dispatch_method() must be called only for methods");
        let (des_args, arg_names) = Self::des_args_buf_reader(&args)?;
        let (ser_ret_stmt, ser_ret_buf) = Self::ser_ret_buf_writer(&ret_ty)?;
        let ret_ty_size = SerDesSizeCG {
            inner: size_in_byte_buf(&ret_ty, xpi_def_path, project)?,
        };

        let real_run = match kind.as_string().as_str() {
            "sync_call" => Ok(mquote!(rust r#"
                    // syncronous call
                    Λdes_args
                    Λser_ret_stmt Λpath(Λarg_names);
                    Λser_ret_buf
                "#)),
            "rtic_spawn" => Ok(mquote!(rust r#"
                    // rtic spawn, TODO: count spawn errors
                    Λdes_args
                    Λser_ret_stmt Λpath::spawn(Λarg_names);
                    Λser_ret_buf
                "#)),
            k => Err(CodegenError::UnsupportedDispatchType(k.to_owned())),
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
        let arg_des_methods = args.args.iter().map(|f| StructDesField {
            ty: CGTy { inner: &f.ty },
        });
        let arg_names_clone = arg_names.clone();
        Ok((
            mquote!(rust r#"
            let mut rd = Buf::new(args);
            ⸨ let ∀arg_names = rd.∀arg_des_methods()?; ⸩*
            if !rd.is_at_end() {
                log_warn◡!◡(=>T, "Unused {} bytes left after deserializing arguments", rd.bytes_left());
            }
        "#),
            mquote!(rust r#" ⸨ ∀arg_names_clone ⸩,* "#),
        ))
    }

    fn ser_ret_buf_writer(ret_ty: &Ty) -> Result<(TokenStream, TokenStream), CodegenError> {
        if ret_ty.kind == TyKind::Unit {
            Ok((TokenStream::new(), mquote!(rust r#" Ok(0) "#)))
        } else {
            let ser_method = StructSerField {
                ty: CGTy { inner: ret_ty },
            };
            Ok((
                mquote!(rust r#" let ret = "#),
                mquote!(rust r#"
                let mut wr = BufMut::new(result);
                wr.Λser_method(ret)?;
                Ok(wr.bytes_pos())
            "#),
            ))
        }
    }
}

fn dependencies() -> Dependencies {
    let depends = vec![Package::RustCrate(
        RustCrateSource::Crates("vhl-stdlib-rust".to_string()),
        VersionReq::parse("0.1.0").unwrap(),
    )];

    use Import::*;
    let uses = vec![Submodule(
        "vhl_stdlib",
        vec![Submodule(
            "serdes",
            vec![
                Submodule(
                    "xwfd",
                    vec![
                        Submodule("uri", vec![Entity("UriIter")]),
                        Submodule("error", vec![Entity("FailReason")]),
                    ],
                ),
                Submodule(
                    "buf",
                    vec![
                        Entity("Buf"),
                        Entity("BufMut"),
                        EntityAs("Error", "BufError"),
                    ],
                ),
            ],
        )],
    )];

    Dependencies { depends, uses }
}
