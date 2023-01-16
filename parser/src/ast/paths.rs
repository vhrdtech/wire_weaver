use super::prelude::*;
use ast::Path;
use std::collections::VecDeque;
use ast::path::PathSegment;

pub struct PathParse(pub Path);

impl<'i> Parse<'i> for PathParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let simple_path = input.expect1(Rule::path, "PathParse")?;
        let mut input = ParseInput::fork(simple_path, input);
        let mut segments = VecDeque::new();
        while input.pairs.peek().is_some() {
            let segment: IdentifierParse<identifier::PathSegment> = input.parse()?;
            segments.push_back(PathSegment {
                ident: segment.0,
                index: None,
            });
        }
        Ok(PathParse(Path { segments }))
    }
}
