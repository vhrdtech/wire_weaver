use crate::token_tree::TokenTree;
use crate::{Group, Spacing};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::iter::FromIterator;

#[derive(Clone, Eq, PartialEq)]
pub struct TokenStream {
    pub(crate) inner: VecDeque<TokenTree>,
}

pub trait ToTokens {
    /// Convert self into a set of tokens and append them to the provided TokenStream.
    fn to_tokens(&self, tokens: &mut TokenStream);
}

impl TokenStream {
    pub fn new() -> Self {
        TokenStream {
            inner: VecDeque::new(),
        }
    }

    /// Modify spacing of the latest token tree inserted.
    /// Used to remove spaces with ∅ sign it mquote! when this is important.
    pub fn modify_last_spacing(&mut self, spacing: Spacing) {
        match self.inner.back_mut() {
            Some(tt) => tt.modify_spacing(spacing),
            None => {}
        }
    }

    /// Replace all groups that contain [TokenTree::Repetition] inside with many groups, each containing
    /// it's own tokens.
    pub fn interpolate_repetitions(&mut self, streams_at: HashMap<u32, VecDeque<TokenStream>>)
    {
        // for (idx, streams) in streams_at {
        //     println!("RI{} = {:?}", idx, streams);
        // }
        self.inner = Self::interpolate_repetitions_inner(self, &streams_at);
    }

    fn interpolate_repetitions_inner(
        ts: &mut TokenStream,
        streams_at: &HashMap<u32, VecDeque<TokenStream>>) -> VecDeque<TokenTree>
    {
        let mut tts_reassemble = VecDeque::new();
        while let Some(t) = ts.inner.pop_front() {
            match t {
                TokenTree::RepetitionGroup(group_ts, separator) => {
                    // collect all repetitions only at this group level
                    let mut streams_in_this_group: HashMap<u32, VecDeque<TokenStream>> = HashMap::new();
                    let mut streams_len = None;
                    for t in &group_ts.inner {
                        if let TokenTree::Repetition(idx) = t {
                            let streams = streams_at.get(&(*idx as u32)).unwrap().clone();
                            let next_streams_len = streams.len();
                            streams_in_this_group.insert(*idx as u32, streams);
                            streams_len = match streams_len {
                                Some(len) => {
                                    if len != next_streams_len {
                                        panic!("Repetition iterables must be of the same length inside each group, at repetition #{}", idx)
                                    }
                                    Some(len)
                                }
                                None => Some(next_streams_len)
                            };
                        }
                    }
                    // iterate over them all in parallel appending to tts_reassemble
                    let streams_len = streams_len.unwrap();
                    for i in 0..streams_len {
                        for t in &group_ts.inner {
                            match t.clone() {
                                TokenTree::Repetition(idx) => {
                                    match streams_in_this_group.get_mut(&(idx as u32)) {
                                        Some(streams) => {
                                            match streams.pop_front() {
                                                Some(repeat_over_ts) => {
                                                    for t in repeat_over_ts.inner {
                                                        tts_reassemble.push_back(t);
                                                    }
                                                }
                                                None => {}
                                            }
                                        }
                                        None => {
                                            panic!("Internal error in interpolate_repetitions_inner: {}", idx)
                                        }
                                    }
                                }
                                TokenTree::RepetitionGroup(mut group_ts, separator) => {
                                    for tt in Self::interpolate_repetitions_inner(&mut group_ts, streams_at) {
                                        tts_reassemble.push_back(tt);
                                        match separator.clone() {
                                            Some(p) => tts_reassemble.push_back(p.into()),
                                            None => {}
                                        }
                                    }
                                }
                                TokenTree::Group(mut group) => {
                                    tts_reassemble.push_back(TokenTree::Group(Group {
                                        delimiter: group.delimiter,
                                        stream: TokenStream {
                                            inner: Self::interpolate_repetitions_inner(&mut group.stream, streams_at),
                                        },
                                    }));
                                }
                                any_else => {
                                    tts_reassemble.push_back(any_else.clone());
                                }
                            }
                        }
                        if i < streams_len - 1 {
                            match separator.clone() {
                                Some(p) => tts_reassemble.push_back(p.into()),
                                None => {}
                            }
                        }
                    }
                }
                TokenTree::Repetition(_) => {
                    panic!("Repetition can only be inside a repetition group delimited by ⸨ ⸩");
                }
                TokenTree::Group(mut group) => {
                    tts_reassemble.push_back(TokenTree::Group(Group {
                        delimiter: group.delimiter,
                        stream: TokenStream {
                            inner: Self::interpolate_repetitions_inner(&mut group.stream, streams_at),
                        },
                    }));
                }
                any_else => {
                    tts_reassemble.push_back(any_else);
                }
            }
        }
        tts_reassemble
    }

