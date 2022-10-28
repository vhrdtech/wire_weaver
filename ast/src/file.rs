use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use crate::{Definition, Identifier, SpanOrigin};

#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub origin: SpanOrigin,
    // pub defs: Vec<Definition>,
    pub defs: HashMap<Identifier, Definition>,
    pub input: String,
    pub line_starts: Vec<usize>,
    // pub attrs: Vec<Attr>
}

// impl File {
//     pub fn from_parser_ast(file: ParserFile) -> Self {
//         let mut file_core_ast = File {
//             items: vec![]
//         };
//         for d in file.defs {
//             match d.try_into() {
//                 Ok(d) => file_core_ast.items.push(d),
//                 Err(e) => {
//                     println!("{:?}", e);
//                 }
//             }
//         }
//         let mut modifier = SpanOriginModifier { to: file.origin };
//         modifier.visit_file(&mut file_core_ast);
//         file_core_ast
//     }
// }
//
// struct SpanOriginModifier {
//     to: SpanOrigin,
// }
// impl VisitMut for SpanOriginModifier {
//     fn visit_span(&mut self, i: &mut Span) {
//         i.origin = self.to.clone();
//     }
// }


// impl<'i> From<ParserFile<'i>> for File {
//     fn from(f: ParserFile<'i>) -> Self {
//         File {
//             items: f.defs.iter().map(|def| def.clone().into()).collect()
//         }
//     }
// }
//
// impl<'i> TryFrom<ParserDefinition<'i>> for Definition {
//     type Error = Error;
//
//     fn try_from(pd: ParserDefinition<'i>) -> Result<Self, Self::Error> {
//         match pd {
//             ParserDefinition::Const(_) => todo!(),
//             ParserDefinition::Enum(ed) => Ok(Definition::Enum(ed.try_into()?)),
//             ParserDefinition::Struct(sd) => Ok(Definition::Struct(sd.into())),
//             ParserDefinition::Function(_) => todo!(),
//             ParserDefinition::TypeAlias(a) => Ok(Definition::TypeAlias(a.try_into()?)),
//             ParserDefinition::XpiBlock(xpi) => Ok(Definition::Xpi(XpiDef::convert_from_parser(xpi, true)?)),
//         }
//     }
// }
//

impl Display for File {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "File from {}", self.origin)?;
        for (id, def) in &self.defs {
            if f.alternate() {
                writeln!(f, "{}: {:#}", id, def)?;
            } else {
                writeln!(f, "{}: {}", id, def)?;
            }
        }
        Ok(())
    }
}
