use crate::ast::ident::Ident;

#[derive(Debug)]
pub struct Path {
    pub segments: Vec<Ident>,
    // arguments
}

impl Path {
    pub fn new_ident(ident: Ident) -> Self {
        Path {
            segments: vec![ident],
        }
    }
}
