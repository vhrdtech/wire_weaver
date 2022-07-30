use crate::ast::item_expr::ItemExpr;
use crate::ast::item_stmt::ItemStmt;
use crate::ast::item_type::Type;
use crate::ast::naming::{XpiKeyName, XpiUriNamedPart};
use crate::error::ParseErrorKind;
use super::prelude::*;

// macro_rules! function {
//     () => {{
//         fn f() {}
//         fn type_name_of<T>(_: T) -> &'static str {
//             std::any::type_name::<T>()
//         }
//         let name = type_name_of(f);
//         &name[..name.len() - 3]
//     }}
// }

#[derive(Debug)]
pub struct ItemXpiBlock<'i> {
    pub uri: XpiUri<'i>,
    pub resource_ty: Option<XpiResourceTy<'i>>,
    pub body: XpiBody<'i>,
}

impl<'i> Parse<'i> for ItemXpiBlock<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        // dbg!("xpi_block parse");
        crate::util::pest_print_tree(input.pairs.clone());
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_block)?, input);
        Ok(ItemXpiBlock {
            uri: input.parse()?,
            resource_ty: input.parse_or_skip()?,
            body: input.parse()?
        })
    }
}

#[derive(Debug)]
pub struct XpiResourceTy<'i> {
    pub access: Option<XpiResourceAccessMode>,
    pub r#type: Option<XpiBlockType<'i>>,
    pub serial: Option<XpiSerial>,
}

impl<'i> Parse<'i> for XpiResourceTy<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let mut input = ParseInput::fork(
            input.expect1(Rule::xpi_resource_ty)?,
            input
        );

        Ok(XpiResourceTy {
            access: input.parse_or_skip()?,
            r#type: input.parse_or_skip()?,
            serial: input.parse_or_skip()?
        })
    }
}

#[derive(Debug)]
pub struct XpiBody<'i> {
    pub kv_list: XpiBlockKVList<'i>,
    pub children: XpiBlockChildren<'i>,
}

impl<'i> Parse<'i> for XpiBody<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let mut input = ParseInput::fork(
            input.expect1(Rule::xpi_body)?,
            input
        );

        Ok(XpiBody {
            kv_list: input.parse()?,
            children: input.parse()?
        })
    }
}

#[derive(Debug)]
pub enum XpiUri<'i> {
    /// `/main`
    OneNamedPart(XpiUriNamedPart<'i>),
    /// /\`'a'..'c'\`_ctrl
    ExprThenNamedPart(ItemExpr<'i>, XpiUriNamedPart<'i>),
    /// /velocity_\`'x'..'z'\`
    NamedPartThenExpr(XpiUriNamedPart<'i>, ItemExpr<'i>),
    /// /register_\`'0'..'9'\`_b
    Full(XpiUriNamedPart<'i>, ItemExpr<'i>, XpiUriNamedPart<'i>)
}

impl<'i> Parse<'i> for XpiUri<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let mut input = ParseInput::fork(
            input.expect1(Rule::xpi_uri_segment)?,
            input
        );

        let mut input_peek = input.pairs.clone();
        let (p1, p2, p3) = (input_peek.next(), input_peek.next(), input_peek.next());
        if p1.is_some() && p2.is_some() && p3.is_some() {
            Ok(XpiUri::Full(input.parse()?, input.parse()?, input.parse()?))
        } else if p1.is_some() && p2.is_some() {
            if p1.unwrap().as_rule() == Rule::identifier {
                Ok(XpiUri::NamedPartThenExpr(input.parse()?, input.parse()?))
            } else {
                Ok(XpiUri::ExprThenNamedPart(input.parse()?, input.parse()?))
            }
        } else if p1.is_some() {
            Ok(XpiUri::OneNamedPart(input.parse()?))
        } else {
            Err(ParseErrorSource::internal())
        }
    }
}

#[derive(Debug)]
pub enum XpiResourceAccessMode {
    Rw,
    Ro,
    Wo,
    Const
}

impl<'i> Parse<'i> for XpiResourceAccessMode {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        match input.pairs.peek() {
            Some(pair) => {
                if pair.as_rule() == Rule::access_mod {
                    let access_mod = input.pairs.next().unwrap();
                    match access_mod.as_str() {
                        "rw" => Ok(XpiResourceAccessMode::Rw),
                        "ro" => Ok(XpiResourceAccessMode::Ro),
                        "wo" => Ok(XpiResourceAccessMode::Wo),
                        "const" => Ok(XpiResourceAccessMode::Const),
                        _ => Err(ParseErrorSource::internal())
                    }
                } else {
                    Err(ParseErrorSource::UnexpectedInput)
                }
            }
            None => Err(ParseErrorSource::UnexpectedInput)
        }
    }
}

#[derive(Debug)]
pub struct XpiBlockType<'i>(pub Option<Type<'i>>);

impl<'i> Parse<'i> for XpiBlockType<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        Ok(XpiBlockType(input.parse_or_skip()?))
    }
}

#[derive(Debug)]
pub struct XpiSerial(pub u32);

impl<'i> Parse<'i> for XpiSerial {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let xpi_serial = input.expect1(Rule::xpi_serial)?;
        Ok(XpiSerial(xpi_serial.as_str().strip_prefix('\'')
                .ok_or_else(|| ParseErrorSource::internal())?
                .parse().map_err(|_| {
                    input.push_error(&xpi_serial, ParseErrorKind::IntParseError);
                    ParseErrorSource::UserError
                })?
        ))
    }
}

#[derive(Debug)]
pub struct XpiBlockKVList<'i>(pub Vec<XpiBlockKeyValue<'i>>);

impl<'i> Parse<'i> for XpiBlockKVList<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let mut kv_list = Vec::new();
        while let Some(kv) = input.parse_or_skip()? {
            kv_list.push(kv);
        }
        Ok(XpiBlockKVList(kv_list))
    }
}

#[derive(Debug)]
pub struct XpiBlockKeyValue<'i> {
    pub key: XpiKeyName<'i>,
    pub value: XpiValue<'i>,
}

impl<'i> Parse<'i> for XpiBlockKeyValue<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let mut input = ParseInput::fork(input.expect1(Rule::xpi_field)?, input);
        Ok(XpiBlockKeyValue {
            key: input.parse()?,
            value: input.parse()?
        })
    }
}

#[derive(Debug)]
pub enum XpiValue<'i> {
    Stmt(ItemStmt<'i>),
    Expr(ItemExpr<'i>),
}

impl<'i> Parse<'i> for XpiValue<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let try_stmt: Option<ItemStmt<'i>> = input.parse_or_skip()?;
        match try_stmt {
            Some(stmt) => Ok(XpiValue::Stmt(stmt)),
            None => Ok(XpiValue::Expr(input.parse()?))
        }
    }
}

#[derive(Debug)]
pub struct XpiBlockChildren<'i>(pub Vec<ItemXpiBlock<'i>>);

impl<'i> Parse<'i> for XpiBlockChildren<'i> {
    fn parse<'m>(input: &mut ParseInput<'i, 'm>) -> Result<Self, ParseErrorSource> {
        // dbg!(function!());
        let mut children = Vec::new();
        while let Some(child) = input.parse_or_skip()? {
            children.push(child);
        }
        Ok(XpiBlockChildren(children))
    }
}