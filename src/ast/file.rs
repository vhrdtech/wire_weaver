use crate::ast::struct_def::StructDef;
use parser::ast::file::File as ParserFile;
use parser::ast::definition::Definition as ParserDefinition;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    pub items: Vec<Definition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Definition {
    //Const(ConstDef),
    //Enum(EnumDef),
    Struct(StructDef),
    //Function(FunctionDef),
    //TypeAlias(TypeAliasDef),
    //XpiBlock(XpiBlockDef),
}

impl<'i> From<ParserFile<'i>> for File {
    fn from(f: ParserFile<'i>) -> Self {
        File {
            items: f.defs.iter().map(|def| def.clone().into()).collect()
        }
    }
}

impl<'i> From<ParserDefinition<'i>> for Definition {
    fn from(pd: ParserDefinition<'i>) -> Self {
        match pd {
            ParserDefinition::Const(_) => todo!(),
            ParserDefinition::Enum(_) => todo!(),
            ParserDefinition::Struct(sd) => Definition::Struct(sd.into()),
            ParserDefinition::Function(_) => todo!(),
            ParserDefinition::TypeAlias(_) => todo!(),
            ParserDefinition::XpiBlock(_) => todo!(),
        }
    }
}