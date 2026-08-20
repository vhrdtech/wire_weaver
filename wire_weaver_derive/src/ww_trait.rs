use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{TokenStreamExt, quote};
use std::path::PathBuf;
use syn::{
    Error, FnArg, GenericArgument, ItemTrait, Lifetime, PathArguments, ReturnType, Type, TypePath,
    parse2,
};
use wire_weaver_core::internal::{PropertyMacroArgs, StreamAndImplMacroArgs};
use wire_weaver_core::load;

pub fn ww_trait(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_trait_inner(attr, item)
        .unwrap_or_else(|e| Error::new(Span::call_site(), e).to_compile_error())
}

fn ww_trait_inner(attr: TokenStream, item: TokenStream) -> Result<TokenStream, String> {
    let item_trait: ItemTrait = syn::parse2(item).map_err(|e| format!("{e:?}"))?;
    let trait_name = item_trait.ident.to_string();
    let crate_path = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("env variable CARGO_MANIFEST_DIR should be set"),
    );
    // Parse fully to catch errors early - in API crates, not in user ones that depend on them.
    let _api_level =
        load(&crate_path, Some(trait_name.clone()), false).map_err(|e| format!("{e:?}"))?;

    // Generate use statements with correct spans so that IDEs help on hover and go to definition features work.
    // Also this will pick up errors if lifetime is absent from a type that requires it, in defining crate and not when user depends on it.
    let mut helper_spans = TokenStream::new();
    let mut unique_marker = MarkerUniqueName::new(trait_name);
    for item in &item_trait.items {
        match item {
            syn::TraitItem::Fn(item_fn) => {
                let marker = unique_marker.next_ty();
                let inputs = &item_fn.sig.inputs;
                let mut lifetimes = vec![];
                for arg in inputs {
                    let FnArg::Typed(pat_type) = arg else {
                        continue;
                    };
                    collect_lifetimes(&pat_type.ty, &mut lifetimes);
                }
                let output = &item_fn.sig.output;
                if let ReturnType::Type(_, ty) = output {
                    collect_lifetimes(ty, &mut lifetimes);
                }
                lifetimes.dedup_by(|a, b| a.ident == b.ident);
                let maybe_lifetimes = if !lifetimes.is_empty() {
                    quote! { < #(#lifetimes),* >}
                } else {
                    quote! {}
                };
                helper_spans.append_all(quote! {
                    type #marker #maybe_lifetimes = fn(#inputs) #output;
                });
            }
            syn::TraitItem::Macro(item_macro) => {
                let kind = item_macro.mac.path.get_ident().unwrap().to_string();
                match kind.as_str() {
                    "stream" | "sink" => {
                        let args: StreamAndImplMacroArgs =
                            parse2(item_macro.mac.tokens.clone()).unwrap();
                        let marker = type_marker(&args.type_or_trait, &mut unique_marker);
                        helper_spans.append_all(marker);
                    }
                    "property" => {
                        let args: PropertyMacroArgs =
                            parse2(item_macro.mac.tokens.clone()).unwrap();
                        let property_ty_marker = type_marker(&args.ty, &mut unique_marker);
                        let error_ty_marker = args
                            .write_err_ty
                            .as_ref()
                            .map(|ty| type_path_marker(ty, &mut unique_marker));
                        helper_spans.append_all(quote! {
                            #property_ty_marker
                            #error_ty_marker
                        });
                    }
                    "ww_impl" => {
                        let args: StreamAndImplMacroArgs =
                            parse2(item_macro.mac.tokens.clone()).unwrap();
                        let ext_trait = args.type_or_trait;
                        let Type::Path(mut type_path) = ext_trait else {
                            continue;
                        };
                        let len = type_path.path.segments.len();
                        if len >= 1 {
                            // assuming ww_impl!(name: ext_crate::TraitName);
                            // generate use ext_crate::NAME_FULL_GID as SRC_MARKER_X;
                            let last = type_path.path.segments.last_mut().unwrap();
                            last.ident = Ident::new(
                                format!("{}_FULL_GID", last.ident)
                                    .to_case(Case::Constant)
                                    .as_str(),
                                last.ident.span(),
                            );
                            let marker = unique_marker.next_const();
                            helper_spans.append_all(quote! {
                                #[allow(unused_imports)]
                                use #type_path as #marker;
                            });
                        } else {
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // let mut check_types_lifetimes = TokenStream::new();
    // for (ty, lifetime) in api_level.external_types() {
    //     let check_name = if lifetime {
    //         "ShouldHaveALifetime"
    //     } else {
    //         "ShouldNotHaveALifetime"
    //     };
    //     let Some(last) = ty.segments.last() else {
    //         continue;
    //     };
    //     let check_name = Ident::new(
    //         format!("_{trait_name}{last}{check_name}",).as_str(),
    //         last.span(),
    //     );
    //     let ty = &ty;
    //     if lifetime {
    //         check_types_lifetimes.extend(quote! {
    //             type #check_name<'i> = #ty<'i>;
    //         });
    //     } else {
    //         check_types_lifetimes.extend(quote! {
    //             type #check_name = #ty;
    //         });
    //     }
    // }

    let docs: Vec<_> = item_trait
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .collect();
    let full_gid = Ident::new(
        format!("{}_FULL_GID", item_trait.ident)
            .to_case(Case::Constant)
            .as_str(),
        item_trait.ident.span(),
    );
    let compact_gid = Ident::new(
        format!("{}_COMPACT_GID", item_trait.ident)
            .to_case(Case::Constant)
            .as_str(),
        item_trait.ident.span(),
    );
    // let trait_name = item_trait.ident;
    Ok(quote! {
        #(#docs)*
        pub const #full_gid: ww_version::FullVersion = wire_weaver::full_version!();
        pub const #compact_gid: Option<ww_version::CompactVersion> = wire_weaver::compact_version!(#attr);
        // pub type #trait_name = ();
        // #check_types_lifetimes
        #helper_spans
    })
}

struct MarkerUniqueName {
    disambiguator: String,
    idx: usize,
}

impl MarkerUniqueName {
    fn new(disambiguator: String) -> Self {
        Self {
            disambiguator,
            idx: 0,
        }
    }

    fn next_const(&mut self) -> Ident {
        let idx = self.idx;
        self.idx += 1;
        Ident::new(
            &format!("SRC_MARKER_{}_{idx}", self.disambiguator).to_case(Case::Constant),
            Span::call_site(),
        )
    }

    fn next_ty(&mut self) -> Ident {
        let idx = self.idx;
        self.idx += 1;
        Ident::new(
            &format!("SrcMarker{}_{idx}", self.disambiguator).to_case(Case::Pascal),
            Span::call_site(),
        )
    }
}

fn type_marker(ty: &Type, unique_marker: &mut MarkerUniqueName) -> TokenStream {
    let marker = unique_marker.next_ty();
    let maybe_args = match &ty {
        Type::Path(type_path) => {
            let args = type_path.path.segments.last().unwrap().arguments.clone();
            Some(quote! { #args })
        }
        Type::Reference(type_reference) => {
            if let Some(lifetime) = &type_reference.lifetime {
                Some(quote! { < #lifetime > })
            } else {
                None
            }
        }
        _ => None,
    };
    quote! {
        // to not try to figure out generic argument vs actual type, in 'type SrcMarkerAbc0<u8> = Vec<u8>;' or RefVec<'i, u8>, etc.
        #[allow(non_camel_case_types)]
        type #marker #maybe_args = #ty;
    }
}

fn type_path_marker(type_path: &TypePath, unique_marker: &mut MarkerUniqueName) -> TokenStream {
    let marker = unique_marker.next_ty();
    let args = type_path.path.segments.last().unwrap().arguments.clone();
    quote! {
        type #marker #args = #type_path;
    }
}

fn collect_lifetimes(ty: &Type, lifetimes: &mut Vec<Lifetime>) {
    match ty {
        Type::Path(type_path) => {
            if let Some(last) = type_path.path.segments.last() {
                if let PathArguments::AngleBracketed(angle) = &last.arguments {
                    for arg in &angle.args {
                        match arg {
                            GenericArgument::Lifetime(lifetime) => {
                                lifetimes.push(lifetime.clone());
                            }
                            GenericArgument::Type(ty) => collect_lifetimes(ty, lifetimes),
                            _ => {}
                        }
                        if let GenericArgument::Lifetime(lifetime) = arg {
                            lifetimes.push(lifetime.clone());
                        }
                    }
                }
            }
        }
        Type::Reference(type_reference) => {
            if let Some(lifetime) = &type_reference.lifetime {
                lifetimes.push(lifetime.clone());
            }
        }
        Type::Slice(type_slice) => {
            collect_lifetimes(&type_slice.elem, lifetimes);
        }
        Type::Tuple(type_tuple) => {
            for ty in &type_tuple.elems {
                collect_lifetimes(ty, lifetimes);
            }
        }
        _ => {}
    }
}
