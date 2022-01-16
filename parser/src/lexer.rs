#[derive(Parser)]
#[grammar = "vhl.pest"]
pub struct Lexer;

// pub struct Lexer<'input> {
//     input: &'input str,
//     chars: CharLocations<'input>,
//
// }

#[cfg(test)]
mod test {
    use super::{Rule, Lexer};
    use pest::Parser;

    // fn lexer<'input>(input: &'input str) -> Box<dyn Iterator<Item = Result<Spanned<Token<'input>>, SpError>>> {
    //     Box::new(Lexer::new(input).take_while(|token| match *token {
    //         Ok(Spanned {
    //             value: Token::Eof, ..
    //         }) => false,
    //         _ => true
    //     }))
    // }
    //
    // fn test(input: &str, div11: &str) {
    //
    // }

    #[test]
    fn test_discrete_numbers() {
        let p = Lexer::parse(Rule::file, "enum FrameId { Standard(u11), Extended(u29) }");
        // let _:() = p;
        // let p = Lexer::parse(Rule::discrete_any_ty, "u32");
        println!("{:?}", p);
    }
}