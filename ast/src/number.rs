use std::fmt::{Display, Formatter};
use parser::ast::ty::AutoNumber as AutoNumberParser;
use crate::ast::lit::Lit;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoNumber {
    pub start: Lit,
    pub step: Lit,
    pub end: Lit,
    pub inclusive: bool,
}

impl<'i> From<AutoNumberParser<'i>> for AutoNumber {
    fn from(au: AutoNumberParser<'i>) -> Self {
        AutoNumber {
            start: au.start.into(),
            step: au.step.into(),
            end: au.end.into(),
            inclusive: au.inclusive
        }
    }
}

impl Display for AutoNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let range_op = if self.inclusive {
            "..="
        } else {
            ".."
        };
        write!(f, "autonum<{}, {} {} {}>", self.start, self.step, range_op, self.end)
    }
}