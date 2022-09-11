#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ParseWarning {
    pub kind: ParseWarningKind,
    pub rule: crate::lexer::Rule,
    pub span: (usize, usize),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ParseWarningKind {
    NonCamelCaseTypename,
}
