#[derive(Parser)]
#[grammar = "vhl.pest"]
pub struct Lexer;

#[cfg(test)]
mod test {
    use crate::pest::{Parser, iterators::Pairs};
    use super::{Rule, Lexer};

    fn verify<I>(mut input: Pairs<Rule>, level: Vec<usize>, spans: &str, expected: I) -> bool
        where I: IntoIterator<Item=Rule>,
    {
        for n in level {
            input = input.skip(n).next().unwrap().into_inner();
        }
        // println!("{:?}", input);
        let output = input.map(|t| {
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
    fn discrete_signed_8_as_discrete_any_ty() {
        let input = "i8";
        let spans = "^^";
        let expected = [Rule::discrete_signed_ty];
        let parsed = Lexer::parse(Rule::discrete_any_ty, input).unwrap();
        assert!(verify(parsed, vec![0], spans, expected));
    }

    #[test]
    fn discrete_unsigned_64_directly() {
        let input = "u64";
        let spans = "^-^";
        let expected = [Rule::discrete_unsigned_ty];
        let parsed = Lexer::parse(Rule::discrete_unsigned_ty, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn discrete_unsigned_29_directly() {
        let input = "u29";
        let spans = "^-^";
        let expected = [Rule::discrete_unsigned_ty];
        let parsed = Lexer::parse(Rule::discrete_unsigned_ty, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn expression_sum() {
        let input = "4 + 4";
        let spans = "^---^";
        let expected = [Rule::expression];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn expression_braced() {
        let input = "{ 4 + 4 }";
        let spans = "^-------^";
        let expected = [Rule::expression_braced];
        let parsed = Lexer::parse(Rule::expression_braced, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn discrete_unsigned_expr() {
        let input = "u{ 4 + 4 }";
        let span1 = "^--------^";
        let span2 = " ^-------^";
        let expected1 = [Rule::discrete_unsigned_ty];
        let expected2 = [Rule::expression_braced];
        let parsed = Lexer::parse(Rule::discrete_unsigned_ty, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed, vec![0], span2, expected2));
    }

    #[test]
    fn xpi_grouping() {
        let input = "/group {}";
        let spans = " ^---^ ^^";
        let expected = [Rule::xpi_uri_segment, Rule::xpi_body];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        assert!(verify(parsed, vec![0], spans, expected));
    }

    #[test]
    fn xpi_grouping_nested() {
        let input = "/group { /nested{} }";
        let spans = " ^---^ ^-----------^";
        let expected = [Rule::xpi_uri_segment, Rule::xpi_body];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        assert!(verify(parsed, vec![0], spans, expected));
    }

    #[test]
    fn xpi_fields() {
        let input = "/group { field: 123; field: 1; }";
        let span1 = " ^---^ ^-----------------------^";
        let span2 = "         ^---------^ ^-------^ ^";
        let span3 = "         ^---^  ^-^         I  ^";
        let span4 = "                     ^---^  |  ^";
        let expected1 = [Rule::xpi_uri_segment, Rule::xpi_body];
        let expected2 = [Rule::xpi_field, Rule::xpi_field];
        let expected34 = [Rule::ident_name, Rule::expression];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        // println!("{:?}", parsed.clone().as_rule().into_inner());
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 1], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 1, 0], span3, expected34));
        assert!(verify(parsed.clone(), vec![0, 1, 1], span4, expected34));
    }

    #[test]
    fn xpi_field_and_group() {
        let input = "/group { field: 1; /inner{} }";
        let spans = "         ^-------^ ^------^  ";
        let expected = [Rule::xpi_field, Rule::xpi_block];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        assert!(verify(parsed, vec![0, 1], spans, expected));
    }
}