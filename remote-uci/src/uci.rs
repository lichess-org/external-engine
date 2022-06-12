use std::num::NonZeroU32;
use std::time::Duration;
use shakmaty::fen::Fen;
use shakmaty::uci::Uci;
use std::hash::{Hash, Hasher};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use nom::IResult;
use nom::character::complete::{one_of, not_line_ending};
use nom::multi::{fold_many0, fold_many1};
use nom::sequence::delimited;
use nom::bytes::complete::tag;
use nom::combinator::value;
use nom::combinator::all_consuming;
use nom::branch::alt;
use nom::sequence::tuple;
use nom::combinator::map;
use nom::multi::many1;
use nom::combinator::opt;

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

#[derive(Debug, Clone)]
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

fn ws0(input: &str) -> IResult<&str, ()> {
    fold_many0(one_of(" \t"), || (), |_, _| ())(input)
}

fn ws1(input: &str) -> IResult<&str, ()> {
    fold_many1(one_of(" \t"), || (), |_, _| ())(input)
}

fn setoption(input: &str) -> IResult<&str, UciIn> {
    map(tuple((
        tag("setoption"),
        ws1,
        tag("name"),
        ws1,
        not_line_ending,
        opt(tuple((
            ws1,
            tag("value"),
            not_line_ending
        )))
    )), |(_, _, _, _, name, value)| {
        assert!(value.is_some());
        UciIn::Setoption { name: UciOptionName("".to_string()), value: None }
    })(input)
}

fn uci_in(input: &str) -> IResult<&str, UciIn> {
    all_consuming(
        delimited(
            ws0,
            alt((
                value(UciIn::Uci, tag("uci")),
                value(UciIn::Isready, tag("isready")),
                value(UciIn::Stop, tag("stop")),
                value(UciIn::Ponderhit, tag("ponderhit")),
                value(UciIn::Ucinewgame, tag("ucinewgame")),
                setoption,
            )),
            ws0
        )
    )(input)
}

#[derive(Debug, Clone)]
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

    #[test]
    fn test_words() {
        dbg!(uci_in(" setoption name hello world value foo \t")).unwrap();
    }
}
