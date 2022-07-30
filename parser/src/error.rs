// use std::backtrace::Backtrace;
use thiserror::Error;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub rule: crate::lexer::Rule,
    pub span: (usize, usize)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ParseErrorKind {
    InternalError,
    Unimplemented(&'static str),
    UnhandledUnexpectedInput,
    UserError,

    AutonumWrongForm,
    AutonumWrongArguments,
    FloatParseError,
    IntParseError,
}

#[derive(Error, Debug)]
pub enum ParseErrorSource {
    /// Parser internal error.
    /// unreachable() and unwrap()'s are converted into this error as well.
    /// Will be pushed onto error list in `ast/file.rs`, so that no errors are silently ignored.
    /// More precise errors might be pushed onto the same list by parsers.
    /// TODO: add auto link to github here
    #[error("Parser internal error, please file a bug is one doesn't yet exists.")]
    InternalError {
        // backtrace: Backtrace,
    },
    /// Parser feature unimplemented
    /// TODO: add link to feature status on github here
    #[error("Parser feature unimplemented, consider contributing or look at features status here: _")]
    Unimplemented(&'static str),
    /// Not enough input or unexpected rule (because expected one is absent).
    /// Might not be an error like in enum with only discriminant values.
    /// The only error to be ignored by `parse_or_skip()`, so that parsing of the
    /// current node can continue.
    /// Will be pushed onto error list in `ast/file.rs` if not ignored along the way.
    #[error("Not enough input or unexpected rule (because expected one is absent)")]
    UnexpectedInput,
    /// User provided erroneous input, invalid number for example.
    #[error("User provided erroneous input, invalid number for example")]
    UserError
}

impl ParseErrorSource {
    pub fn internal() -> ParseErrorSource {
        ParseErrorSource::InternalError {
            // backtrace: Backtrace::capture()
        }
    }
}

// impl ParseErrorSource {
//     pub fn is_internal(&self) -> bool {
//         *self == ParseErrorSource::InternalError
//     }
// }