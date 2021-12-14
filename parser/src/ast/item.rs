use crate::parse::{ParseInput, Parse};
use crate::error::ParseError;
use crate::lexer::Rule;

#[derive(Copy, Clone, Debug)]
pub enum Item<'i> {
    Const(ItemConst),
    Enum(ItemEnum<'i>)
}

impl<'i> Parse<'i> for Item<'i> {
    fn parse<'m>(mut input: ParseInput<'i, 'm>) -> Result<Self, ()> {
        println!("Item::parse {:?}", input.pairs.peek());
        let rule = input.pairs.next().unwrap();
        match rule.as_rule() {
            Rule::enum_def => {
                ParseInput::new(rule.into_inner(), &mut input.warnings, &mut input.errors).parse().map(|enum_item| Item::Enum(enum_item))
            },
            _ => {
                input.errors.push(ParseError::E0001);
                Err(())
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ItemConst {

}


#[derive(Copy, Clone, Debug)]
pub struct Typename<'i> {
    typename: &'i str,
}


impl<'i> Parse<'i> for Typename<'i> {
    fn parse<'m>(input: ParseInput<'i, 'm>) -> Result<Typename<'i>, ()> {
        if let Some(p) = input.pairs.peek() {
            return if p.as_rule() == Rule::type_name {
                Ok(Typename {
                    typename: p.as_str()
                })
            } else {
                Err(())
            };
        }
        Err(())
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ItemEnum<'i> {
    // doc: Option<Doc>,
    // attrs: Vec<Attribute>,
    typename: Typename<'i>,
    // items: Vec<EnumItem>
}

impl<'i> Parse<'i> for ItemEnum<'i> {
    fn parse<'m>(input: ParseInput<'i, 'm>) -> Result<Self, ()> {
        println!("ItemEnum::parse {:?}", input.pairs.peek());
        println!("ItemEnum::parse {:?}", input.pairs.peek());
        println!("ItemEnum::parse {:?}", input.pairs.peek());
        println!("ItemEnum::parse {:?}", input.pairs.peek());
        Ok(ItemEnum {
            typename: input.parse()?
        })
    }
}

