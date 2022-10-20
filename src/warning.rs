use ast::Span;

pub struct Warning {
    pub kind: WarningKind,
    pub span: Span,
}

pub enum WarningKind {
    XpiArrayWithModifiers,
}