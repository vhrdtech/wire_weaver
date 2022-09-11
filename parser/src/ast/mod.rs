pub mod attrs;
pub mod def_const;
pub mod def_enum;
pub mod def_fn;
pub mod def_struct;
pub mod def_type_alias;
pub mod def_xpi_block;
pub mod definition;
pub mod doc;
pub mod expr;
pub mod file;
pub mod generics;
pub mod lit;
pub mod naming;
pub mod num_bound;
pub mod ops;
pub mod paths;
pub mod stmt;
pub mod tuple;
pub mod ty;
pub mod visit;

mod prelude {
    pub use crate::ast::attrs::Attrs;
    pub use crate::ast::doc::Doc;
    pub use crate::ast::naming::Identifier;
    pub use crate::error::ParseErrorSource;
    pub use crate::lexer::Rule;
    pub use crate::parse::{Parse, ParseInput};
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
        let pair_peek = pairs.peek().unwrap();
        let mut input = ParseInput::new(pairs, pair_peek.as_span(), &mut warnings, &mut errors);
        let result: T = input.parse().unwrap();
        assert!(warnings.is_empty());
        assert!(errors.is_empty());
        result
    }
}
