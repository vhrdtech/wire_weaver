use super::prelude::*;
use super::item_enum::ItemEnum;
use std::fmt::Formatter;

#[derive(Debug)]
pub enum Item<'i> {
    Const(ItemConst),
    Enum(ItemEnum<'i>)
}

impl<'i> Parse<'i> for Item<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ()> {
        println!("Item::parse {:?}", input.pairs.peek());
        let rule = input.pairs.next().unwrap();
        match rule.as_rule() {
            Rule::enum_def => {
                ParseInput::fork(rule, input).parse().map(|item| Item::Enum(item))
            },
            _ => {
                input.errors.push(ParseError::E0001);
                Err(())
            }
        }
    }
}

#[derive(Debug)]
pub struct ItemConst {

}


#[derive(Debug)]
pub struct Typename<'i> {
    typename: &'i str,
}


impl<'i> Parse<'i> for Typename<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Typename<'i>, ()> {
        if let Some(p) = input.pairs.peek() {
            return if p.as_rule() == Rule::type_name {
                let p = input.pairs.next().unwrap();
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

#[derive(Debug)]
pub struct Docs<'i> {
    lines: Vec<&'i str>
}

impl<'i> Parse<'i> for Docs<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Docs<'i>, ()> {
        let mut lines = Vec::new();
        while let Some(p) = input.pairs.peek() {
            if p.as_rule() == Rule::doc_comment {
                let p = input.pairs.next().unwrap();
                let line = &p.as_str()[3..];
                let line = line.strip_prefix(" ").unwrap_or(line);
                let line = line.strip_suffix("\r\n").or(line.strip_suffix("\n")).unwrap_or(line);
                lines.push(line);
            } else {
                break;
            }
        }
        Ok(Docs {
            lines
        })
    }
}



