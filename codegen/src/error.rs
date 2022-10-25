use thiserror::Error;

#[derive(Error, Debug)]
pub enum CodegenError {
    // #[error("Error originating in core module")]
    // Core(#[from] vhl::error::Error),
    // #[error("{}, context: {}", .0, .1)]
    // CoreWithContext(vhl::error::Error, String),
    #[error("xPI dispatch code generator: {}", .0)]
    Dispatch(String),
    #[error("Attribute with an expression was expected, not token tree")]
    ExpectedExprAttribute,
    #[error("Attribute provided has unexpected syntax, note: {}", .0)]
    WrongAttributeSyntax(String),
    #[error("Unsupported dispatcher: '{}'", .0)]
    UnsupportedDispatchType(String),
}

impl CodegenError {
    // pub fn core_with_context(core_err: vhl::error::Error, context: &'static str) -> Self {
    //     Self::CoreWithContext(core_err, context.to_owned())
    // }
    //
    // pub fn add_context(self, context: &'static str) -> Self {
    //     match self {
    //         CodegenError::Core(e) => CodegenError::CoreWithContext(e, context.to_owned()),
    //         e => e
    //     }
    // }
}