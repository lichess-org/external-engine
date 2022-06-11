use std::num::NonZeroU32;
use std::time::Duration;
use shakmaty::fen::Fen;
use shakmaty::uci::Uci;
use std::hash::{Hash, Hasher};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

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

#[derive(Clone)]
struct Words<'a> {
    s: &'a str,
}

impl Words<'_> {
    fn new(s: &str) -> Words<'_> {
        Words { s }
    }

    fn eat_while<F>(&mut self, mut pred: F)
    where
        F: FnMut(char) -> bool,
    {
        loop {
            let mut chars = self.s.chars().clone();
            match chars.next() {
                Some(c) if pred(c) => self.s = chars.as_str(),
                _ => break,
            }
        }
    }

    fn ensure_end(&mut self) -> Result<(), UciProtocolError> {
        match self.next() {
            Some(_) => Err(UciProtocolError::ExpectedEol),
            None => Ok(()),
        }
    }

    fn tail(&self) -> &str {
        self.s
    }
}

fn is_sep(c: char) -> bool {
    c == ' ' || c == '\t'
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        self.eat_while(is_sep);
        let mut words = self.clone();
        words.eat_while(|c| !is_sep(c));
        let word = &self.s[..self.s.len() - words.s.len()];
        self.s = words.s;
        if word.len() > 0 {
            Some(word)
        } else {
            None
        }
    }
}

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
    Nop,
}

impl FromStr for UciIn {
    type Err = UciProtocolError;

    fn from_str(s: &str) -> Result<UciIn, UciProtocolError> {
        let mut words = Words::new(s);
        Ok(match words.next() {
            Some("uci") => {
                words.ensure_end()?;
                UciIn::Uci
            },
            Some("isready") => {
                words.ensure_end()?;
                UciIn::Isready
            },
            Some("Ucinewgame") => {
                words.ensure_end()?;
                UciIn::Ucinewgame
            },
            Some("stop") => {
                words.ensure_end()?;
                UciIn::Stop
            },
            Some("ponderhit") => {
                words.ensure_end()?;
                UciIn::Ponderhit
            },
            Some(command @ _) => return Err(UciProtocolError::UnknownCommand { command: command.to_owned() }),
            None =>  UciIn::Nop,
        })
    }
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
        let words: Vec<_> = Words::new("  hello\t world abc ").collect();
        assert_eq!(words, &["hello", "world", "abc"]);
    }
}