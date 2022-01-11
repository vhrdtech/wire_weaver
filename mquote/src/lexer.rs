#[derive(Parser)]
#[grammar = "mquote.pest"]
pub struct MQuoteLexer;

#[cfg(test)]
mod tests {
    use crate::pest::Parser;
    use super::{Rule, MQuoteLexer};

    #[test]
    fn test_basic() {
        let ts = MQuoteLexer::parse(Rule::token_stream, "x + y");
        // let _:() = p;
        // let p = Lexer::parse(Rule::discrete_any_ty, "u32");
        println!("{:?}", ts);
    }
}