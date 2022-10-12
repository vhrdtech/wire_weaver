use super::prelude::*;
use crate::ast::expr::{CallArguments, IndexArguments};
use crate::ast::naming::{Identifier, XpiUriSegmentName};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ResourcePathKind {
    FromRoot,
    FromParent,
    FromSelf,
}

impl ResourcePathKind {
    pub fn to_str(&self) -> &'static str {
        match self {
            ResourcePathKind::FromRoot => "#",
            ResourcePathKind::FromParent => "#..",
            ResourcePathKind::FromSelf => "#.",
        }
    }
}

impl<'i> Parse<'i> for ResourcePathKind {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let kind = input.expect1(Rule::resource_path_start)?;
        match kind.as_str() {
            "#.." => Ok(ResourcePathKind::FromParent),
            "#." => Ok(ResourcePathKind::FromSelf),
            "#" => Ok(ResourcePathKind::FromRoot),
            _ => return Err(ParseErrorSource::internal("ResourcePathKind::parse")),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResourcePathPart<'i> {
    Reference(Identifier<'i, XpiUriSegmentName>),
    IndexInto(IndexIntoResource<'i>),
}

impl<'i> Display for ResourcePathPart<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourcePathPart::Reference(named) => write!(f, "{}", named.name),
            ResourcePathPart::IndexInto(idx) => {
                write!(f, "[{}", idx.name.name)?;
                for arg in &idx.args.0 {
                    write!(f, "{}, ", arg)?;
                }
                write!(f, "]")
            }
        }
    }
}

impl<'i> TryFrom<ResourcePathTail<'i>> for ResourcePathPart<'i> {
    type Error = ParseErrorSource;

    fn try_from(tail: ResourcePathTail<'i>) -> Result<Self, Self::Error> {
        match tail {
            ResourcePathTail::Reference(named) => Ok(ResourcePathPart::Reference(named)),
            ResourcePathTail::Call(_) => Err(ParseErrorSource::UserError),
            ResourcePathTail::IndexInto(idx) => Ok(ResourcePathPart::IndexInto(idx)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResourcePathTail<'i> {
    Reference(Identifier<'i, XpiUriSegmentName>),
    Call(CallResource<'i>),
    IndexInto(IndexIntoResource<'i>),
}

impl<'i> Display for ResourcePathTail<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourcePathTail::Reference(named) => write!(f, "{}", named.name),
            ResourcePathTail::Call(call) => write!(f, "{}", call),
            ResourcePathTail::IndexInto(idx) => write!(f, "{}", idx),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallResource<'i> {
    pub name: Identifier<'i, XpiUriSegmentName>,
    pub args: CallArguments<'i>,
}

impl<'i> Parse<'i> for CallResource<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::call_expr)?, input);
        Ok(CallResource {
            name: input.parse()?,
            args: input.parse()?,
        })
    }
}

impl<'i> Display for CallResource<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}(", self.name.name)?;
        for arg in &self.args.0 {
            write!(f, "{}, ", arg)?;
        }
        write!(f, ")")
    }
}

#[derive(Debug, Clone)]
pub struct IndexIntoResource<'i> {
    pub name: Identifier<'i, XpiUriSegmentName>,
    pub args: IndexArguments<'i>,
}

impl<'i> Parse<'i> for IndexIntoResource<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::index_into_expr)?, input);
        Ok(IndexIntoResource {
            name: input.parse()?,
            args: input.parse()?,
        })
    }
}

impl<'i> Display for IndexIntoResource<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}(", self.name.name)?;
        for arg in &self.args.0 {
            write!(f, "{}, ", arg)?;
        }
        write!(f, ")")
    }
}
