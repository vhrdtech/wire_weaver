#[derive(Parser)]
#[grammar = "mquote.pest"]
pub struct MQuoteLexer;

#[cfg(test)]
mod tests {
    use crate::pest::Parser;
    use super::{Rule, MQuoteLexer};

    struct TestToken {
        start: usize,
        end: usize,
        rule: Rule,
    }

    impl parser_test::Token for TestToken {
        type Rule = Rule;

        fn start(&self) -> usize {
            self.start
        }

        fn end(&self) -> usize {
            self.end
        }

        fn rule(&self) -> Self::Rule {
            self.rule
        }
    }

    fn verify<I>(input: &str, spans: &str, expected: I) -> bool
        where I: IntoIterator<Item = Rule>,
    {
        let mut output = MQuoteLexer::parse(Rule::interpolate_repetition, input).unwrap();
        // println!("{:?}", output);
        let output = output.next().unwrap().into_inner().map(|t| {
            let span = t.as_span();
            TestToken {
                start: span.start(),
                end: span.end() - 1,
                rule: t.as_rule()
            }
        });
        parser_test::test(output, expected, spans)
    }

    #[test]
    fn repetition_simplest() {
        let input = "#(#items)*";
        let spans = "  ^----^  ";
        let expected = [Rule::interpolate];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_simplest_spaced() {
        let input = "# ( # items ) *";
        let spans = "    ^-----^   ";
        let expected = [Rule::interpolate];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_with_interpolate_path() {
        let input = "#(#{self.items})*";
        let spans = "  ^-----------^ ";
        let expected = [Rule::interpolate];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_separator() {
        let input = "#(#items),*";
        let spans = "  ^----^ ^ ";
        let expected = [Rule::interpolate, Rule::repetition_separator];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_separator_spaced() {
        let input = "#(#items) , *";
        let spans = "  ^----^  ^ ";
        let expected = [Rule::interpolate, Rule::repetition_separator];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_separator_is_star() {
        let input = "#(#items)**";
        let spans = "  ^----^ ^ ";
        let expected = [Rule::interpolate, Rule::repetition_separator];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_separator_is_star_spaced() {
        let input = "#(#items) * *";
        let spans = "  ^----^  ^  ";
        let expected = [Rule::interpolate, Rule::repetition_separator];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_token_trees() {
        let input = "#( a b #items x y )*";
        let spans = "   ^ ^ ^----^ ^ ^ ";
        let expected = [
            Rule::token_except_delimiters,
            Rule::token_except_delimiters,
            Rule::interpolate,
            Rule::token_except_delimiters,
            Rule::token_except_delimiters,
        ];
        assert!(verify(input, spans, expected));
    }

    #[test]
    fn repetition_key_value() {
        let input = "#( a b #k x y #v z );*";
        let spans = "   ^ ^ ^^ ^ ^ ^^ ^  ^ ";
        let expected = [
            Rule::token_except_delimiters,
            Rule::token_except_delimiters,
            Rule::interpolate,
            Rule::token_except_delimiters,
            Rule::token_except_delimiters,
            Rule::interpolate,
            Rule::token_except_delimiters,
            Rule::repetition_separator
        ];
        assert!(verify(input, spans, expected));
    }
}