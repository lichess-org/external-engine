use std::num::NonZeroU32;
use std::time::Duration;
use shakmaty::fen::Fen;
use shakmaty::uci::Uci;
use std::hash::{Hash, Hasher};
use std::fmt;
use thiserror::Error;
use memchr::memchr2_iter;

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

fn is_separator(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn read(s: &str) -> (Option<&str>, &str) {
    let s = s.trim_start_matches(is_separator);
    if s.is_empty() {
        (None, s)
    } else {
        let (head, tail) = s.split_at(s.find(is_separator).unwrap_or_else(|| s.len()));
        (Some(head), tail)
    }
}

fn read_until<'a>(s: &'a str, token: &str) -> (Option<&'a str>, &'a str) {
    let s = s.trim_start_matches(is_separator);
    if s.is_empty() {
        (None, "")
    } else {
        for end in memchr2_iter(b' ', b'\t', s.as_bytes()) {
            let (head, tail) = s.split_at(end);
            if let (Some(next_token), _) = read(tail) {
                if next_token == token {
                    return (Some(head), tail);
                }
            }
        }
        (Some(s), "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read() {
        assert_eq!(read(""), (None, ""));
        assert_eq!(read(" abc\t def g"), (Some("abc"), "\t def g"));
        assert_eq!(read("  end"), (Some("end"), ""));
    }

    #[test]
    fn test_read_until() {
        assert_eq!(read_until("abc def value foo", "value"), (Some("abc def"), " value foo"));
        assert_eq!(read_until("abc def valuefoo", "value"), (Some("abc def valuefoo"), ""));
        assert_eq!(read_until("value abc", "value"), (Some("value abc"), ""));
    }
}
