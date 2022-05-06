use std::{
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use tokio::{
    sync::{Mutex, MutexGuard, Notify},
    time::{interval, MissedTickBehavior},
};

use crate::engine::{ClientCommand, Engine, Session};

pub struct SharedEngine {
    session: AtomicU64,
    notify: Notify,
    engine: Mutex<Engine>,
}

impl SharedEngine {
    pub fn new(engine: Engine) -> SharedEngine {
        SharedEngine {
            session: AtomicU64::new(0),
            notify: Notify::new(),
            engine: Mutex::new(engine),
        }
    }
}

pub async fn handler(engine: Arc<SharedEngine>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(engine, socket))
}

async fn handle_socket(shared_engine: Arc<SharedEngine>, mut socket: WebSocket) {
    if let Err(err) = handle_socket_inner(&shared_engine, &mut socket).await {
        log::error!("handler: {}", err);
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
    let mut session = Session(0);

    let mut missed_pong = false;
    let mut timeout = interval(Duration::from_secs(10));
    timeout.set_missed_tick_behavior(MissedTickBehavior::Delay);
    timeout.reset();

    loop {
        // Try to end session if another session wants to take over.
        // We send a stop command, and keep the previous session the engine
        // is actually idle.
        if let Some(mut engine) = locked_engine.take() {
            if session != Session(shared_engine.session.load(Ordering::SeqCst)) {
                log::warn!("{}: trying to end session ...", session.0);
                if engine.is_searching() {
                    engine.send(session, b"stop").await?;
                }
                if engine.is_idle() {
                    log::warn!("{}: session ended", session.0);
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
                engine_out = engine.recv(session) => Event::Engine(engine_out),
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
                    log::error!("{}: ping timeout", session.0);
                    if let Some(ref mut engine) = locked_engine {
                        engine.ensure_idle(session).await?;
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
                        session = Session(shared_engine.session.fetch_add(1, Ordering::SeqCst) + 1);
                        log::warn!("{}: starting or restarting session ...", session.0);
                        shared_engine.notify.notify_one();
                        let mut engine = shared_engine.engine.lock().await;
                        log::warn!("{}: new session started", session.0);
                        engine.ensure_newgame(session).await?;
                        engine
                    }
                };

                engine.send(session, text.as_bytes()).await?;
                locked_engine = Some(engine);
            }
            Event::Socket(Some(Ok(Message::Pong(_)))) => missed_pong = false,
            Event::Socket(Some(Ok(Message::Ping(data)))) => socket
                .send(Message::Pong(data))
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?,
            Event::Socket(Some(Ok(Message::Binary(_)))) => {
                if let Some(ref mut engine) = locked_engine {
                    engine.ensure_idle(session).await?;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "binary messages not supported",
                ));
            }
            Event::Socket(None | Some(Ok(Message::Close(_)))) => {
                if let Some(ref mut engine) = locked_engine {
                    engine.ensure_idle(session).await?;
                }
                break Ok(());
            }
            Event::Socket(Some(Err(err))) => {
                if let Some(ref mut engine) = locked_engine {
                    engine.ensure_idle(session).await?;
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
