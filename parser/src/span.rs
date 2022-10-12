use ast::{SourceOrigin, Span, SpanOrigin};

pub fn ast_span_from_pest(span: pest::Span) -> Span {
    Span {
        start: span.start(),
        end: span.end(),
        origin: SpanOrigin::Parser(SourceOrigin::Pest),
    }
}