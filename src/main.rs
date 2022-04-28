use std::{
    error::Error,
    fmt, io,
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use either::{Left, Right};
use rand::random;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines},
    process::{ChildStdin, ChildStdout, Command},
    sync::{Mutex, MutexGuard, Notify},
};

#[derive(Debug, Parser)]
struct Opt {
    engine: PathBuf,
    #[clap(long, default_value = "127.0.0.1:9670")]
    bind: SocketAddr,
}

struct Engine {
    session: AtomicU64,
    notify: Notify,
    pipes: Mutex<EnginePipes>,
}

struct EnginePipes {
    pending_uciok: u64,
    pending_readyok: u64,
    pending_bestmove: u64,
    stdin: BufWriter<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
}

impl Engine {
    async fn new(path: PathBuf) -> io::Result<Engine> {
        let mut process = Command::new(path)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()?;

        Ok(Engine {
            session: AtomicU64::new(0),
            notify: Notify::new(),
            pipes: Mutex::new(EnginePipes {
                pending_uciok: 0,
                pending_bestmove: 0,
                pending_readyok: 0,
                stdin: BufWriter::new(process.stdin.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdin closed")
                })?),
                stdout: BufReader::new(process.stdout.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdout closed")
                })?)
                .lines(),
            }),
        })
    }
}

impl EnginePipes {
    async fn write(&mut self, msg: UciIn) -> io::Result<()> {
        match msg {
            UciIn::Uci => self.pending_uciok += 1,
            UciIn::Isready => self.pending_readyok += 1,
            UciIn::Go(_) => self.pending_bestmove += 1,
            _ => (),
        }

        log::debug!("<< {}", msg);
        self.stdin.write_all(msg.to_string().as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read(&mut self) -> io::Result<Option<UciOut>> {
        let line = match self.stdout.next_line().await? {
            Some(line) => line,
            None => return Ok(None),
        };

        let msg = UciOut::from_str(&line)?;
        log::debug!(">> {}", msg);

        match msg {
            UciOut::Uciok => {
                self.pending_uciok = self.pending_uciok.checked_sub(1).unwrap_or_else(|| {
                    log::warn!("unexpected uciok");
                    0
                })
            }
            UciOut::Readok => {
                self.pending_readyok = self.pending_readyok.checked_sub(1).unwrap_or_else(|| {
                    log::warn!("unexpected readyok");
                    0
                })
            }
            UciOut::Bestmove(_) => {
                self.pending_bestmove = self.pending_bestmove.checked_sub(1).unwrap_or_else(|| {
                    log::warn!("unexpected bestmove");
                    0
                })
            }
            _ => (),
        }

        Ok(Some(msg))
    }

    async fn idle(&mut self) -> io::Result<()> {
        let mut stopped = false;

        while self.pending_uciok > 0 || self.pending_readyok > 0 || self.pending_bestmove > 0 {
            if self.pending_bestmove > 0 && !stopped {
                self.write(UciIn::Stop).await?;
                stopped = true;
            }

            match self.read().await? {
                Some(UciOut::Bestmove(_)) => stopped = false,
                Some(_) => (),
                None => break,
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(
        env_logger::Env::new()
            .filter("REMOTE_UCI_LOG")
            .write_style("REMOTE_UCI_LOG_STYLE"),
    )
    .format_target(false)
    .format_module_path(false)
    .init();

    let opt = Opt::parse();

    let engine = Arc::new(Engine::new(opt.engine).await?);

    let secret_route = Box::leak(format!("/{:032x}", random::<u128>() & 0).into_boxed_str()); // XXX
    log::info!(
        "secret route: file:///home/niklas/Projekte/remote-uci/test.html#{}",
        secret_route
    );

    let app = Router::new().route(
        secret_route,
        get({
            let engine = Arc::clone(&engine);
            move |ws| handler(engine, ws)
        }),
    );

    axum::Server::bind(&opt.bind)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn handler(engine: Arc<Engine>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(engine, socket))
}

async fn handle_socket(engine: Arc<Engine>, mut socket: WebSocket) {
    if let Err(err) = handle_socket_inner(&engine, &mut socket).await {
        log::warn!("socket handler error: {}", err);
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn handle_socket_inner(engine: &Engine, socket: &mut WebSocket) -> io::Result<()> {
    let mut pipes: Option<MutexGuard<EnginePipes>> = None;

    loop {
        let event = if let Some(ref mut locked_pipes) = pipes {
            tokio::select! {
                engine_in = socket.recv() => Left(engine_in),
                engine_out = locked_pipes.read() => Right(engine_out),
                _ = engine.notify.notified() => continue,
            }
        } else {
            Left(socket.recv().await)
        };

        match event {
            Left(Some(Ok(Message::Text(text)))) => {
                let msg = UciIn::from_str(&text)?;

                let mut locked_pipes = match pipes.take() {
                    Some(locked_pipes) => locked_pipes,
                    None => {
                        engine.notify.notify_one();
                        let mut locked_pipes = engine.pipes.lock().await;
                        locked_pipes.idle().await?;
                        locked_pipes.write(UciIn::Uci).await?;
                        locked_pipes.idle().await?;
                        locked_pipes.write(UciIn::Ucinewgame).await?;
                        locked_pipes.write(UciIn::Isready).await?;
                        locked_pipes.idle().await?;
                        locked_pipes
                    }
                };

                locked_pipes.write(msg).await?;
                pipes = Some(locked_pipes);
            }
            Left(Some(Ok(Message::Pong(_)))) => (),
            Left(Some(Ok(Message::Ping(data)))) => socket
                .send(Message::Pong(data))
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?,
            Left(Some(Ok(Message::Binary(_)))) => {
                if let Some(ref mut locked_pipes) = pipes {
                    locked_pipes.idle().await?;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "binary messages not supported",
                ));
            }
            Left(None | Some(Ok(Message::Close(_)))) => {
                if let Some(ref mut locked_pipes) = pipes {
                    locked_pipes.idle().await?;
                }
                break Ok(());
            }
            Left(Some(Err(err))) => {
                if let Some(ref mut locked_pipes) = pipes {
                    locked_pipes.idle().await?;
                }
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, err));
            }

            Right(Ok(Some(UciOut::Unkown))) => (),
            Right(Ok(Some(msg))) => {
                socket
                    .send(Message::Text(msg.to_string()))
                    .await
                    .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;
            }
            Right(Ok(None)) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "engine stdout closed unexpectedly",
                ))
            }
            Right(Err(err)) => return Err(err),
        }
    }
}

enum UciIn {
    Uci,
    Isready,
    Setoption(String),
    Ucinewgame,
    Position(String),
    Go(String),
    Stop,
    Ponderhit,
}

impl FromStr for UciIn {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<UciIn, Self::Err> {
        let mut parts = s.split(' ');
        Ok(match parts.next().unwrap() {
            "uci" => UciIn::Uci,
            "isready" => UciIn::Isready,
            "ucinewgame" => UciIn::Ucinewgame,
            "ponderhit" => UciIn::Ponderhit,
            "stop" => UciIn::Stop,
            "setoption" => UciIn::Setoption(
                parts
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid setopion"))?
                    .to_owned(),
            ),
            "position" => UciIn::Position(
                parts
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid position"))?
                    .to_owned(),
            ),
            "go" => UciIn::Go(
                parts
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid go"))?
                    .to_owned(),
            ),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid uci input",
                ))
            }
        })
    }
}

