use std::{
    error::Error,
    io,
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
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
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    process::{ChildStdin, ChildStdout, Command},
    sync::{Mutex, MutexGuard, Notify},
};
use sysinfo::{System, SystemExt, RefreshKind};

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
    searching: bool,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
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
                pending_readyok: 0,
                searching: false,
                stdin: BufWriter::new(process.stdin.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdin closed")
                })?),
                stdout: BufReader::new(process.stdout.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdout closed")
                })?),
            }),
        })
    }
}

impl EnginePipes {
    async fn write(&mut self, line: &[u8]) -> io::Result<()> {
        if line.contains(&b'\n') {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "disallowed line feed"));
        }

        match ClientCommand::classify(line) {
            Some(ClientCommand::Uci) => self.pending_uciok += 1,
            Some(ClientCommand::Isready) => self.pending_readyok += 1,
            Some(ClientCommand::Go) => {
                if self.searching {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "already searching"));
                }
                self.searching = true;
            }
            None => (),
        }

        log::info!("<< {}", String::from_utf8_lossy(line));
        self.stdin.write_all(line).await?;
        self.stdin.write_all(b"\r\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read(&mut self) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        self.stdout.read_until(b'\n', &mut line).await?;
        if line.ends_with(b"\n") {
            line.pop();
        }
        if line.ends_with(b"\r") {
            line.pop();
        }
        log::debug!(">> {}", String::from_utf8_lossy(&line));

        match EngineCommand::classify(&line) {
            Some(EngineCommand::Uciok) => self.pending_uciok = self.pending_uciok.saturating_sub(1),
            Some(EngineCommand::Readyok) => self.pending_readyok = self.pending_readyok.saturating_sub(1),
            Some(EngineCommand::Bestmove) => self.searching = false,
            None => (),
        }
        Ok(line)
    }

    fn is_idle(&self) -> bool {
        self.pending_uciok == 0 && self.pending_readyok == 0 && !self.searching
    }

    async fn ensure_idle(&mut self) -> io::Result<()> {
        while !self.is_idle() {
            if self.searching && self.pending_readyok < 1 {
                self.write(b"stop").await?;
                self.write(b"isready").await?;
            }
            self.read().await?;
        }
        Ok(())
    }

    async fn ensure_newgame(&mut self) -> io::Result<()> {
        self.ensure_idle().await?;
        self.write(b"ucinewgame").await?;
        self.write(b"isready").await?;
        self.ensure_idle().await?;
        Ok(())
    }
}

#[derive(Debug)]
struct RemoteSpec {
    url: String,
    threads: usize,
    hash: u64,
    variants: Vec<()>,
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

    let engine = Engine::new(opt.engine).await?;

    //let mut locked_pipes = engine.pipes.lock().await;
    //drop(locked_pipes);

    let engine = Arc::new(engine);

    let secret_route = Box::leak(format!("/{:032x}", random::<u128>() & 0).into_boxed_str());
    let spec = RemoteSpec {
        url: format!("ws://{}{}", opt.bind, secret_route),
        threads: thread::available_parallelism()?.into(),
        hash: {
            let sys = System::new_with_specifics(RefreshKind::new().with_memory());
            (sys.available_memory() / 1024).next_power_of_two() / 2
        },
        variants: Vec::new(),
    };

    for prefix in ["https://lichess.org", "https://lichess.dev", "http://localhost:9663", "http://l.org"] {
        println!("{}/analysis/external?url={}&maxThreads={}&maxHash={}&name={}", prefix, spec.url, spec.threads, spec.hash, "remote-uci");
    }

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
        log::error!("socket handler error: {}", err);
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn handle_socket_inner(engine: &Engine, socket: &mut WebSocket) -> io::Result<()> {
    let mut pipes: Option<MutexGuard<EnginePipes>> = None;
    let mut session = 0;

    loop {
        if let Some(mut locked_pipes) = pipes.take() {
            if session != engine.session.load(Ordering::SeqCst) {
                log::warn!("ending session {} ...", session);
                if locked_pipes.searching {
                    locked_pipes.write(b"stop").await?;
                }
                if locked_pipes.is_idle() {
                    log::warn!("session {} ended", session);
                } else {
                    pipes = Some(locked_pipes);
                }
            } else {
                pipes = Some(locked_pipes);
            }
        }

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
                let mut locked_pipes = match pipes.take() {
                    Some(locked_pipes) => locked_pipes,
                    None => {
                        session = engine.session.fetch_add(1, Ordering::SeqCst) + 1;
                        log::warn!("starting or restarting session {} ...", session);
                        engine.notify.notify_one();
                        let mut locked_pipes = engine.pipes.lock().await;
                        log::warn!("new session {} started", session);
                        locked_pipes.ensure_newgame().await?;
                        locked_pipes
                    }
                };

                locked_pipes.write(text.as_bytes()).await?;
                pipes = Some(locked_pipes);
            }
            Left(Some(Ok(Message::Pong(_)))) => (),
            Left(Some(Ok(Message::Ping(data)))) => socket
                .send(Message::Pong(data))
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?,
            Left(Some(Ok(Message::Binary(_)))) => {
                if let Some(ref mut locked_pipes) = pipes {
                    locked_pipes.ensure_idle().await?;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "binary messages not supported",
                ));
            }
            Left(None | Some(Ok(Message::Close(_)))) => {
                if let Some(ref mut locked_pipes) = pipes {
                    locked_pipes.ensure_idle().await?;
                }
                break Ok(());
            }
            Left(Some(Err(err))) => {
                if let Some(ref mut locked_pipes) = pipes {
                    locked_pipes.ensure_idle().await?;
                }
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, err));
            }

            Right(Ok(msg)) => {
                socket
                    .send(Message::Text(String::from_utf8(msg).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?))
                    .await
                    .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;
            }
            Right(Err(err)) => return Err(err),
        }
    }
}

enum ClientCommand {
    Uci,
    Isready,
    Go,
}

impl ClientCommand {
    fn classify(line: &[u8]) -> Option<ClientCommand> {
        Some(match line.split(|ch| *ch == b' ').next().unwrap() {
            b"uci" => ClientCommand::Uci,
            b"isready" => ClientCommand::Isready,
            b"go" => ClientCommand::Go,
            _ => return None,
        })
    }
}

enum EngineCommand {
    Uciok,
    Readyok,
    Bestmove,
}

impl EngineCommand {
    fn classify(line: &[u8]) -> Option<EngineCommand> {
        Some(match line.split(|ch| *ch == b' ').next().unwrap() {
            b"uciok" => EngineCommand::Uciok,
            b"readyok" => EngineCommand::Readyok,
            b"bestmove" => EngineCommand::Bestmove,
            _ => return None,
        })
    }
}
