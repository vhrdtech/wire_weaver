#[derive(Parser)]
#[grammar = "vhl.pest"]
pub struct Lexer;

#[cfg(test)]
mod test {
    use crate::pest::{Parser, iterators::Pairs};
    use super::{Rule, Lexer};

    fn verify_inner<I>(input: Pairs<Rule>, spans: &str, expected: I) -> bool
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
        let spans = "^-^";
        let expected = [Rule::discrete_unsigned_ty];
        assert!(verify_inner(Rule::discrete_any_ty, input, spans, expected));
    }

    #[test]
    fn discrete_unsigned_expr() {
        let input = "u{ 4 + 4 }";
        let spans = "^--------^";
        let expected = [Rule::discrete_unsigned_ty];
        assert!(verify_inner(Rule::discrete_any_ty, input, spans, expected));
    }

    #[test]
    fn xpi_grouping() {
        let input = "/group {}";
        let spans = " ^---^ ^^";
        let expected = [Rule::xpi_uri_segment, Rule::xpi_body];
        assert!(verify_inner(Rule::xpi_block, input, spans, expected));
    }

    #[test]
    fn xpi_grouping_nested() {
        let input = "/group { /nested{} }";
        let spans = " ^---^ ^-----------^";
        let expected = [Rule::xpi_uri_segment, Rule::xpi_body];
        assert!(verify_inner(Rule::xpi_block, input, spans, expected));
    }
    
    #[test]
    fn xpi_fields() {
        let input = "/group { field: 123, field: 1, }"; // \n doesn't work instead of a comma!
        let spans = " ^---^ ^-----------------------^";
        let expected = [Rule::xpi_uri_segment, Rule::xpi_body];
        assert!(verify_inner(Rule::xpi_block, input, spans, expected));
    }


}
