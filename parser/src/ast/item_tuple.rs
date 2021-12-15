use super::prelude::*;
use super::ty::Ty;

#[derive(Debug)]
pub struct TupleFields {
    pub fields: Vec<Ty>
}

impl<'i> Parse<'i> for TupleFields {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
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
                Err(())
            }
        } else {
            Err(())
        }
    }
}