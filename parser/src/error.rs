#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub rule: crate::lexer::Rule,
    pub span: (usize, usize)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ParseErrorKind {
    InternalError,
    UnhandledUnexpectedInput,
    UserError,

    AutonumWrongForm,
    AutonumWrongArguments,
    FloatParseError,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseErrorSource {
    /// Parser internal error, for example if a feature is not implemented.
    /// unreachable() and unwrap()'s are converted into this error as well.
    /// Will be pushed onto error list in `ast/file.rs`, so that no errors are silently ignored.
    /// More precise errors might be pushed onto the same list by parsers.
    InternalError,
    /// Not enough input or unexpected rule (because expected one is absent).
    /// Might not be an error like in enum with only discriminant values.
    /// The only error to be ignored by `parse_or_skip()`, so that parsing of the
    /// current node can continue.
    /// Will be pushed onto error list in `ast/file.rs` if not ignored along the way.
    UnexpectedInput,
    /// User provided erroneous input, invalid number for example.
    UserError
}

impl ParseErrorSource {
    pub fn is_internal(&self) -> bool {
        *self == ParseErrorSource::InternalError
    }
}