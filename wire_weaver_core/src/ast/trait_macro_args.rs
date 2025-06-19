use proc_macro2::Ident;
use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Token};

/// Used inside `ww_trait!` to define a stream or a sink: `stream!(stream_name: StreamTy);`
pub struct StreamMacroArgs {
    pub ident: Ident,
    _colon: Token![:],
    pub ty: syn::Type,
}

impl Parse for StreamMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(StreamMacroArgs {
            ident: input.parse()?,
            _colon: input.parse()?,
            ty: input.parse()?,
        })
    }
}

/// Used inside `ww_trait!` to refer to another `ww_trait` and generate a trait_name_x_process_inner handler and a call to.
/// * In the same file: `ww_impl!(resource_name: TraitName)`
/// * In another file: `ww_impl!(resource_name: "../path/to/file.rs"::TraitName)`
/// * Inside a crate lib.rs: `ww_impl!(resource_name: "crate_name:x.y"::TraitName)`
#[derive(Debug)]
pub struct ImplTraitMacroArgs {
    pub resource_name: Ident,
    _colon: Token![:],
    pub location: ImplTraitLocation,
    _colon_colon: Token![::],
    pub trait_name: Ident,
}

impl Parse for ImplTraitMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let resource_name = input.parse()?;
        let _colon: Token![:] = input.parse()?;
        let location = input.parse()?;
        if !matches!(location, ImplTraitLocation::SameFile) {
            let _colon_colon: Token![::] = input.parse()?;
        }
        Ok(ImplTraitMacroArgs {
            resource_name,
            _colon: Default::default(),
            location,
            _colon_colon: Default::default(),
            trait_name: input.parse()?,
        })
    }
}

#[derive(Debug)]
pub enum ImplTraitLocation {
    SameFile,
    AnotherFile {
        path: String,
        part_of_crate: Ident,
    },
    CratesIo {
        crate_name: String,
        major: u32,
        minor: u32,
    },
}

impl Parse for ImplTraitLocation {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(syn::LitStr) {
            let location: LitStr = input.parse()?;
            // TODO: parse crates.io location
            // } else {
            //     let trait_name_version = trait_source_str.split(':').collect::<Vec<&str>>();
            //     if trait_name_version.len() != 2 {
            //         return Error::custom("Expected crates.io \"crate_name:x.y.z\" or \"./path/to/src.ww\" or \"../path/to/src.ww\"")
            //             .with_span(&args.location.span())
            //             .write_errors();
            //     }
            //     let _crate_name = trait_name_version[0];
            //     let _version = trait_name_version[1];
            //     return Error::custom(format!(
            //         "crates.io loading is not supported yet {_crate_name} {_version}"
            //     ))
            //     .write_errors();
            // }
            let _: Token![as] = input.parse()?;
            let crate_name = input.parse()?;
            Ok(ImplTraitLocation::AnotherFile {
                path: location.value(),
                part_of_crate: crate_name,
            })
        } else {
            Ok(ImplTraitLocation::SameFile)
        }
    }
}
