use crate::ast::trait_macro_args::ImplTraitMacroArgs;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use shrink_wrap_core::ast::path::Path;
use shrink_wrap_core::ast::{Docs, Type};
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
    Reserved,
}

#[derive(Debug)]
pub struct Argument {
    pub ident: Ident,
    pub ty: Type,
}

#[derive(Debug, Copy, Clone)]
pub enum PropertyAccess {
    /// Property is not going to change, observe not available
    Const,
    /// Property can only be read, but can change and be observed for changes
    ReadOnly,
    /// Property can be read, written and observed for changes
    ReadWrite,
    /// Property can only be written
    WriteOnly,
}

impl ApiLevel {
    pub fn mod_ident(&self, crate_name: &Ident) -> Ident {
        Ident::new(
            format!(
                "{}_{}",
                crate_name,
                self.name.to_string().to_case(Case::Snake)
            )
            .as_str(),
            crate_name.span(),
        )
    }

    pub fn client_struct_name(&self, crate_name: &Ident) -> Ident {
        let mod_name = self.mod_ident(crate_name);
        Ident::new(
            format!("{}_client", mod_name)
                .to_case(Case::Pascal)
                .as_str(),
            mod_name.span(),
        )
    }

    pub fn stream_ser_struct_name(&self, crate_name: &Ident) -> Ident {
        let mod_name = self.mod_ident(crate_name);
        Ident::new(
            format!("{}_stream_serializer", mod_name)
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
                ApiItemKind::Reserved => {}
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

    pub fn full_gid(&self) -> Ident {
        Ident::new(
            format!("{}_FULL_GID", self.name)
                .to_case(Case::Constant)
                .as_str(),
            self.name.span(),
        )
    }

    pub fn compact_gid(&self) -> Ident {
        Ident::new(
            format!("{}_COMPACT_GID", self.name)
                .to_case(Case::Constant)
                .as_str(),
            self.name.span(),
        )
    }

    pub fn gid_paths(&self) -> (TokenStream, TokenStream) {
        let crate_name = self.source_location.crate_name();
        let full_gid = self.full_gid();
        let compact_gid = self.compact_gid();
        let full = quote! { #crate_name::#full_gid };
        let compact = quote! { #crate_name::#compact_gid };
        (full, compact)
    }

    pub fn make_owned(&mut self) {
        for item in &mut self.items {
            match &mut item.kind {
                ApiItemKind::Method {
                    args, return_type, ..
                } => {
                    for arg in args {
                        arg.ty.make_owned();
                    }
                    if let Some(return_type) = return_type {
                        return_type.make_owned();
                    }
                }
                ApiItemKind::Property { ty, .. } => {
                    ty.make_owned();
                }
                ApiItemKind::Stream { ty, .. } => {
                    ty.make_owned();
                }
                ApiItemKind::ImplTrait { level, .. } => {
                    if let Some(level) = level {
                        level.make_owned();
                    }
                }
                ApiItemKind::Reserved => {}
            }
        }
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

impl ApiLevelSourceLocation {
    pub fn crate_name(&self) -> &Ident {
        match self {
            ApiLevelSourceLocation::File { part_of_crate, .. } => part_of_crate,
            ApiLevelSourceLocation::Crate { crate_name, .. } => crate_name,
        }
    }
}
