use super::prelude::*;

pub struct UnitParse(pub ());

impl<'i> Parse<'i> for UnitParse {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let _si_expr = ParseInput::fork(input.expect1(Rule::si_expr, "UnitParse")?, input);
        Ok(UnitParse(()))
    }
}
