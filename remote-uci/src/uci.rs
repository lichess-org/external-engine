use std::{
    fmt,
    hash::{Hash, Hasher},
    num::{NonZeroU32, ParseIntError},
    time::Duration,
};

use memchr::{memchr2, memchr2_iter};
use shakmaty::{
    fen::{Fen, ParseFenError},
    uci::{ParseUciError, Uci},
};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UciOptionValue(String);

impl fmt::Display for UciOptionValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

enum UciOption {
    Check { default: bool },
    Spin { default: i64, min: i64, max: i64 },
    Combo { default: String, var: Vec<String> },
    Button,
    String { default: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UciIn {
    Uci,
    Isready,
    Setoption {
        name: UciOptionName,
        value: Option<UciOptionValue>,
    },
    Ucinewgame,
    Position {
        fen: Option<Fen>,
        moves: Vec<Uci>,
    },
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

impl UciIn {
    pub fn from_line(s: &str) -> Result<Option<UciIn>, ProtocolError> {
        Parser::new(s)?.parse_in()
    }
}

impl fmt::Display for UciIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(match self {
            UciIn::Uci => f.write_str("uci")?,
            UciIn::Isready => f.write_str("isready")?,
            UciIn::Setoption { name, value } => {
                write!(f, "setoption name {name}")?;
                if let Some(value) = value {
                    write!(f, " value {value}")?;
                }
            }
            UciIn::Ucinewgame => f.write_str("ucinewgame")?,
            UciIn::Position { fen, moves } => {
                match fen {
                    Some(fen) => write!(f, "position fen {fen}")?,
                    None => f.write_str("position startpos")?,
                }
                if !moves.is_empty() {
                    f.write_str(" moves ")?;
                    for m in moves {
                        write!(f, " {}", m)?;
                    }
                }
            }
            UciIn::Go { searchmoves, ponder, wtime, btime, winc, binc, movestogo, depth, nodes, mate, movetime, infinite } => {
                f.write_str("go")?;
                if let Some(searchmoves) = searchmoves {
                    f.write_str(" searchmoves")?;
                    for m in searchmoves {
                        write!(f, " {}", m)?;
                    }
                }
                if *ponder {
                    f.write_str(" ponder")?;
                }
                if let Some(wtime) = wtime {
                    write!(f, " wtime {}", wtime.as_millis())?;
                }
                if let Some(btime) = btime {
                    write!(f, " btime {}", btime.as_millis())?;
                }
                if let Some(winc) = winc {
                    write!(f, " winc {}", winc.as_millis())?;
                }
                if let Some(binc) = binc {
                    write!(f, " binc {}", binc.as_millis())?;
                }
                if let Some(movestogo) = movestogo {
                    write!(f, " movestogo {movestogo}")?;
                }
                if let Some(depth) = depth {
                    write!(f, " depth {depth}")?;
                }
                if let Some(nodes) = nodes {
                    write!(f, " nodes {nodes}")?;
                }
                if let Some(mate) = mate {
                    write!(f, " mate {mate}")?;
                }
                if let Some(movetime) = movetime {
                    write!(f, " movetime {}", movetime.as_millis())?;
                }
                if *infinite {
                    f.write_str(" infinite")?;
                }
            }
            UciIn::Stop => f.write_str("stop")?,
            UciIn::Ponderhit => f.write_str("ponderhit")?,
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
    IdName(String),
    IdAuthor(String),
    Uciok,
    Readyok,
    Bestmove {
        m: Option<Uci>,
        ponder: Option<Uci>,
    },
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
    },
}

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("unexpected token")]
    UnexpectedToken,
    #[error("unexpected line break in uci command")]
    UnexpectedLineBreak,
    #[error("expected end of line")]
    ExpectedEndOfLine,
    #[error("unexpected end of line")]
    UnexpectedEndOfLine,
    #[error("invalid fen: {0}")]
    InvalidFen(#[from] ParseFenError),
    #[error("invalid move: {0}")]
    InvalidMove(#[from] ParseUciError),
    #[error("invalid integer: {0}")]
    InvalidInteger(#[from] ParseIntError),
}

struct Parser<'a> {
    s: &'a str,
}

impl<'a> Iterator for Parser<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let (head, tail) = read(self.s);
        self.s = tail;
        head
    }
}

impl Parser<'_> {
    pub fn new(s: &str) -> Result<Parser<'_>, ProtocolError> {
        match memchr2(b'\r', b'\n', s.as_bytes()) {
            Some(_) => Err(ProtocolError::UnexpectedLineBreak),
            None => Ok(Parser { s }),
        }
    }

    fn peek(&self) -> Option<&str> {
        let (head, _) = read(self.s);
        head
    }

    fn until(&mut self, token: &str) -> Option<&str> {
        let (head, tail) = read_until(self.s, |t| t == token);
        self.s = tail;
        head
    }

    fn tail(&mut self) -> Option<&str> {
        let (tail, _) = read_until(self.s, |_| false);
        tail
    }

    fn end(&self) -> Result<(), ProtocolError> {
        match self.peek() {
            Some(_) => Err(ProtocolError::ExpectedEndOfLine),
            None => Ok(()),
        }
    }

    fn parse_setoption(&mut self) -> Result<UciIn, ProtocolError> {
        Ok(match self.next() {
            Some("name") => UciIn::Setoption {
                name: UciOptionName(
                    self.until("value")
                        .ok_or(ProtocolError::UnexpectedEndOfLine)?
                        .to_owned(),
                ),
                value: match self.next() {
                    Some("value") => Some(UciOptionValue(
                        self.tail()
                            .ok_or(ProtocolError::UnexpectedEndOfLine)?
                            .to_owned(),
                    )),
                    Some(_) => unreachable!(),
                    None => None,
                },
            },
            Some(_) => return Err(ProtocolError::UnexpectedToken),
            None => return Err(ProtocolError::UnexpectedEndOfLine),
        })
    }

    fn parse_position(&mut self) -> Result<UciIn, ProtocolError> {
        Ok(UciIn::Position {
            fen: match self.until("moves") {
                Some("startpos") => None,
                Some(fen) => Some(fen.parse()?),
                None => return Err(ProtocolError::UnexpectedEndOfLine),
            },
            moves: match self.next() {
                Some("moves") => self
                    .map(|m| m.parse())
                    .collect::<Result<_, ParseUciError>>()?,
                Some(_) => unreachable!(),
                None => Vec::new(),
            },
        })
    }

    fn parse_millis(&mut self) -> Result<Duration, ProtocolError> {
        Ok(Duration::from_millis(
            self.next()
                .ok_or(ProtocolError::UnexpectedEndOfLine)?
                .parse()?,
        ))
    }

    fn parse_searchmoves(&mut self) -> Vec<Uci> {
        let mut searchmoves = Vec::new();
        while let Some(m) = self.peek() {
            match m.parse() {
                Ok(uci) => {
                    self.next();
                    searchmoves.push(uci);
                }
                Err(_) => break,
            }
        }
        searchmoves
    }

    fn parse_go(&mut self) -> Result<UciIn, ProtocolError> {
        let mut searchmoves = None;
        let mut ponder = false;
        let mut wtime = None;
        let mut btime = None;
        let mut winc = None;
        let mut binc = None;
        let mut movestogo = None;
        let mut depth = None;
        let mut nodes = None;
        let mut mate = None;
        let mut movetime = None;
        let mut infinite = false;
        loop {
            match self.next() {
                Some("ponder") => ponder = true,
                Some("infinite") => infinite = true,
                Some("movestogo") => {
                    movestogo = Some(
                        self.next()
                            .ok_or(ProtocolError::UnexpectedEndOfLine)?
                            .parse()?,
                    )
                }
                Some("depth") => {
                    depth = Some(
                        self.next()
                            .ok_or(ProtocolError::UnexpectedEndOfLine)?
                            .parse()?,
                    )
                }
                Some("nodes") => {
                    nodes = Some(
                        self.next()
                            .ok_or(ProtocolError::UnexpectedEndOfLine)?
                            .parse()?,
                    )
                }
                Some("mate") => {
                    mate = Some(
                        self.next()
                            .ok_or(ProtocolError::UnexpectedEndOfLine)?
                            .parse()?,
                    )
                }
                Some("movetime") => movetime = Some(self.parse_millis()?),
                Some("wtime") => wtime = Some(self.parse_millis()?),
                Some("btime") => btime = Some(self.parse_millis()?),
                Some("winc") => winc = Some(self.parse_millis()?),
                Some("binc") => binc = Some(self.parse_millis()?),
                Some("searchmoves") => searchmoves = Some(self.parse_searchmoves()),
                Some(_) => return Err(ProtocolError::UnexpectedToken),
                None => break,
            }
        }
        Ok(UciIn::Go {
            searchmoves,
            ponder,
            wtime,
            btime,
            winc,
            binc,
            movestogo,
            depth,
            nodes,
            mate,
            movetime,
            infinite,
        })
    }

    fn parse_in(&mut self) -> Result<Option<UciIn>, ProtocolError> {
        Ok(Some(match self.next() {
            Some("uci") => {
                self.end()?;
                UciIn::Uci
            }
            Some("isready") => {
                self.end()?;
                UciIn::Isready
            }
            Some("ucinewgame") => {
                self.end()?;
                UciIn::Ucinewgame
            }
            Some("stop") => {
                self.end()?;
                UciIn::Stop
            }
            Some("ponderhit") => {
                self.end()?;
                UciIn::Ponderhit
            }
            Some("setoption") => self.parse_setoption()?,
            Some("position") => self.parse_position()?,
            Some("go") => self.parse_go()?,
            Some(_) => return Err(ProtocolError::UnexpectedToken),
            None => return Ok(None),
        }))
    }

    fn parse_option(&mut self) -> Result<UciOut, ProtocolError> {
        Ok(match self.next() {
            Some("name") => {
                let name = self.until("type").ok_or(ProtocolError::UnexpectedEndOfLine)?;
                self.next().ok_or(ProtocolError::UnexpectedEndOfLine)?; // type
                match self.next() {
                    Some("check") => todo!()
                    Some("spin") => todo!()
                    Some("combo") => todo!()
                    Some("button") => todo!()
                    Some("string") => todo!()
                    Some(_) => return Err(ProtocolError::UnexpectedToken),
                    None => return Err(ProtocolError::UnexpectedEndOfLine),
                }
            }
            Some(_) => return Err(ProtocolError::UnexpectedToken),
            None => return Err(ProtocolError::UnexpectedEndOfLine),
        })
    }

    fn parse_out(&mut self) -> Result<Option<UciOut>, ProtocolError> {
        Ok(Some(match self.next() {
            Some("id") => match self.next() {
                Some("name") => UciOut::IdName(self.tail().ok_or(ProtocolError::UnexpectedEndOfLine)?.to_owned()),
                Some("author") => UciOut::IdAuthor(self.tail().ok_or(ProtocolError::UnexpectedEndOfLine)?.to_owned()),
                Some(_) => return Err(ProtocolError::UnexpectedToken),
                None => return Err(ProtocolError::UnexpectedEndOfLine),
            },
            Some("uciok") => UciOut::Uciok,
            Some("readyok") => UciOut::Readyok,
            Some("bestmove") => todo!(),
            Some("info") => todo!(),
            Some("option") => self.parse_option()?,
            Some(_) => return Err(ProtocolError::UnexpectedToken),
            None => return Ok(None),
        }))
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
        let (head, tail) =
            s.split_at(memchr2(b' ', b'\t', s.as_bytes()).unwrap_or_else(|| s.len()));
        (Some(head), tail)
    }
}

