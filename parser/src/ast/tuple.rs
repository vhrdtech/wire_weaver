use super::prelude::*;
use super::ty::Ty;

#[derive(Debug, Clone)]
pub struct TupleFieldsTy<'i> {
    pub fields: Vec<Ty<'i>>,
}

impl<'i> Parse<'i> for TupleFieldsTy<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        let mut input = ParseInput::fork(input.expect1(Rule::tuple_fields)?, input);

        let mut fields = Vec::new();
        while let Some(_) = input.pairs.peek() {
            input.parse().map(|ty| fields.push(ty))?;
        }

        Ok(TupleFieldsTy { fields })
    }
}