impl fmt::Display for UciIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UciIn::Uci => f.write_str("uci"),
            UciIn::Isready => f.write_str("isready"),
            UciIn::Setoption(args) => write!(f, "setoption {}", args),
            UciIn::Ucinewgame => f.write_str("ucinewgame"),
            UciIn::Position(args) => write!(f, "position {}", args),
            UciIn::Go(args) => write!(f, "go {}", args),
            UciIn::Stop => f.write_str("stop"),
            UciIn::Ponderhit => f.write_str("ponderhit"),
        }
    }
}

enum UciOut {
    Id(String),
    Uciok,
    Readok,
    Bestmove(String),
    Info(String),
    Option(String),
    Unkown,
}

impl FromStr for UciOut {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<UciOut, Self::Err> {
        let mut parts = s.splitn(2, ' ');
        Ok(match parts.next().unwrap() {
            "id" => UciOut::Id(
                parts
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid id"))?
                    .to_owned(),
            ),
            "uciok" => UciOut::Uciok,
            "readyok" => UciOut::Readok,
            "bestmove" => UciOut::Bestmove(
                parts
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid bestmove"))?
                    .to_owned(),
            ),
            "info" => UciOut::Info(
                parts
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid info"))?
                    .to_owned(),
            ),
            "option" => UciOut::Option(
                parts
                    .next()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid option"))?
                    .to_owned(),
            ),
            _ => UciOut::Unkown,
        })
    }
}

impl fmt::Display for UciOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UciOut::Id(args) => write!(f, "id {}", args),
            UciOut::Uciok => f.write_str("uciok"),
            UciOut::Readok => f.write_str("readyok"),
            UciOut::Bestmove(args) => write!(f, "bestmove {}", args),
            UciOut::Info(args) => write!(f, "info {}", args),
            UciOut::Option(args) => write!(f, "option {}", args),
            UciOut::Unkown => Ok(()),
        }
    }
}
