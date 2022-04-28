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
use rand::random;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines},
    process::{ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

#[derive(Debug, Parser)]
struct Opt {
    engine: PathBuf,
    #[clap(long, default_value = "127.0.0.1:9670")]
    bind: SocketAddr,
}

struct Engine {
    current_handler: AtomicU64,
    pipes: Mutex<EnginePipes>,
}

struct EnginePipes {
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
            current_handler: AtomicU64::new(0),
            pipes: Mutex::new(EnginePipes {
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

    let secret_route = Box::leak(format!("/{:032x}", random::<u128>()).into_boxed_str());
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

async fn handle_socket(engine: Arc<Engine>, socket: WebSocket) {
    if let Err(err) = handle_socket_inner(engine, socket).await {
        log::warn!("socket handler error: {}", err);
    }
}

async fn handle_socket_inner(
    engine: Arc<Engine>,
    mut socket: WebSocket,
) -> Result<(), axum::Error> {
    let current_handler = engine.current_handler.fetch_add(1, Ordering::SeqCst) + 1;
    let mut pipes = engine.pipes.lock().await;
    while current_handler == engine.current_handler.load(Ordering::SeqCst) {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(mut text))) => {
                        log::debug!("<< {}", text);
                        text.push_str("\n");
                        pipes.stdin.write_all(text.as_bytes()).await.map_err(|err| axum::Error::new(err))?;
                        pipes.stdin.flush().await.map_err(|err| axum::Error::new(err))?;
                    }
                    Some(Ok(Message::Pong(_))) => (),
                    Some(Ok(Message::Ping(data))) => socket.send(Message::Pong(data)).await?,
                    Some(Ok(Message::Binary(_))) => return Err(axum::Error::new(io::Error::new(io::ErrorKind::InvalidData, "accepting only text messages"))),
                    None | Some(Ok(Message::Close(_))) => break,
                    Some(Err(err)) => return Err(err),
                }
            }
            line = pipes.stdout.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        log::debug!(">> {}", line);
                        socket.send(Message::Text(line)).await?;
                    }
                    Ok(None) =>
                    return Err(axum::Error::new(io::Error::new(io::ErrorKind::UnexpectedEof, "engine stdout closed unexpectedly"))),
                    Err(err) => return Err(axum::Error::new(err)),
                }
            }
        }
    }
    socket.send(Message::Close(None)).await?;
    Ok(())
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
    type Err = ();

    fn from_str(s: &str) -> Result<UciIn, Self::Err> {
        let mut parts = s.split(' ');
        Ok(match parts.next().unwrap() {
            "uci" => UciIn::Uci,
            "isready" => UciIn::Isready,
            "ucinewgame" => UciIn::Ucinewgame,
            "ponderhit" => UciIn::Ponderhit,
            "stop" => UciIn::Stop,
            "setoption" => UciIn::Setoption(parts.next().ok_or_else(|| ())?.to_owned()),
            "position" => UciIn::Position(parts.next().ok_or_else(|| ())?.to_owned()),
            "go" => UciIn::Go(parts.next().ok_or_else(|| ())?.to_owned()),
            _ => return Err(()),
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
}

impl FromStr for UciOut {
    type Err = ();

    fn from_str(s: &str) -> Result<UciOut, Self::Err> {
        let mut parts = s.splitn(2, ' ');
        Ok(match parts.next().unwrap() {
            "id" => UciOut::Id(parts.next().ok_or_else(|| ())?.to_owned()),
            "uciok" => UciOut::Uciok,
            "readyok" => UciOut::Readok,
            "bestmove" => UciOut::Bestmove(parts.next().ok_or_else(|| ())?.to_owned()),
            "info" => UciOut::Info(parts.next().ok_or_else(|| ())?.to_owned()),
            "option" => UciOut::Option(parts.next().ok_or_else(|| ())?.to_owned()),
            _ => return Err(()),
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
        }
    }
}
