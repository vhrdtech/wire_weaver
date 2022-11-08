use crate::Lit;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoNumber {
    pub start: Lit,
    pub step: Lit,
    pub end: Lit,
    pub inclusive: bool,
}

impl Display for AutoNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let range_op = if self.inclusive { "..=" } else { ".." };
        write!(
            f,
            "autonum<{}, {} {} {}>",
            self.start, self.step, range_op, self.end
        )
    }
}
