use std::num::NonZeroU32;
use std::time::Duration;
use combine::parser::repeat::repeat_until;
use shakmaty::fen::Fen;
use shakmaty::uci::Uci;
use std::hash::{Hash, Hasher};
use std::fmt;
use thiserror::Error;

use combine::{Parser, skip_many, satisfy, Stream, choice, eof, skip_many1, attempt, many1, not_followed_by, optional};
use combine::parser::char::string;

#[derive(Clone, Debug, Eq)]
pub struct UciOptionName(String);

impl PartialEq for UciOptionName {
    fn eq(&self, other: &UciOptionName) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Hash for UciOptionName {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        for byte in self.0.bytes() {
            hasher.write_u8(byte.to_ascii_lowercase());
        }
        hasher.write_u8(0xff);
    }
}

impl fmt::Display for UciOptionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UciOptionValue(String);

enum UciOption {
    Check {
        default: bool,
    },
    Spin {
        default: i64,
        min: i64,
        max: i64,
    },
    Combo {
        default: String,
        var: Vec<String>,
    },
    Button,
    String {
        default: String,
    }
}

#[derive(Error, Debug)]
enum UciProtocolError {
    #[error("unexpected line break in uci command")]
    UnexpectedLineBreak,
    #[error("unknown command: {command}")]
    UnknownCommand { command: String },
    #[error("expected eol, got token")]
    ExpectedEol,
}

fn ws<Input>() -> impl Parser<Input, Output=char>
where
    Input: Stream<Token = char>
{
    satisfy(|c| c == ' ' || c == '\t')
}

fn text<Input>() -> impl Parser<Input, Output=char>
where
    Input: Stream<Token = char>
{
    satisfy(|c| c != '\r' && c != '\n')
}

fn setoption<Input>() -> impl Parser<Input, Output=UciIn>
where
    Input: Stream<Token = char>
{
    string("setoption")
        .skip(skip_many1(ws()))
        .skip(string("name"))
        .skip(skip_many1(ws()))
        .with(
            text().and(
            repeat_until(text(), attempt(skip_many1(ws()).with(string("value")))))
        )
        .and(
            optional(
                skip_many1(ws())
                    .skip(string("value"))
                    .skip(skip_many1(ws()))
                    .with(many1(text()))
            )
        )
        .map(|(name, value): ((char, String), Option<String>)| UciIn::Uci)
}

fn uci_in<Input>() -> impl Parser<Input, Output=UciIn>
where
    Input: Stream<Token = char>
{
    skip_many(ws())
        .with(choice((
            attempt(string("uci").map(|_| UciIn::Uci)),
            attempt(string("isready").map(|_| UciIn::Isready)),
            attempt(string("ucinewgame").map(|_| UciIn::Ucinewgame)),
            attempt(string("stop").map(|_| UciIn::Stop)),
            attempt(string("ponderhit").map(|_| UciIn::Ponderhit)),
            attempt(setoption()),
        )))
        .skip(skip_many(ws()))
        .skip(eof())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UciIn {
    Uci,
    Isready,
    Setoption { name: UciOptionName, value: Option<UciOptionValue> },
    Ucinewgame,
    Position { fen: Option<Fen>, moves: Vec<Uci> },
    Go {
        searchmoves: Option<Vec<Uci>>,
        ponder: bool,
        wtime: Option<Duration>,
        btime: Option<Duration>,
        winc: Option<Duration>,
        binc: Option<Duration>,
        movestogo: Option<u32>,
        depth: Option<u32>,
        nodes: Option<u64>,
        mate: Option<u32>,
        movetime: Option<Duration>,
        infinite: bool,
    },
    Stop,
    Ponderhit,
}

enum Eval {
    Cp(i64),
    Mate(i32),
    MateGiven,
}

struct Score {
    eval: Eval,
    lowerbound: bool,
    upperbound: bool,
}

enum UciOut {
    IdName,
    IdAuthor,
    Uciok,
    Readyok,
    Bestmove { m: Option<Uci>, ponder: Option<Uci> },
    Info {
        depth: Option<u32>,
        seldepth: Option<u32>,
        time: Option<Duration>,
        nodes: Option<u64>,
        pv: Option<Vec<Uci>>,
        multipv: Option<NonZeroU32>,
        score: Option<Score>,
        currmove: Option<Uci>,
        currmovenumber: Option<u32>,
        hashfull: Option<u32>,
        nps: Option<u64>,
        tbhits: Option<u64>,
        sbhits: Option<u64>,
        cpuload: Option<u32>,
        string: String,
        refutation: (Uci, Vec<Uci>), // at least 1
        currline: (u64, Vec<Uci>),
    },
    Option {
        name: UciOptionName,
        option: UciOption,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use combine::EasyParser;

    #[test]
    fn test_words() {
        //assert_eq!(uci_in().easy_parse(" uci \t "), Ok((UciIn::Uci, "")));

        if let Err(err) = uci_in().easy_parse(combine::stream::position::Stream::new(" setoption name hi there value foo \t ")) {
            panic!("{}", err);
        }
    }
}
