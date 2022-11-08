use crate::error::Error;
use crate::{Definition, Identifier, SpanOrigin};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub origin: SpanOrigin,
    // pub defs: Vec<Definition>,
    pub defs: HashMap<Identifier, Definition>,
    pub input: String,
    pub line_starts: Vec<usize>,
    // pub attrs: Vec<Attr>
}

impl File {
    pub fn line_start(&self, line_index: usize) -> Result<usize, Error> {
        match line_index.cmp(&self.line_starts.len()) {
            Ordering::Less => Ok(self
                .line_starts
                .get(line_index)
                .cloned()
                .expect("failed despite previous check")),
            Ordering::Equal => Ok(self.input.len()),
            Ordering::Greater => Err(Error::LineTooLarge {
                given: line_index,
                max: self.line_starts.len() - 1,
            }),
        }
    }

    pub fn line_index(&self, byte_index: usize) -> Result<usize, Error> {
        Ok(self
            .line_starts
            .binary_search(&byte_index)
            .unwrap_or_else(|next_line| next_line - 1))
    }

    pub fn line_range(&self, line_index: usize) -> Result<Range<usize>, Error> {
        let line_start = self.line_start(line_index)?;
        let next_line_start = self.line_start(line_index + 1)?;

        Ok(line_start..next_line_start)
    }
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
}
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
