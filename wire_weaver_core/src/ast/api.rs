use crate::ast::path::Path;
use crate::ast::trait_macro_args::ImplTraitMacroArgs;
use crate::ast::{Docs, Type};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug)]
pub struct ApiLevel {
    pub docs: Docs,
    pub name: Ident,
    pub source_location: ApiLevelSourceLocation,
    pub items: Vec<ApiItem>,
}

#[derive(Debug, Clone)]
pub enum ApiLevelSourceLocation {
    File {
        path: PathBuf,
        part_of_crate: Ident,
    },
    Crate {
        crate_name: Ident,
        major: u32,
        minor: u32,
    },
}

#[derive(Debug)]
pub struct ApiItem {
    pub id: u32,
    pub docs: Docs,
    pub multiplicity: Multiplicity,
    pub kind: ApiItemKind,
}

#[derive(Debug, Copy, Clone)]
pub enum Multiplicity {
    Flat,
    Array { size_bound: u32 },
}

#[derive(Debug)]
pub enum ApiItemKind {
    Method {
        ident: Ident,
        args: Vec<Argument>,
        return_type: Option<Type>,
    },
    Property {
        ident: Ident,
        ty: Type,
        access: PropertyAccess,
    },
    Stream {
        ident: Ident,
        ty: Type,
        is_up: bool,
    },
    ImplTrait {
        args: ImplTraitMacroArgs,
        level: Option<Box<ApiLevel>>,
    },
}

#[derive(Debug)]
pub struct Argument {
    pub ident: Ident,
    pub ty: Type,
}

#[derive(Debug, Copy, Clone)]
pub enum PropertyAccess {
    ReadOnly,
    ReadWrite,
    WriteOnly,
}

impl ApiLevel {
    pub fn mod_ident(&self, ext_crate_name: Option<&Ident>) -> Ident {
        if let Some(ext_crate_name) = ext_crate_name {
            Ident::new(
                format!(
                    "{}_{}",
                    ext_crate_name,
                    self.name.to_string().to_case(Case::Snake)
                )
                .as_str(),
                ext_crate_name.span(),
            )
        } else {
            Ident::new(
                self.name.to_string().to_case(Case::Snake).as_str(),
                Span::call_site(),
            )
        }
    }

    pub fn client_struct_name(&self, ext_crate_name: Option<&Ident>) -> Ident {
        let mod_name = self.mod_ident(ext_crate_name);
        Ident::new(
            format!("{}_client", mod_name)
                .to_case(Case::Pascal)
                .as_str(),
            mod_name.span(),
        )
    }

    pub fn external_types(&self) -> HashSet<(Path, bool)> {
        let mut ext_types = HashSet::new();
        for item in &self.items {
            match &item.kind {
                ApiItemKind::Method {
                    args, return_type, ..
                } => {
                    for arg in args {
                        arg.ty.visit_external_types(&mut |ext, lifetime| {
                            ext_types.insert((ext.clone(), lifetime));
                        });
                    }
                    if let Some(ty) = return_type {
                        ty.visit_external_types(&mut |ext, lifetime| {
                            ext_types.insert((ext.clone(), lifetime));
                        });
                    }
                }
                ApiItemKind::Property { ty, .. } | ApiItemKind::Stream { ty, .. } => {
                    ty.visit_external_types(&mut |ext, lifetime| {
                        ext_types.insert((ext.clone(), lifetime));
                    });
                }
                ApiItemKind::ImplTrait { .. } => {}
            }
        }
        ext_types
    }

    pub fn use_external_types(&self, parent: Path, no_alloc: bool) -> TokenStream {
        let ext_types = self.external_types();
        let mut ts = TokenStream::new();
        for (ext_ty, lifetime) in ext_types {
            if lifetime && !no_alloc {
                // use UserTypeOwned instead of UserType<'_>
                let mut ty_owned = ext_ty.clone();
                ty_owned.make_owned();
                use_ty(&parent, &ty_owned, &mut ts);
            } else {
                use_ty(&parent, &ext_ty, &mut ts);
            }
        }
        ts
    }
}

fn use_ty(parent: &Path, path: &Path, ts: &mut TokenStream) {
    if path.segments.len() == 1 {
        ts.extend(quote! {
            use #parent::#path;
        });
    } else {
        ts.extend(quote! {
            use #path;
        });
    }
}
