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
pub mod identifier;
pub mod lit;
pub mod num_bound;
pub mod ops;
pub mod paths;
pub mod stmt;
pub mod ty;
pub mod unit;

mod prelude {
    pub use crate::ast::attrs::AttrsParse;
    pub use crate::ast::doc::DocParse;
    pub use crate::ast::identifier;
    pub use crate::ast::identifier::IdentifierParse;
    pub use crate::error::ParseErrorSource;
    pub use crate::lexer::Rule;
    pub use crate::parse::{Parse, ParseInput};
    pub use crate::span::ast_span_from_pest;
}

#[cfg(test)]
pub(crate) mod test {
    use crate::lexer::{Lexer, Rule};
    use crate::parse::{Parse, ParseInput};
    use crate::pest::Parser;
    use crate::span::ast_span_from_pest;

    pub(crate) fn parse_str<'i, T: Parse<'i>>(input: &'i str, as_rule: Rule) -> T {
        let pairs = Lexer::parse(as_rule, input).unwrap();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        let pair_peek = pairs.peek().unwrap();
        let mut input = ParseInput::new(
            pairs,
            ast_span_from_pest(pair_peek.as_span()),
            &mut warnings,
            &mut errors,
        );
        let result: T = input.parse().unwrap();
        assert!(warnings.is_empty());
        assert!(errors.is_empty());
        result
    }
}
