use std::{io, path::PathBuf, process::Stdio};

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
}

impl ClientCommand {
    pub fn classify(line: &[u8]) -> Option<ClientCommand> {
        Some(match line.split(|ch| *ch == b' ').next().unwrap() {
            b"uci" => ClientCommand::Uci,
            b"isready" => ClientCommand::Isready,
            b"go" => ClientCommand::Go,
            b"stop" => ClientCommand::Stop,
            _ => return None,
        })
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
        Some(match line.split(|ch| *ch == b' ').next().unwrap() {
            b"uciok" => EngineCommand::Uciok,
            b"readyok" => EngineCommand::Readyok,
            b"bestmove" => EngineCommand::Bestmove,
            b"info" => EngineCommand::Info,
            _ => return None,
        })
    }
}

pub struct Engine {
    pending_uciok: u64,
    pending_readyok: u64,
    searching: bool,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Engine {
    pub async fn new(path: PathBuf) -> io::Result<Engine> {
        let mut process = Command::new(path)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()?;

        Ok(Engine {
            pending_uciok: 0,
            pending_readyok: 0,
            searching: false,
            stdin: BufWriter::new(
                process.stdin.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdin closed")
                })?,
            ),
            stdout: BufReader::new(process.stdout.take().ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "engine stdout closed")
            })?),
        })
    }
}

impl Engine {
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
        self.stdout.read_until(b'\n', &mut line).await?;
        if line.ends_with(b"\n") {
            line.pop();
        }
        if line.ends_with(b"\r") {
            line.pop();
        }

        let command = EngineCommand::classify(&line);

        match command {
            Some(EngineCommand::Info) => log::debug!("{} >> {}", session.0, String::from_utf8_lossy(&line)),
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
