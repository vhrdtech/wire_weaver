use super::prelude::*;
use ast::Path;
use std::collections::VecDeque;

pub struct PathParse(pub Path);

impl<'i> Parse<'i> for PathParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let simple_path = input.expect1(Rule::path)?;
        let mut input = ParseInput::fork(simple_path, input);
        let mut segments = VecDeque::new();
        while let Some(_) = input.pairs.peek() {
            let segment: IdentifierParse<identifier::PathSegment> = input.parse()?;
            segments.push_back(segment.0);
        }
        Ok(PathParse(Path { segments }))
    }
}
