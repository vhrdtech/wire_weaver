
// pub struct Lexer<'input> {
//     input: &'input str,
//     chars: CharLocations<'input>,
//
// }

#[cfg(test)]
mod test {
    use crate::token::Token;
    use std::str::{Chars, CharIndices};
    use std::iter::Peekable;

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

    /// Helper type that create (start, end) iterator over strings like "^ ^--^ ^^"
    struct Highlighter<'a> {
        spans: Peekable<CharIndices<'a>>,
    }
    impl<'a> Highlighter<'a> {
        fn new(spans: &'a str) -> Highlighter<'a> {
            Self {
                spans: spans.char_indices().peekable(),
            }
        }
    }
    impl<'a> Iterator for Highlighter<'a> {
        type Item = (usize, usize);

        fn next(&mut self) -> Option<Self::Item> {
            match self.spans.next() {
                Some((start, c)) => {
                    assert_eq!(c, '^');

                    let mut end = start;
                    loop {
                        let mut end_found = false;
                        match self.spans.next() {
                            Some((pos, c)) => {
                                if c.is_whitespace() {
                                    break;
                                } else if c == '-' {
                                    match self.spans.peek() {
                                        Some((_, c)) => {
                                            if c.is_whitespace() {
                                                panic!("Wrong highlighter string: ^--^ sequence unterminated");
                                            }
                                        }
                                        None => {
                                            panic!("Wrong highlighter string: ^--^ sequence unterminated");
                                        }
                                    }
                                    continue;
                                } else if c == '^' {
                                    end = pos;
                                } else {
                                    panic!("Wrong highlighter string: only '^', '-' and ' ' are allowed");
                                }
                            }
                            None => {
                                break;
                            }
                        }
                    }
                    Some((start, end))
                }
                None => None
            }
        }
    }

    #[test]
    fn test_highlighter() {
        let mut hl = Highlighter::new("^");
        assert_eq!(hl.next(), Some((0, 0)));
        assert_eq!(hl.next(), None);

        let mut hl = Highlighter::new("^ ^");
        assert_eq!(hl.next(), Some((0, 0)));
        assert_eq!(hl.next(), Some((2, 2)));
        assert_eq!(hl.next(), None);

        let mut hl = Highlighter::new("^-^ ^^ ^--^ ^ ^----^");
        assert_eq!(hl.next(), Some((0, 2)));
        assert_eq!(hl.next(), Some((4, 5)));
        assert_eq!(hl.next(), Some((7, 10)));
        assert_eq!(hl.next(), Some((12, 12)));
        assert_eq!(hl.next(), Some((14, 19)));
        assert_eq!(hl.next(), None);

        let mut hl = Highlighter::new("^--");
        let r = std::panic::catch_unwind(move || hl.next());
        assert!(r.is_err());

        let mut hl = Highlighter::new("^-- ");
        let r = std::panic::catch_unwind(move || hl.next());
        assert!(r.is_err());
    }
}