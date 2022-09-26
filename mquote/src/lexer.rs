#[derive(Parser)]
#[grammar = "mquote.pest"]
pub struct MQuoteLexer;

// #[cfg(test)]
// mod tests {
//     use super::{MQuoteLexer, Rule};
//     use crate::pest::Parser;
//
//     fn verify_inner<I>(rule: Rule, input: &str, spans: &str, expected: I) -> bool
//     where
//         I: IntoIterator<Item = Rule>,
//     {
//         let mut output = MQuoteLexer::parse(rule, input).unwrap();
//         // println!("{:?}", output);
//         let output = output.next().unwrap().into_inner().map(|t| {
//             let span = t.as_span();
//             parser_test::TestToken {
//                 start: span.start(),
//                 end: span.end() - 1,
//                 rule: t.as_rule(),
//             }
//         });
//         parser_test::test(output, expected, spans)
//     }
//
//     #[test]
//     fn repetition_simplest() {
//         let input = "#(#items)*";
//         let spans = "  ^----^  ";
//         let expected = [Rule::interpolate];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_simplest_spaced() {
//         let input = "# ( # items ) *";
//         let spans = "    ^-----^   ";
//         let expected = [Rule::interpolate];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_with_interpolate_path() {
//         let input = "#(#{self.items})*";
//         let spans = "  ^-----------^ ";
//         let expected = [Rule::interpolate];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_separator() {
//         let input = "#(#items),*";
//         let spans = "  ^----^ ^ ";
//         let expected = [Rule::interpolate, Rule::repetition_separator];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_separator_spaced() {
//         let input = "#(#items) , *";
//         let spans = "  ^----^  ^ ";
//         let expected = [Rule::interpolate, Rule::repetition_separator];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_separator_is_star() {
//         let input = "#(#items)**";
//         let spans = "  ^----^ ^ ";
//         let expected = [Rule::interpolate, Rule::repetition_separator];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_separator_is_star_spaced() {
//         let input = "#(#items) * *";
//         let spans = "  ^----^  ^  ";
//         let expected = [Rule::interpolate, Rule::repetition_separator];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_token_trees() {
//         let input = "#( a b #items x y )*";
//         let spans = "   ^ ^ ^----^ ^ ^ ";
//         let expected = [
//             Rule::token,
//             Rule::token,
//             Rule::interpolate,
//             Rule::token,
//             Rule::token,
//         ];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
//
//     #[test]
//     fn repetition_key_value() {
//         let input = "#( a b #k x y #v z );*";
//         let spans = "   ^ ^ ^^ ^ ^ ^^ ^  ^ ";
//         let expected = [
//             Rule::token,
//             Rule::token,
//             Rule::interpolate,
//             Rule::token,
//             Rule::token,
//             Rule::interpolate,
//             Rule::token,
//             Rule::repetition_separator,
//         ];
//         assert!(verify_inner(
//             Rule::interpolate_repetition,
//             input,
//             spans,
//             expected
//         ));
//     }
// }
