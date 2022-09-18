use mtoken::TokenStream;

pub mod error;
// pub mod multilang;
pub mod ast_wrappers;
pub mod dart;
pub mod dependencies;
pub mod file;
pub mod rust;

pub trait Codegen {
    type Error;

    fn codegen(&self) -> Result<TokenStream, Self::Error>;
}

pub mod prelude {
    pub use mquote::mquote;
    pub use mtoken::ext::TokenStreamExt;
    pub use mtoken::token::IdentFlavor;
    pub use mtoken::{ToTokens, TokenStream};
    pub use std::rc::Rc;
    pub use vhl::ast::identifier::IdentifierContext;
    pub use crate::dependencies::{Dependencies, Depends, Import, Package, RustCrateSource};
    pub use semver::VersionReq;
    pub use super::Codegen;
    pub use super::error::CodegenError;
}
