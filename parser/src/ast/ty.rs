use super::prelude::*;

#[derive(Debug)]
pub enum Ty {
    Boolean,
    Discrete {
        is_signed: bool,
        bits: u32
    },
    FixedPoint {
        is_signed: bool,
        m: u32,
        n: u32,
    },
    FloatingPoint {
        bits: u32
    },
    Textual,
    Sequence,
    UserDefined
}

impl<'i> Parse<'i> for Ty {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        let ty = input.pairs.next().unwrap();
        println!("ty: {:?}", ty);
        match ty.as_rule() {
            Rule::bool_ty => {
                Ok(Ty::Boolean)
            }
            Rule::discrete_any_ty => {
                let bits: u32 = ty
                    .as_str().strip_prefix("u")
                    .or(ty.as_str().strip_prefix("i"))
                    .unwrap().parse().unwrap();
                let is_signed = ty
                    .into_inner().next().unwrap().as_rule() == Rule::discrete_signed_ty;
                Ok(Ty::Discrete { is_signed, bits })
            }
            Rule::fixed_any_ty => {
                Err(())
            }
            Rule::floating_any_ty => {
                Err(())
            }
            Rule::textual_any_ty => {
                Err(())
            }
            Rule::tuple_ty => {
                Err(())
            }
            Rule::array_ty => {
                Err(())
            }
            Rule::type_name => {
                Err(())
            }
            _ => {
                Err(())
            }
        }
    }
}
