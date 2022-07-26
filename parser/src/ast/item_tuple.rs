use super::prelude::*;
use super::item_type::Type;

#[derive(Debug)]
pub struct TupleFields<'i> {
    pub fields: Vec<Type<'i>>
}

impl<'i> Parse<'i> for TupleFields<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        if let Some(tf) = input.pairs.peek() {
            if tf.as_rule() == Rule::tuple_fields {
                let tf = input.pairs.next().unwrap();
                let mut tf = ParseInput::fork(tf, input);
                let mut fields = Vec::new();
                while let Some(_) = tf.pairs.peek() {
                    tf.parse().map(|ty| fields.push(ty))?;
                }

                Ok(TupleFields {
                    fields
                })
            } else {
                Err(ParseErrorSource::Internal)
            }
        } else {
            Err(ParseErrorSource::Internal)
        }
    }
}