use ast::{SourceOrigin, Span, SpanOrigin, VisitMut};

pub fn ast_span_from_pest(span: pest::Span) -> Span {
    Span {
        start: span.start(),
        end: span.end(),
        origin: SpanOrigin::Parser(SourceOrigin::Pest),
    }
}

pub struct ChangeOrigin {
    pub to: SpanOrigin,
}

impl VisitMut for ChangeOrigin {
    fn visit_span(&mut self, i: &mut Span) {
        i.origin = self.to.clone();
    }
}