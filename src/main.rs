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
};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines},
    process::{ChildStdin, ChildStdout, Command},
    sync::{mpsc, Mutex},
};

#[derive(Debug, Parser)]
struct Opt {
    engine: PathBuf,
    #[clap(long, default_value = "127.0.0.1:9670")]
    bind: SocketAddr,
}

fn secret() -> String {
    format!("{:032x}", rand::random::<u128>())
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
    let opt = Opt::parse();

    let engine = Arc::new(Engine::new(opt.engine).await?);

    let app = Router::new().route(
        "/:secret",
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
    handle_socket_inner(engine, socket).await;
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
                    Some(Ok(Message::Text(text))) => pipes.stdin.write_all(text.as_bytes()).await.map_err(|err| axum::Error::new(err))?,
                    Some(Ok(Message::Pong(_))) => (),
                    Some(Ok(Message::Ping(data))) => socket.send(Message::Pong(data)).await?,
                    Some(Ok(Message::Binary(binary))) => pipes.stdin.write_all(&binary).await.map_err(|err| axum::Error::new(err))?,
                    None | Some(Ok(Message::Close(_))) => break,
                    Some(Err(err)) => return Err(err),
                }
            }
            line = pipes.stdout.next_line() => {
                match line {
                    Ok(Some(line)) => socket.send(Message::Text(line)).await?,
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
