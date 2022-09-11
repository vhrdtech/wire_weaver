use std::convert::{TryFrom, TryInto};
use crate::ast::struct_def::StructDef;
use crate::ast::visit_mut::VisitMut;
use parser::ast::definition::Definition as ParserDefinition;
use parser::ast::file::File as ParserFile;
use parser::span::{Span, SpanOrigin};
use crate::ast::xpi_def::XpiDef;
use crate::error::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    pub items: Vec<Definition>,
}

impl File {
    pub fn from_parser_ast(file: ParserFile) -> Self {
        let mut file_core_ast = File {
            items: vec![]
        };
        for d in file.defs {
            match d.try_into() {
                Ok(d) => file_core_ast.items.push(d),
                Err(e) => {
                    println!("{:?}", e);
                }
            }
        }
        let mut modifier = SpanOriginModifier { to: file.origin };
        modifier.visit_file(&mut file_core_ast);
        file_core_ast
    }
}

struct SpanOriginModifier {
    to: SpanOrigin,
}
impl VisitMut for SpanOriginModifier {
    fn visit_span(&mut self, i: &mut Span) {
        i.origin = self.to.clone();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Definition {
    //Const(ConstDef),
    //Enum(EnumDef),
    Struct(StructDef),
    //Function(FunctionDef),
    //TypeAlias(TypeAliasDef),
    Xpi(XpiDef),
}

// impl<'i> From<ParserFile<'i>> for File {
//     fn from(f: ParserFile<'i>) -> Self {
//         File {
//             items: f.defs.iter().map(|def| def.clone().into()).collect()
//         }
//     }
// }

impl<'i> TryFrom<ParserDefinition<'i>> for Definition {
    type Error = Error;

    fn try_from(pd: ParserDefinition<'i>) -> Result<Self, Self::Error> {
        match pd {
            ParserDefinition::Const(_) => todo!(),
            ParserDefinition::Enum(_) => todo!(),
            ParserDefinition::Struct(sd) => Ok(Definition::Struct(sd.into())),
            ParserDefinition::Function(_) => todo!(),
            ParserDefinition::TypeAlias(_) => todo!(),
            ParserDefinition::XpiBlock(xpi) => Ok(Definition::Xpi(xpi.try_into()?)),
        }
    }
}
