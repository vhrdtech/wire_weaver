#[derive(Parser)]
#[grammar = "vhl.pest"]
pub struct Lexer;

#[cfg(test)]
mod test {
    use crate::pest::{Parser, iterators::Pairs};
    use super::{Rule, Lexer};
    use crate::util::ppt;

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
        let expected2 = [Rule::any_lit, Rule::op_binary, Rule::any_lit, Rule::op_binary, Rule::any_lit];
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
        let expected2 = [Rule::identifier, Rule::op_binary, Rule::identifier, Rule::op_binary, Rule::identifier];
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
        assert!(verify(parsed, vec![], spans, expected));
    }

    // #[test]
    // fn expression_braced() {
    //     let input = "{ 4 + 4 }";
    //     let spans = "^-------^";
    //     let expected = [Rule::expression_braced];
    //     let parsed = Lexer::parse(Rule::expression_braced, input).unwrap();
    //     assert!(verify(parsed, vec![], spans, expected));
    // }

    #[test]
    fn expression_tuple_literal() {
        let input = "('x', 'y')";
        let spans = "^--------^";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::any_lit];
        let expected3 = [Rule::tuple_lit];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], spans, expected1));
        assert!(verify(parsed.clone(), vec![0], spans, expected2));
        assert!(verify(parsed.clone(), vec![0, 0], spans, expected3));
    }

    #[test]
    fn expression_call() {
        let input = "fun(1, 2)";
        let span1 = "^-------^";
        let span3 = "^-^^----^";
        let span4 = "    |  | ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::call_expr];
        let expected3 = [Rule::identifier, Rule::call_arguments];
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
        let span3 = "        ^-^^------^";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::identifier, Rule::op_binary, Rule::identifier, Rule::op_binary, Rule::call_expr];
        let expected3 = [Rule::identifier, Rule::call_arguments];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        // println!("{:?}", parsed);
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 4], span3, expected3));
    }

    #[test]
    fn expression_compound_sum() {
        let input = "(1 + x + data.y + fun()) + fun2(1, 2)";
        let span1 = "^-----------------------------------^";
        let span2 = "^----------------------^ | ^--------^";
        let span3 = " | | | | ^--^|| | ^---^         I  I ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::expression_parenthesized, Rule::op_binary, Rule::call_expr];
        let expected3 = [
            Rule::any_lit, Rule::op_binary, Rule::identifier,
            Rule::op_binary, Rule::identifier, Rule::op_binary, Rule::identifier,
            Rule::op_binary, Rule::call_expr
        ];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0, 0], span3, expected3));
    }

    #[test]
    fn expression_call_then_index() {
        let input = "fun(1)['x']";
        let span1 = "^---------^";
        let span2 = "^-^^-^^---^";
        let expected1 = [Rule::expression];
        let expected1b = [Rule::call_expr];
        let expected2 = [Rule::identifier, Rule::call_arguments, Rule::index_arguments];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected1b));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
    }

    #[test]
    fn expression_index_then_call() {
        let input = "fun['x'](1)";
        let span1 = "^---------^";
        let span2 = "^-^^---^^-^";
        let expected1 = [Rule::expression];
        let expected1b = [Rule::call_expr];
        let expected2 = [Rule::identifier, Rule::index_arguments, Rule::call_arguments];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected1b));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
    }

    #[test]
    fn expression_index() {
        let input = "matrix[3, 7]";
        let span1 = "^----------^";
        let span2 = "^----^^----^";
        let expected1 = [Rule::expression];
        let expected1b = [Rule::index_into_expr];
        let expected2 = [Rule::identifier, Rule::index_arguments];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected1b));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
    }

    #[test]
    fn resource_path_index() {
        let input = "#/sensors/acc/raw[0]";
        let span1 = "^------------------^";
        let span2 = "||^-----^|^-^|^----^";
        let expected1 = [Rule::expression];
        let expected2 = [
            Rule::resource_path_start, Rule::op_binary, Rule::identifier, Rule::op_binary,
            Rule::identifier, Rule::op_binary, Rule::index_into_expr];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
    }

    #[test]
    fn resource_path_relative() {
        let input = "#./#../xyz"; // TODO: add lint to discourage paths like that
        let span1 = "^--------^";
        let span2 = "^^|^-^|^-^";
        let expected1 = [Rule::expression];
        let expected2 = [
            Rule::resource_path_start, Rule::op_binary, Rule::resource_path_start,
            Rule::op_binary, Rule::identifier];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
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
        let input = "u<4+4>";
        let span1 = "^----^";
        let span2 = " ^---^";
        let expected1 = [Rule::discrete_unsigned_ty];
        let expected2 = [Rule::generics];
        let parsed = Lexer::parse(Rule::discrete_unsigned_ty, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
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
        let input = "uq<3, 12>";
        let spans = "^-------^";
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
    fn xpi_resource_with_unit() {
        let input = "/speed<f32 `m/s`> {}";
        let span1 = " ^---^^---------^ ^^";
        let span2 = "       ^-------^    ";
        let span3 = "            ^-^     ";
        let expected1 = [Rule::xpi_uri_segment, Rule::xpi_resource_ty, Rule::xpi_body];
        let expected2 = [Rule::floating_any_ty];
        let expected3 = [Rule::si_expr];
        let parsed = Lexer::parse(Rule::xpi_block, input).unwrap();
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 1], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 1, 0], span3, expected3));
    }

    #[test]
    fn xpi_const_property_resource() {
        let input = "/channel_count<const indexof<#./channel>> {}";
        let span1 = "^------------------------------------------^";
        let span2 = " ^-----------^^-------------------------^ ^^";
        let span3 = "               ^---^ ^-----------------^    ";
        let expected1 = [Rule::xpi_block];
        let expected2 = [Rule::xpi_uri_segment, Rule::xpi_resource_ty, Rule::xpi_body];
        let expected3 = [Rule::access_mod, Rule::generic_ty];
        let parsed = Lexer::parse(Rule::definition, input).unwrap();
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0, 1], span3, expected3));
    }

    #[test]
    fn xpi_array_property_resource() {
        let input = "/channels<[indexof<#../channel>; 3..=4]> {}";
        let span1 = "^-----------------------------------------^";
        let span2 = " ^------^^-----------------------------^ ^^";
        let span3 = "          ^---------------------------^    ";
        let expected1 = [Rule::xpi_block];
        let expected2 = [Rule::xpi_uri_segment, Rule::xpi_resource_ty, Rule::xpi_body];
        let expected3 = [Rule::array_ty];
        let parsed = Lexer::parse(Rule::definition, input).unwrap();
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0, 1], span3, expected3));
    }

    #[test]
    fn xpi_method_resource() {
        let input = "/query<fn()> {}";
        let span1 = "^-------------^";
        let span2 = "       ^--^    ";
        let expected1 = [Rule::xpi_block];
        let expected2 = [Rule::fn_ty];
        let parsed = Lexer::parse(Rule::definition, input).unwrap();
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 0, 1], span2, expected2));
    }

    #[test]
    fn xpi_name_interpolation() {
        let input = "/velocity_`'x'..'z'` {}";
        let span1 = " ^-----------------^ ^^";
        let span2 = " ^-------^^--------^   ";
        let expected1 = [Rule::xpi_uri_segment, Rule::xpi_body];
        let expected2 = [Rule::identifier, Rule::expression_ticked];
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
        let expected34 = [Rule::identifier, Rule::expression];
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
        // println!("{:?}", parsed);
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 1], span2, expected2));
    }

    #[test]
    fn char_literal() {
        let input = "'a'";
        let spans = "^-^";
        let expected = [Rule::any_lit];
        let expected2 = [Rule::char_lit];
        let parsed = Lexer::parse(Rule::any_lit, input).unwrap();
        assert!(verify(parsed.clone(), vec![], spans, expected));
        assert!(verify(parsed, vec![0], spans, expected2));
    }

    #[test]
    fn char_literal_unicode() {
        let input = "'∈'";
        let spans = "^---^";
        let expected = [Rule::any_lit];
        let parsed = Lexer::parse(Rule::any_lit, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn float_literals() {
        let numbers = ["123.0f64", "0.1f64", "0.1f32", "12e+99f64", "5f32"];
        for n in numbers {
            let _ = Lexer::parse(Rule::any_lit, n).unwrap();
        }
    }

    #[test]
    fn type_parameter() {
        let input = "Cell<_>";
        let span1 = "^--^^-^";
        let span2 = "     | ";
        let expected1 = [Rule::identifier, Rule::generics];
        let expected2 = [Rule::derive];
        let parsed = Lexer::parse(Rule::generic_ty, input).unwrap();
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed, vec![0, 1], span2, expected2));
    }

    #[test]
    fn type_parameter_xpi_path() {
        let input = "Cell<#../channel>";
        let spans = "^--^^-----------^";
        let expected = [Rule::identifier, Rule::generics];
        let parsed = Lexer::parse(Rule::generic_ty, input).unwrap();
        assert!(verify(parsed, vec![0], spans, expected));
    }

    #[test]
    fn type_parameters() {
        let input = "alias<u16, _>";
        let spans = "^---^^------^";
        let expected = [Rule::identifier, Rule::generics];
        let parsed = Lexer::parse(Rule::generic_ty, input).unwrap();
        assert!(verify(parsed, vec![0], spans, expected));
    }

    #[test]
    fn char_literal_ascii_escape() {
        let input = "'\\n'";
        let spans = "^--^";
        let expected = [Rule::any_lit];
        let parsed = Lexer::parse(Rule::any_lit, input).unwrap();
        assert!(verify(parsed, vec![], spans, expected));
    }

    #[test]
    fn tuple_literal() {
        let input = "(1, 2, 3)";
        let span1 = "^-------^";
        let span2 = "^-------^";
        let span3 = " |  |  | ";
        let expected1 = [Rule::expression];
        let expected2 = [Rule::any_lit];
        let expected3 = [Rule::any_lit, Rule::any_lit, Rule::any_lit];
        let parsed = Lexer::parse(Rule::expression, input).unwrap();
        ppt!(parsed);
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0, 0], span3, expected3));
    }

    #[test]
    fn si_meters_per_second() {
        let input = "m / s";
        let span1 = "^---^";
        let span2 = "| | |";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::si_name, Rule::si_op, Rule::si_name];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
    }

    #[test]
    fn si_meter_cubed() {
        let input = "m+3";
        let span1 = "^-^";
        let span2 = "|||";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::si_name, Rule::si_op, Rule::dec_lit_raw];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
    }

    #[test]
    fn si_meter() {
        let input = "m";
        let span1 = "|";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::si_name];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected2));
    }

    #[test]
    fn si_millimeter() {
        let input = "mm";
        let span1 = "^^";
        let span2 = "| ";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::si_name];
        let expected3 = [Rule::si_prefix];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected2));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected3));
    }

    #[test]
    fn si_millihertz() {
        let input = "mHz";
        let span1 = "^-^";
        let span2 = "| ";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::si_name];
        let expected3 = [Rule::si_prefix];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span1, expected2));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected3));
    }

    #[test]
    fn si_gibibytes_per_second() {
        let input = "GiB / s";
        let span1 = "^-----^";
        let span2 = "^-^ | |";
        let span3 = "^^     ";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::si_name, Rule::si_op, Rule::si_name];
        let expected3 = [Rule::bin_prefix];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0], span3, expected3));
    }

    #[test]
    fn si_unit_of() {
        let input = "unit_of(#./position) / s";
        let span1 = "^----------------------^";
        let span2 = "^-----^^-----------^ | |";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::si_fn, Rule::call_arguments, Rule::si_op, Rule::si_name];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
    }

    #[test]
    fn si_scaling() {
        let input = "1024 * B";
        let span1 = "^------^";
        let span2 = "^--^ | |";
        let expected1 = [Rule::si_expr];
        let expected2 = [Rule::dec_lit_raw, Rule::si_op, Rule::si_name];
        let parsed = Lexer::parse(Rule::si_expr, input).unwrap();
        assert!(verify(parsed.clone(), vec![], span1, expected1));
        assert!(verify(parsed.clone(), vec![0], span2, expected2));
    }

    #[test]
    fn type_alias_definition() {
        let input = "type UserTy = autonum<0, 1 ..= 10>;";
        let span1 = "^---------------------------------^";
        let span2 = "     ^----^   ^------------------^ ";
        let span3 = "              ^-----^^-----------^ ";
        let expected1 = [Rule::type_alias_def];
        let expected2 = [Rule::identifier, Rule::generic_ty];
        let expected3 = [Rule::identifier, Rule::generics];
        let parsed = Lexer::parse(Rule::definition, input).unwrap();
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
        assert!(verify(parsed.clone(), vec![0, 0, 1], span3, expected3));
    }

    #[test]
    fn enum_definition_simple() {
        let input = "enum UserEnum { Di1, Di2, }";
        let span1 = "^-------------------------^";
        let span2 = "     ^------^   ^-^  ^-^   ";
        let expected1 = [Rule::enum_def];
        let expected2 = [Rule::identifier, Rule::enum_item, Rule::enum_item];
        let parsed = Lexer::parse(Rule::definition, input).unwrap();
        assert!(verify(parsed.clone(), vec![0], span1, expected1));
        assert!(verify(parsed.clone(), vec![0, 0], span2, expected2));
    }
}