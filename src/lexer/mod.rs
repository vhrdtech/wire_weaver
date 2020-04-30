use nom_greedyerror::GreedyError;
use nom_tracable::{tracable_parser, TracableInfo, HasTracableInfo, histogram, cumulative_histogram};
use nom_locate::{LocatedSpan, position};
use nom::bytes::complete::{take_until, tag};
use nom::branch::alt;
use nom::sequence::tuple;
use nom::character::complete::{alpha1, digit1};

#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub struct SpanInfo {
    #[cfg(feature = "trace")]
    pub tracable_info: TracableInfo
}

pub type Span<'a> = LocatedSpan<&'a str, SpanInfo>;
pub type IResult<T, U> = nom::IResult<T, U, GreedyError<T>>;

#[cfg(feature = "trace")]
impl HasTracableInfo for SpanInfo {
    fn get_tracable_info(&self) -> TracableInfo {
        self.tracable_info
    }

    fn set_tracable_info(mut self, info: TracableInfo) -> Self {
        self.tracable_info = info;
        self
    }
}

#[derive(Debug)]
struct Token<'a> {
    pub position: Span<'a>,
    pub foo: &'a str,
}

#[tracable_parser]
fn parse1(s: Span) -> IResult<Span, ()> {
    let (s, _) = take_until("foo")(s)?;
    Ok((s, ()))
}

#[tracable_parser]
fn parse_smth(s: Span) -> IResult<Span, Token> {
    let (s, _) = parse1(s)?;
    let (s, pos) = position(s)?;
    let (s, foo) = tag("foo")(s)?;

    Ok((
        s,
        Token {
            position: pos,
            foo: foo.fragment()
        }
        ))
}

#[tracable_parser]
fn parser(s: Span) -> IResult<Span, (Span, Span, Span)> {
    alt((
        tuple((alpha1, digit1, alpha1)),
        tuple((digit1, alpha1, digit1)),
    ))(s)
}


pub fn lexer_play() {
    println!("lexer_play():");
    let info = TracableInfo::new().parser_width(64).fold("abc012abc");
    let input = Span::new_extra("abc012abc", SpanInfo { tracable_info: info });
    let output = parser(input);
    println!("{:#?}", output);

    histogram();
    cumulative_histogram();
}