    // /// Recreate proper tree structure after using interpolation with escaped delimiters.
    // ///
    // /// For example if `#( #methods \\( #names \\) ?; )*` construction was used in mquote macro,
    // /// token stream will contain DelimiterRaw tokens flat with other tokens (no nested groups):
    // /// `Id(fun1) DR( Id(self) P. Id(x) DR) P; Id(fun2) DR( Id(self) P. Id(y) DR) P;`
    // /// will be turned into
    // /// `Id(fun1) G( Id(self) P. Id(x)  ) P; Id(fun2) G( Id(self) P. Id(y)  ) P;`
    // /// Note that first token stream is flat while the second has two nested groups.
    // ///
    // /// # Panics
    // ///
    // /// Panics if:
    // /// * Unterminated opening or closing raw delimiter is encountered.
    // /// * Non matching closing delimiter is encountered.
    // pub fn recreate_trees(&mut self) {
    //     self.inner = Self::collect_inner(self, None);
    // }
    //
    // fn collect_inner(ts: &mut TokenStream, raw: Option<DelimiterRaw>) -> VecDeque<TokenTree> {
    //     let mut tts_reassemble = VecDeque::new();
    //     while let Some(t) = ts.inner.pop_front() {
    //         match t {
    //             TokenTree::DelimiterRaw(delim) => {
    //                 if delim.is_open() {
    //                     tts_reassemble.push_back(TokenTree::Group(Group {
    //                         delimiter: delim.clone().into(),
    //                         stream: TokenStream {
    //                             inner: Self::collect_inner(ts, Some(delim)),
    //                         },
    //                     }));
    //                 } else {
    //                     match raw {
    //                         Some(open_raw_delim) => {
    //                             if !open_raw_delim.is_same_kind(delim) {
    //                                 panic!(
    //                                     "Open delimiter was: {:?} got non matching closing: {:?}",
    //                                     open_raw_delim, delim
    //                                 )
    //                             }
    //                         }
    //                         None => panic!("Unexpected closing raw delimiter: {:?}", delim),
    //                     }
    //                     return tts_reassemble;
    //                 }
    //             }
    //             TokenTree::Group(mut group) => {
    //                 tts_reassemble.push_back(TokenTree::Group(Group {
    //                     delimiter: group.delimiter,
    //                     stream: TokenStream {
    //                         inner: Self::collect_inner(&mut group.stream, None),
    //                     },
    //                 }));
    //             }
    //             any_else => {
    //                 tts_reassemble.push_back(any_else);
    //             }
    //         }
    //     }
    //     match raw {
    //         Some(open_raw_delim) => panic!("Unterminated raw delimiter: {:?}", open_raw_delim),
    //         None => {}
    //     }
    //     tts_reassemble
    // }
}

impl Display for TokenStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut joint = false;
        for (i, tt) in self.inner.iter().enumerate() {
            if i != 0 && !joint {
                write!(f, " ")?;
            }
            joint = false;
            match tt {
                TokenTree::Group(tt) => Display::fmt(tt, f),
                TokenTree::Ident(tt) => {
                    joint = tt.spacing() == Spacing::Joint;
                    Display::fmt(tt, f)
                },
                TokenTree::Punct(tt) => {
                    joint = tt.spacing() == Spacing::Joint;
                    Display::fmt(tt, f)
                }
                // TokenTree::DelimiterRaw(tt) => Display::fmt(tt, f),
                TokenTree::Literal(tt) => {
                    joint = tt.spacing() == Spacing::Joint;
                    Display::fmt(tt, f)
                },
                TokenTree::Comment(tt) => Display::fmt(tt, f),
                // TokenTree::InterpolateOne(idx) => write!(f, "#{}", idx),
                // TokenTree::InterpolateIter(idx) => write!(f, "I{}", idx),
                TokenTree::Repetition(idx) => write!(f, "RI{}", idx),
                TokenTree::RepetitionGroup(g, p) => write!(f, "RG⸨{} {:?}⸩", g, p),
            }?;
        }

        Ok(())
    }
}

impl From<TokenTree> for TokenStream {
    fn from(tree: TokenTree) -> TokenStream {
        let mut stream = TokenStream::new();
        // stream.push_token(tree);
        stream.inner.push_back(tree);
        stream
    }
}

impl FromIterator<TokenTree> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenTree>>(tokens: I) -> Self {
        let mut stream = TokenStream::new();
        stream.extend(tokens);
        stream
    }
}

impl FromIterator<TokenStream> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenStream>>(streams: I) -> Self {
        let mut v = VecDeque::new();

        for stream in streams {
            v.extend(stream.inner);
        }

        TokenStream { inner: v }
    }
}

impl Extend<TokenTree> for TokenStream {
    fn extend<T: IntoIterator<Item = TokenTree>>(&mut self, tokens: T) {
        tokens.into_iter().for_each(|tt| self.inner.push_back(tt));
    }
}

impl Extend<TokenStream> for TokenStream {
    fn extend<T: IntoIterator<Item = TokenStream>>(&mut self, stream: T) {
        self.inner.extend(stream.into_iter().flatten());
    }
}

impl ToTokens for TokenStream {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(self.clone().into_iter())
    }
}

pub(crate) type TokenTreeIter = std::collections::vec_deque::IntoIter<TokenTree>;

impl IntoIterator for TokenStream {
    type Item = TokenTree;
    type IntoIter = TokenTreeIter;

    fn into_iter(self) -> TokenTreeIter {
        self.inner.into_iter()
    }
}

impl Debug for TokenStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.inner.is_empty() {
            write!(f, "TS{{∅}}")?;
        } else {
            write!(f, "TS{{ ")?;
            for t in &self.inner {
                if f.alternate() {
                    write!(f, "{:#?} ", t)?;
                } else {
                    write!(f, "{:?} ", t)?;
                }
            }
            write!(f, "}}")?;
        }
        Ok(())
    }
}
