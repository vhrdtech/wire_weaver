use std::fmt::{Display, Formatter};
use crate::ast::expr::Expr;
use crate::ast::naming::XpiUriNamedPart;
use super::prelude::*;

#[derive(Debug, Clone, Copy)]
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
            ResourcePathKind::FromSelf => "#."
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
            _ => {
                return Err(ParseErrorSource::internal("ResourcePathKind::parse"))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResourcePathPart<'i> {
    Reference(XpiUriNamedPart<'i>),
    IndexInto(XpiUriNamedPart<'i>, Vec<Expr<'i>>)
}

impl<'i> Display for ResourcePathPart<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourcePathPart::Reference(named) => write!(f, "{}", named.name),
            ResourcePathPart::IndexInto(named, args) => {
                write!(f, "[{}", named.name)?;
                for arg in args {
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
            ResourcePathTail::Call(_, _) => Err(ParseErrorSource::UserError),
            ResourcePathTail::IndexInto(named, args) => Ok(ResourcePathPart::IndexInto(named, args))
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResourcePathTail<'i> {
    Reference(XpiUriNamedPart<'i>),
    Call(XpiUriNamedPart<'i>, Vec<Expr<'i>>),
    IndexInto(XpiUriNamedPart<'i>, Vec<Expr<'i>>),
}

impl<'i> Display for ResourcePathTail<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourcePathTail::Reference(named) => write!(f, "{}", named.name),
            ResourcePathTail::Call(named, args) => {
                write!(f, "{}(", named.name)?;
                for arg in args {
                    write!(f, "{}, ", arg)?;
                }
                write!(f, "]")
            }
            ResourcePathTail::IndexInto(named, args) => {
                write!(f, "{}[", named.name)?;
                for arg in args {
                    write!(f, "{}, ", arg)?;
                }
                write!(f, "]")
            }
        }
    }
}