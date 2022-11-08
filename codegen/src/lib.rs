use crate::file::CGPiece;

pub mod error;
// pub mod multilang;
pub mod ast_wrappers;
pub mod dart;
pub mod dependencies;
pub mod file;
pub mod rust;

pub trait Codegen {
    type Error;

    fn codegen(&self) -> Result<CGPiece, Self::Error>;
}

pub mod prelude {
    pub use super::error::CodegenError;
    pub use super::Codegen;
    pub use crate::dependencies::{Dependencies, Depends, Import, Package, RustCrateSource};
    pub use ast::identifier::IdentifierContext;
    pub use mquote::mquote;
    pub use mtoken::ext::TokenStreamExt;
    pub use mtoken::token::IdentFlavor;
    pub use mtoken::{ToTokens, TokenStream};
    pub use semver::VersionReq;
    pub use std::rc::Rc;
}
