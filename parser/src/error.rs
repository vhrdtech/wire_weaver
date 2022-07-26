#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub rule: crate::lexer::Rule,
    pub span: (usize, usize)
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ParseErrorKind {
    InternalError,
    AutonumWrongForm,
    AutonumWrongArguments,
    FloatParseError,

}

#[derive(PartialEq, Eq)]
pub enum ParseErrorSource {
    Internal,
    User
}

impl ParseErrorSource {
    pub fn is_internal(&self) -> bool {
        *self == ParseErrorSource::Internal
    }
}