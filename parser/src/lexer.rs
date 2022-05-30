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
        let input = "1 + 2 + 3";
        let span1 = "^-------^";
        let span2 = "| | | | |";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::dec_lit, Rule::binary_op, Rule::dec_lit, Rule::binary_op, Rule::dec_lit];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
    }

    #[test]
    fn expression_field_access() {
        let input = "x.y.z";
        let span1 = "^---^";
        let span2 = "|||||";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::ident_name, Rule::binary_op, Rule::ident_name, Rule::binary_op, Rule::ident_name];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
    }

    #[test]
    fn expression_open_range() {
        let input = "0 .. 7";
        let spans = "^----^";
        let expected = [Rule::expression];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn expression_closed_range() {
        let input = "0 ..= 7";
        let spans = "^-----^";
        let expected = [Rule::expression];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        println!("{:?}", parsed);
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
    fn expression_tuple_literal() {
        let input = "('x', 'y')";
        let spans = "^--------^";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::tuple_of_expressions];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], spans, expected1));
        assert!(verify(parsed.clone(), vec![0], spans, expected2));
    }

    #[test]
    fn expression_call() {
        let input = "fun(1, 2)";
        let span1 = "^-------^";
        let span3 = "^-^^----^";
        let span4 = "    |  | ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::call_expr];
        let expected3 = [Rule::ident_name, Rule::call_arguments];
        let expected4 = [Rule::expression, Rule::expression];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected2));
        assert!(verify(parsed.clone(), vec![0, 0], span3, expected3));
        assert!(verify(parsed.clone(), vec![0, 0, 1], span4, expected4));
    }


    #[test]
    fn expression_call_method() {
        let input = "abc.xyz.uvw(1, 1+1)";
        let span1 = "^-----------------^";
        let span2 = "^-^|^-^|^---------^";
        let span3 = "            |  ^-^ ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::ident_name, Rule::binary_op, Rule::ident_name, Rule::binary_op, Rule::call_expr];
        let expected3 = [Rule::expression, Rule::expression];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        // println!("{:?}", parsed);
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 4, 1], span3, expected3));
    }

    #[test]
    fn expression_compound_sum() {
        let input = "(1 + x + data.y + fun()) + fun2(1, 2)";
        let span1 = "^-----------------------------------^";
        let span2 = "^----------------------^ | ^--------^";
        let span3 = " | | | | ^--^|| | ^---^         I  I ";
        let span4 = "                                |  | ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::expression_parenthesized, Rule::binary_op, Rule::call_expr];
        let expected3 = [
            Rule::dec_lit, Rule::binary_op, Rule::ident_name,
            Rule::binary_op, Rule::ident_name, Rule::binary_op, Rule::ident_name,
            Rule::binary_op, Rule::call_expr
        ];
        let expected4 = [Rule::expression, Rule::expression];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        // println!("{:?}", parsed);
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0, 0], span3, expected3));
        assert!(verify(parsed.clone(), vec![0, 2, 1], span4, expected4));
    }

    #[test]
    fn tuple_of_expressions() {
        let input = "(1 + 1, 2, x.y)";
        let span1 = "^-------------^";
        let span3 = " ^---^  |  ^-^ ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::tuple_of_expressions];
        let expected3 = [Rule::expression, Rule::expression, Rule::expression];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected2));
        assert!(verify(parsed.clone(), vec![0, 0], span3, expected3));
    }

    #[test]
    fn expression_parenthesized() {
        let input = "(1+1)";
        let spans = "^---^";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::expression_parenthesized];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], spans, expected1));
        assert!(verify(parsed.clone(), vec![0], spans, expected2));
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
    fn fixed_ty_directly() {
        let input = "uq3.12";
        let spans = "^----^";
        let expected = [Rule::fixed_unsigned_ty];
        let parsed = Lexer::parse(Rule::fixed_unsigned_ty, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn fixed_ty_expr_directly() {
        let input = "uq{ (3, 12) }";
        let spans = "^-----------^";
        let expected = [Rule::fixed_unsigned_ty];
        let parsed = Lexer::parse(Rule::fixed_unsigned_ty, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn xpi_group() {
        let input = "/group {}";
        let spans = " ^---^ ^^";
        let expected = [Rule::xpi_uri_segment, Rule::xpi_body];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        assert!(verify(parsed, vec![0], spans, expected));
    }

    #[test]
    fn xpi_name_interpolation() {
        let input = "/velocity_`'x'..'z'` {}";
        let span1 = " ^-----------------^ ^^";
        let span2 = " ^-------^^--------^   ";
        let expected1 = [Rule::xpi_uri_segment, Rule::xpi_body];
        let expected2 = [Rule::xpi_uri_ident, Rule::expression_ticked];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        // println!("{:?}", parsed);
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
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
        let span2 = "         ^---------^ ^-------^  ";
        let span3 = "         ^---^  ^-^         I   ";
        let span4 = "                     ^---^  |   ";
        let expected1 = [Rule::xpi_uri_segment, Rule::xpi_body];
        let expected2 = [Rule::xpi_field, Rule::xpi_field];
        let expected34 = [Rule::ident_name, Rule::expression];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
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

    #[test]
    fn xpi_resource_type_and_serial() {
        let input = "/resource<u8, '0> {}";
        let span1 = " ^------^^------^ ^^";
        let span2 = "          ^^  ^^    ";
        let expected1 = [Rule::xpi_uri_segment, Rule::xpi_resource_ty, Rule::xpi_body];
        let expected2 = [Rule::discrete_any_ty, Rule::xpi_serial];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        println!("{:?}", parsed);
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 1], span2, expected2));
    }

    #[test]
    fn char_literal() {
        let input = "'a'";
        let spans = "^-^";
        let expected = [Rule::char_lit];
        let parsed = Lexer::parse(Rule::any_lit, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn char_literal_unicode() {
        let input = "'∈'";
        let spans = "^---^";
        let expected = [Rule::char_lit];
        let parsed = Lexer::parse(Rule::any_lit, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn char_literal_ascii_escape() {
        let input = "'\\n'";
        let spans = "^--^";
        let expected = [Rule::char_lit];
        let parsed = Lexer::parse(Rule::any_lit, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn tuple_literal() {
        let input = "(1, 2, 3)";
        let span1 = "^-------^";
        let span2 = " |  |  | ";
        let span3 = " |       ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::expression, Rule::expression, Rule::expression];
        let expected3 = [Rule::dec_lit];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0, 0], span3, expected3));
    }
}