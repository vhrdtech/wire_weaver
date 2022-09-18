use thiserror::Error;

#[derive(Error, Debug)]
pub enum CodegenError {
    #[error("Error originating in core module")]
    Core(#[from] vhl::error::Error),
    #[error("Attribute with an expression was expected, not token tree")]
    ExpectedExprAttribute,
    #[error("Attribute provided has unexpected syntax, note: {}", .0)]
    WrongAttributeSyntax(String),
}
