#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;
use thiserror::Error;
use crate::lexer::Rule;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub rule: crate::lexer::Rule,
    pub span: (usize, usize)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ParseErrorKind {
    InternalError {
        rule: Option<Rule>,
        message: &'static str,
        #[cfg(feature = "backtrace")]
        backtrace: String,
    },
    Unimplemented(&'static str),
    UnhandledUnexpectedInput,
    UserError,

    AutonumWrongForm,
    AutonumWrongArguments,
    IndexOfWrongForm,
    FloatParseError,
    IntParseError,
    MalformedResourcePath,
    WrongAccessModifier,
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
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
        rule: Option<Rule>,
        message: &'static str,
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
    pub fn internal(message: &'static str,) -> ParseErrorSource {
        ParseErrorSource::InternalError {
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
            rule: None,
            message
        }
    }

    pub fn internal_with_rule(rule: Rule, message: &'static str) -> ParseErrorSource {
        ParseErrorSource::InternalError {
            #[cfg(feature = "backtrace")]
            backtrace: Backtrace::capture(),
            rule: Some(rule),
            message,
        }
    }
}

// impl ParseErrorSource {
//     pub fn is_internal(&self) -> bool {
//         *self == ParseErrorSource::InternalError
//     }
// }