#[derive(Debug)]
pub enum Error {
    AstError(ast::error::Error),
    FindDef(String),
    FindXpiDef(String),
    FileNotFound(usize),
}

impl From<ast::error::Error> for Error {
    fn from(e: ast::error::Error) -> Self {
        Error::AstError(e)
    }
}