fn read_until<'a, P>(s: &'a str, mut pred: P) -> (Option<&'a str>, &'a str)
where
    P: FnMut(&str) -> bool,
{
    let s = s.trim_start_matches(is_separator);
    if s.is_empty() {
        (None, "")
    } else {
        for end in memchr2_iter(b' ', b'\t', s.as_bytes()) {
            let (head, tail) = s.split_at(end);
            if let (Some(next_token), _) = read(tail) {
                if pred(next_token) {
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
        assert_eq!(
            read_until("abc def value foo", |t| t == "value"),
            (Some("abc def"), " value foo")
        );
        assert_eq!(
            read_until("abc def valuefoo", |t| t == "value"),
            (Some("abc def valuefoo"), "")
        );
        assert_eq!(
            read_until("value abc", |t| t == "value"),
            (Some("value abc"), "")
        );
    }

    #[test]
    fn test_setoption() -> Result<(), ProtocolError> {
        assert_eq!(
            UciIn::from_line("setoption name Skill Level value 10")?,
            Some(UciIn::Setoption {
                name: UciOptionName("skill level".to_owned()),
                value: Some(UciOptionValue("10".to_owned()))
            })
        );

        assert_eq!(
            UciIn::from_line("setoption name Clear Hash")?,
            Some(UciIn::Setoption {
                name: UciOptionName("clEAR haSH".to_owned()),
                value: None
            })
        );

        Ok(())
    }
}
