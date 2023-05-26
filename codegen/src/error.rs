pub enum CodegenError {
    // #[error("Error originating in vhl_core module")]
    Core(vhl_core::user_error::UserError),
    CoreInternal(vhl_core::error::Error),
    Ast(ast::Error),
    // #[error("{}, context: {}", .0, .1)]
    CoreWithContext(vhl_core::user_error::UserError, String),
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

impl From<vhl_core::user_error::UserError> for CodegenError {
    fn from(e: vhl_core::user_error::UserError) -> Self {
        CodegenError::Core(e)
    }
}

impl From<vhl_core::error::Error> for CodegenError {
    fn from(e: vhl_core::error::Error) -> Self {
        CodegenError::CoreInternal(e)
    }
}

impl From<ast::Error> for CodegenError {
    fn from(e: ast::Error) -> Self {
        CodegenError::Ast(e)
    }
}

impl CodegenError {
    pub fn core_with_context(
        core_err: vhl_core::user_error::UserError,
        context: &'static str,
    ) -> Self {
        Self::CoreWithContext(core_err, context.to_owned())
    }

    pub fn add_context(self, context: &'static str) -> Self {
        match self {
            CodegenError::Core(e) => CodegenError::CoreWithContext(e, context.to_owned()),
            e => e,
        }
    }

    pub fn print_report(&self) {
        match self {
            CodegenError::Core(core_err) => {
                println!("Core user error: {:?}", core_err.report());
            }
            CodegenError::CoreInternal(core_internal) => {
                println!("Core internal error: {:?}", core_internal)
            }
            CodegenError::CoreWithContext(core_err, context) => {
                println!("{}: {:?}", context, core_err.report())
            }
            CodegenError::Ast(ast_err) => {
                println!("AST error: {:?}", ast_err)
            }
            CodegenError::Internal(description) => println!("Internal: {}", description),
            CodegenError::Dispatch(description) => println!("Dispatch: {}", description),
            CodegenError::ExpectedExprAttribute => println!("ExpectedExprAttribute"),
            CodegenError::WrongAttributeSyntax(note) => println!("WrongAttributeSyntax: {}", note),
            CodegenError::UnsupportedDispatchType(note) => {
                println!("UnsupportedDispatchType: {}", note)
            }
        }
    }
}
