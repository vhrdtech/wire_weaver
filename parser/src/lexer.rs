#[derive(Parser)]
#[grammar = "vhl.pest"]
pub struct Lexer;

#[cfg(test)]
mod test {
    use crate::pest::Parser;
    use super::{Rule, Lexer};

    fn verify_inner<I>(rule: Rule, input: &str, spans: &str, expected: I) -> bool
        where I: IntoIterator<Item = Rule>,
    {
        let mut output = match Lexer::parse(rule, input) {
            Ok(output) => output,
            Err(e) => panic!("{}", e)
        };
        println!("{:?}", output);
        let output = output.next().unwrap().into_inner().map(|t| {
            let span = t.as_span();
            parser_test::TestToken {
                start: span.start(),
                end: span.end() - 1,
                rule: t.as_rule()
            }
        });
        parser_test::test(output, expected, spans)
    }

    #[test]
    fn discrete_signed_8() {
        let input = "i8";
        let spans = "^^";
        let expected = [Rule::discrete_signed_ty];
        assert!(verify_inner(Rule::discrete_any_ty, input, spans, expected));
    }

    #[test]
    fn discrete_unsigned_64() {
        let input = "u64";
        let spans = "^^^";
        let expected = [Rule::discrete_unsigned_ty];
        assert!(verify_inner(Rule::discrete_any_ty, input, spans, expected));
    }

    #[test]
    fn discrete_unsigned_29() {
        let input = "u29";
        let spans = "^^^";
        let expected = [Rule::discrete_unsigned_ty];
        assert!(verify_inner(Rule::discrete_any_ty, input, spans, expected));
    }

    #[test]
    fn discrete_unsigned_expr() {
        let input = "u{ 4 + 4 }";
        let spans = "^^^^^^^^^^";
        let expected = [Rule::discrete_unsigned_ty];
        assert!(verify_inner(Rule::discrete_any_ty, input, spans, expected));
    }
}