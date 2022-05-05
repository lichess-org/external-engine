mod engine;

use std::{
    error::Error,
    io,
    net::SocketAddr,
    ops::Not,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use engine::{ClientCommand, Engine};
use rand::random;
use serde::Serialize;
use serde_with::{serde_as, CommaSeparator, DisplayFromStr, StringWithSeparator};
use sysinfo::{RefreshKind, System, SystemExt};
use tokio::{
    sync::{Mutex, MutexGuard, Notify},
    time::{interval, MissedTickBehavior},
};

#[derive(Debug, Parser)]
struct Opt {
    engine: PathBuf,
    #[clap(long, default_value = "127.0.0.1:9670")]
    bind: SocketAddr,
    #[clap(long)]
    name: Option<String>,
    #[clap(long, hide = true)]
    promise_official_stockfish: bool,
}

#[serde_as]
#[derive(Debug, Serialize)]
struct RemoteSpec {
    url: String,
    name: String,
    max_threads: usize,
    max_hash: u64,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variants: Vec<String>,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(skip_serializing_if = "Not::not")]
    official_stockfish: bool,
}

struct SharedEngine {
    session: AtomicU64,
    notify: Notify,
    engine: Mutex<Engine>,
}

impl SharedEngine {
    fn new(engine: Engine) -> SharedEngine {
        SharedEngine {
            session: AtomicU64::new(0),
            notify: Notify::new(),
            engine: Mutex::new(engine),
        }
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

    let engine = Engine::new(opt.engine).await?;

    //let mut locked_pipes = engine.pipes.lock().await;
    //drop(locked_pipes);

    let engine = Arc::new(SharedEngine::new(engine));

    let secret_route = Box::leak(format!("/{:032x}", random::<u128>() & 0).into_boxed_str());
    let spec = RemoteSpec {
        url: format!("ws://{}{}", opt.bind, secret_route),
        max_threads: thread::available_parallelism()?.into(),
        max_hash: {
            let sys = System::new_with_specifics(RefreshKind::new().with_memory());
            (sys.available_memory() / 1024).next_power_of_two() / 2
        },
        variants: Vec::new(),
        name: opt.name.unwrap_or_else(|| "remote-uci".to_owned()),
        official_stockfish: opt.promise_official_stockfish,
    };

    for prefix in [
        "https://lichess.org",
        "https://lichess.dev",
        "http://localhost:9663",
        "http://l.org",
    ] {
        println!(
            "{}/analysis/external?{}",
            prefix,
            serde_urlencoded::to_string(&spec).expect("serialize spec"),
        );
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

async fn handler(engine: Arc<SharedEngine>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(engine, socket))
}

async fn handle_socket(shared_engine: Arc<SharedEngine>, mut socket: WebSocket) {
    if let Err(err) = handle_socket_inner(&shared_engine, &mut socket).await {
        log::error!("socket handler error: {}", err);
    }
    let _ = socket.send(Message::Close(None)).await;
}

enum Event {
    Socket(Option<Result<Message, axum::Error>>),
    Engine(io::Result<Vec<u8>>),
    CheckSession,
    Tick,
}

async fn handle_socket_inner(
    shared_engine: &SharedEngine,
    socket: &mut WebSocket,
) -> io::Result<()> {
    let mut locked_engine: Option<MutexGuard<Engine>> = None;
    let mut session = 0;

    let mut missed_pong = false;
    let mut timeout = interval(Duration::from_secs(10));
    timeout.set_missed_tick_behavior(MissedTickBehavior::Delay);
    timeout.reset();

    loop {
        // Try to end session if another session wants to take over.
        // We send a stop command, and keep the previous session the engine
        // is actually idle.
        if let Some(mut engine) = locked_engine.take() {
            if session != shared_engine.session.load(Ordering::SeqCst) {
                log::warn!("trying to end session {} ...", session);
                if engine.is_searching() {
                    engine.send(b"stop").await?;
                }
                if engine.is_idle() {
                    log::warn!("session {} ended", session);
                } else {
                    locked_engine = Some(engine);
                }
            } else {
                locked_engine = Some(engine);
            }
        }

        // Select next event to handle.
        let event = if let Some(ref mut engine) = locked_engine {
            tokio::select! {
                engine_in = socket.recv() => Event::Socket(engine_in),
                engine_out = engine.recv() => Event::Engine(engine_out),
                _ = shared_engine.notify.notified() => Event::CheckSession,
                _ = timeout.tick() => Event::Tick,
            }
        } else {
            tokio::select! {
                engine_in = socket.recv() => Event::Socket(engine_in),
                _ = timeout.tick() => Event::Tick,
            }
        };

        // Handle event.
        match event {
            Event::CheckSession => continue,

            Event::Tick => {
                if missed_pong {
                    log::error!("ping timeout in session {}", session);
                    if let Some(ref mut engine) = locked_engine {
                        engine.ensure_idle().await?;
                    }
                    break Ok(());
                } else {
                    socket
                        .send(Message::Ping(Vec::new()))
                        .await
                        .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;
                    missed_pong = true;
                }
            }

            Event::Socket(Some(Ok(Message::Text(text)))) => {
                let mut engine = match locked_engine.take() {
                    Some(engine) => engine,
                    None if ClientCommand::classify(text.as_bytes())
                        == Some(ClientCommand::Stop) =>
                    {
                        // No need to make a new session just to send a stop
                        // command.
                        continue;
                    }
                    None => {
                        session = shared_engine.session.fetch_add(1, Ordering::SeqCst) + 1;
                        log::warn!("starting or restarting session {} ...", session);
                        shared_engine.notify.notify_one();
                        let mut engine = shared_engine.engine.lock().await;
                        log::warn!("new session {} started", session);
                        engine.ensure_newgame().await?;
                        engine
                    }
                };

                engine.send(text.as_bytes()).await?;
                locked_engine = Some(engine);
            }
            Event::Socket(Some(Ok(Message::Pong(_)))) => missed_pong = false,
            Event::Socket(Some(Ok(Message::Ping(data)))) => socket
                .send(Message::Pong(data))
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?,
            Event::Socket(Some(Ok(Message::Binary(_)))) => {
                if let Some(ref mut engine) = locked_engine {
                    engine.ensure_idle().await?;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "binary messages not supported",
                ));
            }
            Event::Socket(None | Some(Ok(Message::Close(_)))) => {
                if let Some(ref mut engine) = locked_engine {
                    engine.ensure_idle().await?;
                }
                break Ok(());
            }
            Event::Socket(Some(Err(err))) => {
                if let Some(ref mut engine) = locked_engine {
                    engine.ensure_idle().await?;
                }
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, err));
            }

            Event::Engine(Ok(msg)) => {
                socket
                    .send(Message::Text(String::from_utf8(msg).map_err(|err| {
                        io::Error::new(io::ErrorKind::InvalidData, err)
                    })?))
                    .await
                    .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;
            }
            Event::Engine(Err(err)) => return Err(err),
        }
    }
}
