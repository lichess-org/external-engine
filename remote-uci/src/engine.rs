use std::{io, iter::zip, path::PathBuf, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    process::{ChildStdin, ChildStdout, Command},
};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Session(pub u64);

#[derive(Eq, PartialEq)]
pub enum ClientCommand {
    Uci,
    Isready,
    Go,
    Stop,
    Setoption,
}

impl ClientCommand {
    pub fn classify(line: &[u8]) -> Option<ClientCommand> {
        Some(
            match line
                .trim_ascii_start()
                .split(|ch| *ch == b' ')
                .next()
                .unwrap()
            {
                b"uci" => ClientCommand::Uci,
                b"isready" => ClientCommand::Isready,
                b"go" => ClientCommand::Go,
                b"stop" => ClientCommand::Stop,
                b"setoption" => ClientCommand::Setoption,
                _ => return None,
            },
        )
    }
}

pub enum EngineCommand {
    Uciok,
    Readyok,
    Bestmove,
    Info,
}

impl EngineCommand {
    pub fn classify(line: &[u8]) -> Option<EngineCommand> {
        Some(
            match line
                .trim_ascii_start()
                .split(|ch| *ch == b' ')
                .next()
                .unwrap()
            {
                b"uciok" => EngineCommand::Uciok,
                b"readyok" => EngineCommand::Readyok,
                b"bestmove" => EngineCommand::Bestmove,
                b"info" => EngineCommand::Info,
                _ => return None,
            },
        )
    }
}

pub struct Engine {
    pending_uciok: u64,
    pending_readyok: u64,
    searching: bool,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

#[derive(Default, Debug)]
pub struct EngineInfo {
    pub name: Option<String>,
    pub max_threads: Option<usize>,
    pub max_hash: Option<u64>,
    pub variants: Vec<String>,
}

impl Engine {
    pub async fn new(path: PathBuf) -> io::Result<(Engine, EngineInfo)> {
        // TODO: Should wrap with safe-uci.

        let mut process = Command::new(path)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()?;

        let mut engine =
            Engine {
                pending_uciok: 0,
                pending_readyok: 0,
                searching: false,
                stdin: BufWriter::new(process.stdin.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdin closed")
                })?),
                stdout: BufReader::new(process.stdout.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdout closed")
                })?),
            };

        let info = engine.engine_info(Session(0)).await?;
        Ok((engine, info))
    }

    async fn engine_info(&mut self, session: Session) -> io::Result<EngineInfo> {
        let mut info = EngineInfo::default();
        self.send(session, b"uci").await?;
        while !self.is_idle() {
            let line = self.recv(session).await?;
            let mut parts = line.split(|c| c.is_ascii_whitespace());
            match parts.next().unwrap() {
                b"id" => match parts.next() {
                    Some(b"name") => {
                        info.name = Some(String::from_utf8_lossy(parts.as_slice()).into_owned())
                    }
                    _ => (),
                },
                b"option" => match parts.next() {
                    // Quick and dirty parsing of available options.
                    Some(b"name") => match parts.next() {
                        Some(b"Hash") => match parts.next() {
                            Some(b"type") => {
                                info.max_hash = parts
                                    .skip_while(|part| part != b"max")
                                    .skip(1)
                                    .next()
                                    .and_then(|part| btoi::btou(part).ok())
                            }
                            _ => (),
                        },
                        Some(b"Threads") => match parts.next() {
                            Some(b"type") => {
                                info.max_threads = parts
                                    .skip_while(|part| part != b"max")
                                    .skip(1)
                                    .next()
                                    .and_then(|part| btoi::btou(part).ok())
                            }
                            _ => (),
                        },
                        Some(b"UCI_Variant") => match parts.next() {
                            Some(b"type") => {
                                info.variants = zip(parts.clone().skip(1), parts)
                                    .filter_map(|(l, r)| {
                                        if l == b"option" {
                                            String::from_utf8(r.to_owned()).ok()
                                        } else {
                                            None
                                        }
                                    })
                                    .collect()
                            }
                            _ => (),
                        },
                        _ => (),
                    },
                    _ => (),
                },
                _ => (),
            }
        }
        Ok(info)
    }

    pub async fn send(&mut self, session: Session, line: &[u8]) -> io::Result<()> {
        if line.contains(&b'\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "disallowed line feed",
            ));
        }

        match ClientCommand::classify(line) {
            Some(ClientCommand::Uci) => self.pending_uciok += 1,
            Some(ClientCommand::Isready) => self.pending_readyok += 1,
            Some(ClientCommand::Go) => {
                if self.searching {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "already searching",
                    ));
                }
                self.searching = true;
            }
            _ => (),
        }

        log::info!("{} << {}", session.0, String::from_utf8_lossy(line));
        self.stdin.write_all(line).await?;
        self.stdin.write_all(b"\r\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub async fn recv(&mut self, session: Session) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        if self.stdout.read_until(b'\n', &mut line).await? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "engine stdout closed",
            ));
        }
        if line.ends_with(b"\n") {
            line.pop();
        }
        if line.ends_with(b"\r") {
            line.pop();
        }

        let command = EngineCommand::classify(&line);

        match command {
            Some(EngineCommand::Info) => {
                log::debug!("{} >> {}", session.0, String::from_utf8_lossy(&line))
            }
            _ => log::info!("{} >> {}", session.0, String::from_utf8_lossy(&line)),
        }

        match command {
            Some(EngineCommand::Uciok) => self.pending_uciok = self.pending_uciok.saturating_sub(1),
            Some(EngineCommand::Readyok) => {
                self.pending_readyok = self.pending_readyok.saturating_sub(1)
            }
            Some(EngineCommand::Bestmove) => self.searching = false,
            _ => (),
        }

        Ok(line)
    }

    pub fn is_searching(&self) -> bool {
        self.searching
    }

    pub fn is_idle(&self) -> bool {
        self.pending_uciok == 0 && self.pending_readyok == 0 && !self.searching
    }

    pub async fn ensure_idle(&mut self, session: Session) -> io::Result<()> {
        while !self.is_idle() {
            if self.searching && self.pending_readyok < 1 {
                self.send(session, b"stop").await?;
                self.send(session, b"isready").await?;
            }
            self.recv(session).await?;
        }
        Ok(())
    }

    pub async fn ensure_newgame(&mut self, session: Session) -> io::Result<()> {
        self.ensure_idle(session).await?;
        self.send(session, b"ucinewgame").await?;
        self.send(session, b"isready").await?;
        self.ensure_idle(session).await?;
        Ok(())
    }
}
