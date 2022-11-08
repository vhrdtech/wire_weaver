use core::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialMultiUri {}

impl Display for SerialMultiUri {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "MultiUri()")
    }
}
