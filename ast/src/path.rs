use std::fmt::{Display, Formatter};
use crate::ast::identifier::Identifier;
use itertools::Itertools;
use crate::Identifier;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Path {
    pub items: Vec<Identifier>,
}

impl Path {
    pub fn new() -> Self {
        Path {
            items: vec![]
        }
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // TODO: replace with standard function when it is stabilized
        Itertools::intersperse(
            self.items.iter().map(|elem| format!("{:-}", elem)),
            "::".to_owned(),
        ).try_for_each(|s| write!(f, "{}", s))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ResourcePathMarker {
    FromRoot,
    FromParent,
    FromSelf,
}

impl ResourcePathMarker {
    pub fn to_str(&self) -> &'static str {
        match self {
            ResourcePathKind::FromRoot => "#",
            ResourcePathKind::FromParent => "#..",
            ResourcePathKind::FromSelf => "#.",
        }
    }
}
