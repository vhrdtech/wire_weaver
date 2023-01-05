use crate::lexer::Rule;
use ast::{Span, SpanOrigin};
use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::files::SimpleFile;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use pest::error::InputLocation;
use std::fmt::{Display, Formatter};
use std::ops::Range;
use thiserror::Error;

#[derive(Error, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub origin: SpanOrigin,
    pub input: String,
}

#[derive(Error, Debug)]
pub enum ErrorKind {
    #[error("Source contains syntax errors")]
    Grammar(pest::error::Error<Rule>),
    #[error("Source contains structural errors")]
    Parser(Vec<ParseError>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub rule: crate::lexer::Rule,
    pub span: (usize, usize),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ParseErrorKind {
    InternalError {
        rule: Option<Rule>,
        message: &'static str,
    },
    Unimplemented(&'static str),
    UnhandledUnexpectedInput {
        expect1: Option<Rule>,
        expect2: Option<Rule>,
        got: Option<Rule>,
        context: &'static str,
        span: Span,
    },
    UserError,
    UnexpectedUnconsumedInput(String),
    EmptyInput,

    AutonumWrongForm,
    AutonumWrongArguments,
    IndexOfWrongForm,
    FloatParseError,
    IntParseError,
    MalformedResourcePath,
    WrongAccessModifier,
    CellWithAccessModifier,
    FnWithMods,
    ConstWithMods,
    WoObserve,
    StreamWithoutDirection,

    ArrayFillLitWithNotDiscreteSize,
    ArrayFillLitWrongSize,
}

#[derive(Error, Debug)]
pub enum ParseErrorSource {
    /// Parser internal error.
    /// unreachable() and unwrap()'s are converted into this error as well.
    /// Will be pushed onto error list in `ast/file.rs`, so that no errors are silently ignored.
    /// More precise errors might be pushed onto the same list by parsers.
    /// TODO: add auto link to github here
    #[error("Parser internal error, please file a bug is one doesn't yet exists.")]
    InternalError {
        rule: Option<Rule>,
        message: &'static str,
    },
    /// Parser feature unimplemented
    /// TODO: add link to feature status on github here
    #[error(
    "Parser feature unimplemented, consider contributing or look at features status here: _"
    )]
    Unimplemented(&'static str),
    /// Not enough input or unexpected rule (because expected one is absent).
    /// Might not be an error like in enum with only discriminant values.
    /// The only error to be ignored by `parse_or_skip()`, so that parsing of the
    /// current node can continue.
    /// Will be pushed onto error list in `ast/file.rs` if not ignored along the way.
    #[error("Not enough input or unexpected rule (because expected one is absent)")]
    UnexpectedInput {
        expect1: Option<Rule>,
        expect2: Option<Rule>,
        got: Option<Rule>,
        context: &'static str,
        span: Span,
    },
    /// User provided erroneous input, invalid number for example.
    #[error("User provided erroneous input, invalid number for example")]
    UserError,
}

impl ParseErrorSource {
    pub fn internal(message: &'static str) -> ParseErrorSource {
        ParseErrorSource::InternalError {
            rule: None,
            message,
        }
    }

    pub fn internal_with_rule(rule: Rule, message: &'static str) -> ParseErrorSource {
        ParseErrorSource::InternalError {
            rule: Some(rule),
            message,
        }
    }
}

impl Error {
    pub fn report(&self) -> Vec<Diagnostic<()>> {
        match &self.kind {
            ErrorKind::Grammar(error) => {
                let range = Self::pest_location_to_range(&error.location);
                let renamed = error
                    .clone()
                    .renamed_rules(crate::user_readable::rule_names);
                match &renamed.variant {
                    pest::error::ErrorVariant::ParsingError { .. } => {
                        unreachable!()
                    }
                    pest::error::ErrorVariant::CustomError { message } => {
                        vec![Diagnostic::error()
                            .with_code("E0001")
                            .with_message("grammar error")
                            .with_labels(vec![Label::primary((), range).with_message(message)])]
                    }
                }
            }
            ErrorKind::Parser(errors) => {
                errors.iter().map(Self::parse_error_to_diagnostic).collect()
            }
        }
    }

    fn parse_error_to_diagnostic(error: &ParseError) -> Diagnostic<()> {
        let range = error.span.0..error.span.1;
        match &error.kind {
            ParseErrorKind::InternalError { rule, message } => Diagnostic::bug()
                .with_code("E0002")
                .with_message("internal parser error (unknown)")
                .with_labels(vec![
                    Label::primary((), range).with_message(message.to_owned())
                ])
                .with_notes(vec![format!("grammar rule hint: {:?}", rule)]),
            ParseErrorKind::Unimplemented(thing) => Diagnostic::bug()
                .with_code("E0003")
                .with_message("internal parser error (unimplemented)")
                .with_labels(vec![Label::primary((), range)
                    .with_message(format!("{} is not yet implemented", thing))]),
            ParseErrorKind::UnhandledUnexpectedInput { expect1, expect2, got, context, span } => {
                let note = match (expect1, expect2, got) {
                    (Some(rule1), None, None) => format!("expected: {rule1:?}, got: None"),
                    (Some(rule1), None, Some(got)) => format!("expected: {rule1:?}, got: {got:?}"),

                    (Some(rule1), Some(rule2), None) => format!("expected: {rule1:?} or {rule2:?}, got: None"),
                    (Some(rule1), Some(rule2), Some(got)) => format!("expected: {rule1:?} or {rule2:?}, got: {got:?}"),

                    (None, None, None) => "expected any pair, got: None".to_owned(),
                    (expect1, expect2, got) => format!("bug: expect1: {expect1:?} expect2: {expect2:?} got: {got:?}")
                };
                let range_secondary = span.start..span.end;
                Diagnostic::error()
                    .with_code("E0004")
                    .with_labels(vec![Label::primary((), range), Label::secondary((), range_secondary)])
                    .with_message("unhandled unexpected input (probably a bug)")
                    .with_notes(vec![note, format!("parser context: {}", context)])
            }
            // ParseErrorKind::UserError => {}
            // ParseErrorKind::UnexpectedUnconsumedInput => {}
            // ParseErrorKind::AutonumWrongForm => {}
            // ParseErrorKind::AutonumWrongArguments => {}
            // ParseErrorKind::IndexOfWrongForm => {}
            // ParseErrorKind::FloatParseError => {}
            // ParseErrorKind::IntParseError => {}
            // ParseErrorKind::MalformedResourcePath => {}
            // ParseErrorKind::WrongAccessModifier => {}
            // ParseErrorKind::CellWithAccessModifier => {}
            // ParseErrorKind::FnWithMods => {}
            // ParseErrorKind::ConstWithMods => {}
            // ParseErrorKind::WoObserve => {}
            // ParseErrorKind::CellWithConstRo => {}
            // ParseErrorKind::CellWithRoStream => {}
            kind => Diagnostic::bug()
                .with_code("Exxx")
                .with_message("not yet properly rendered error")
                .with_labels(vec![
                    Label::primary((), range).with_message(format!("error kind: {:?}", kind))
                ]),
        }
    }

    pub fn print_report(&self) {
        let diagnostics = self.report();
        let writer = StandardStream::stderr(ColorChoice::Always);
        let config = codespan_reporting::term::Config::default();
        let file = SimpleFile::new(self.origin.clone(), &self.input);
        for diagnostic in &diagnostics {
            codespan_reporting::term::emit(&mut writer.lock(), &config, &file, diagnostic).unwrap();
        }
    }

    fn pest_location_to_range(loc: &InputLocation) -> Range<usize> {
        match loc {
            InputLocation::Pos(start) => *start..*start,
            InputLocation::Span((start, end)) => *start..*end,
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ErrorKind::Grammar(pest_err) => {
                let ll_file =
                    crate::file_ll::LLFile::parse(self.input.as_str(), self.origin.clone());
                // println!("{:?}", ll_file);
                match ll_file.check_delimiters() {
                    Ok(()) => {
                        // TODO: colorize pest error in the same way
                        writeln!(
                            f,
                            " --> {}\n\x1b[31m{}\x1b[0m",
                            self.origin,
                            pest_err
                                .clone()
                                .renamed_rules(crate::user_readable::rule_names)
                        )
                    }
                    Err(e) => {
                        // Input contains unmatched delimiters, display extensive information about them
                        writeln!(f, "{}", e)

                        // writeln!(
                        //     f,
                        //     " --> {}\n\x1b[31m{}\x1b[0m",
                        //     self.origin,
                        //     pest_err
                        //         .clone()
                        //         .renamed_rules(|r| crate::user_readable::rule_names(r))
                        // )
                    }
                }
            }
            ErrorKind::Parser(parser_errors) => {
                writeln!(f, " --> {}\n{:?}", self.origin, parser_errors)
            }
        }
    }
}
