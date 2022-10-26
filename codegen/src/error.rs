
pub enum CodegenError {
    // #[error("Error originating in core module")]
    Core(vhl::error::Error),
    // #[error("{}, context: {}", .0, .1)]
    CoreWithContext(vhl::error::Error, String),
    // #[error("Internal error: {}", .0)]
    Internal(String),
    // #[error("xPI dispatch code generator: {}", .0)]
    Dispatch(String),
    // #[error("Attribute with an expression was expected, not token tree")]
    ExpectedExprAttribute,
    // #[error("Attribute provided has unexpected syntax, note: {}", .0)]
    WrongAttributeSyntax(String),
    // #[error("Unsupported dispatcher: '{}'", .0)]
    UnsupportedDispatchType(String),
}

impl From<vhl::error::Error> for CodegenError {
    fn from(e: vhl::error::Error) -> Self {
        CodegenError::Core(e)
    }
}

impl CodegenError {
    pub fn core_with_context(core_err: vhl::error::Error, context: &'static str) -> Self {
        Self::CoreWithContext(core_err, context.to_owned())
    }

    pub fn add_context(self, context: &'static str) -> Self {
        match self {
            CodegenError::Core(e) => CodegenError::CoreWithContext(e, context.to_owned()),
            e => e
        }
    }

    pub fn print_report(&self) {
        match self {
            CodegenError::Core(core_err) => {
                println!("{:?}", core_err.report());
            }
            CodegenError::CoreWithContext(core_err, context) => {
                println!("{}: {:?}", context, core_err.report())
            }
            CodegenError::Internal(description) => println!("Internal: {}", description),
            CodegenError::Dispatch(description) => println!("Dispatch: {}", description),
            CodegenError::ExpectedExprAttribute => println!("ExpectedExprAttribute"),
            CodegenError::WrongAttributeSyntax(note) => println!("WrongAttributeSyntax: {}", note),
            CodegenError::UnsupportedDispatchType(note) => println!("UnsupportedDispatchType: {}", note),
        }
    }
}