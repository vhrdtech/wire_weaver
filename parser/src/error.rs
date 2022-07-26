#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ParseError {
    pub rule: crate::lexer::Rule,
    pub span: (usize, usize)
}