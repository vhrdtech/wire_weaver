use std::collections::VecDeque;
use ast::Path;
use ast::path::ResourcePathMarker;
use super::prelude::*;

pub struct PathParse(pub Path);

pub struct ResourcePathMarkerParse(pub ResourcePathMarker);

impl<'i> Parse<'i> for PathParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let simple_path = input.expect1(Rule::simple_path)?;
        let mut input = ParseInput::fork(simple_path, input);
        let mut segments = VecDeque::new();
        while let Some(_) = input.pairs.peek() {
            let segment: IdentifierParse<identifier::PathSegment> = input.parse()?;
            segments.push_back(segment.0);
        }
        Ok(PathParse(Path { segments }))
    }
}

impl<'i> Parse<'i> for ResourcePathMarkerParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let kind = input.expect1(Rule::resource_path_start)?;
        let ast_marker = match kind.as_str() {
            "#.." => ResourcePathMarker::FromParent,
            "#." => ResourcePathMarker::FromSelf,
            "#" => ResourcePathMarker::FromRoot,
            _ => return Err(ParseErrorSource::internal("ResourcePathKind::parse")),
        };
        Ok(ResourcePathMarkerParse(ast_marker))
    }
}