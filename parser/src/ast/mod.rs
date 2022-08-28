pub mod file;
pub mod definition;
pub mod attrs;
pub mod def_enum;
pub mod tuple;
pub mod ty;
pub mod def_type_alias;
pub mod doc;
pub mod naming;
pub mod lit;
pub mod ops;
pub mod def_xpi_block;
pub mod expr;
pub mod stmt;
pub mod def_const;
pub mod def_fn;
pub mod generics;
pub mod visit;
pub mod num_bound;
pub mod paths;
pub mod def_struct;

mod prelude {
    pub use crate::parse::{ParseInput, Parse};
    pub use crate::lexer::Rule;
    pub use crate::ast::naming::Identifier;
    pub use crate::ast::doc::Doc;
    pub use crate::ast::attrs::Attrs;
    pub use crate::error::ParseErrorSource;
}

#[cfg(test)]
pub(crate) mod test {
    use crate::lexer::{Lexer, Rule};
    use crate::parse::{Parse, ParseInput};
    use crate::pest::Parser;

    pub(crate) fn parse_str<'i, T: Parse<'i>>(input: &'i str, as_rule: Rule) -> T {
        let pairs = Lexer::parse(as_rule, input).unwrap();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        let mut input = ParseInput::new(pairs, &mut warnings, &mut errors);
        let result: T = input.parse().unwrap();
        assert!(warnings.is_empty());
        assert!(errors.is_empty());
        result
    }
}