use crate::ast::naming::PathSegment;
use crate::error::ParseErrorSource;
use super::prelude::*;

#[derive(Debug)]
pub struct Attrs<'i> {
    pub attributes: Vec<Attr<'i>>
}

impl<'i> Parse<'i> for Attrs<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut attributes = Vec::new();
        while let Some(a) = input.pairs.peek() {
            if a.as_rule() == Rule::outer_attribute || a.as_rule() == Rule::inner_attribute {
                let a = input.pairs.next().unwrap();
                ParseInput::fork(a, input).parse().map(|attr| attributes.push(attr))?;
            } else {
                break;
            }
        }
        Ok(Attrs {
            attributes
        })
    }
}

#[derive(Debug)]
pub struct Attr<'i> {
    pub path: Vec<PathSegment<'i>>,
    pub input: bool,
}

impl<'i> Parse<'i> for Attr<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let (simple_path, attr_input) = input.next2(Rule::simple_path, Rule::attribute_input);

        let path_segments = match simple_path {
            Some(simple_path) => {
                let mut path_segments = Vec::new();
                for segment in simple_path.into_inner() {
                    ParseInput::fork(segment, input).parse().map(|s| path_segments.push(s))?;
                }
                path_segments
            },
            None => {
                return Err(ParseErrorSource::Internal);
            }
        };

        Ok(Attr {
            path: path_segments,
            input: attr_input.is_some()
        })
    }
}