//pub mod context;
//pub mod loader;
//mod utils;
//mod doctree;
//mod types;
//
//#[cfg(test)]
//mod tests {
//    #[test]
//    fn it_works() {
//        assert_eq!(2 + 2, 4);
//    }
//}
pub mod loader;
pub mod lexer;
pub mod parser;
pub mod ast;
pub mod ir;
pub mod types;
pub mod error;
