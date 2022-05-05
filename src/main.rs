mod engine;
mod ws;

use std::{fmt, fs, io, net::SocketAddr, ops::Not, path::PathBuf, sync::Arc, thread};

use axum::{routing::get, Router};
use clap::Parser;
use rand::random;
use serde::Serialize;
use serde_with::{serde_as, CommaSeparator, DisplayFromStr, StringWithSeparator};
use sysinfo::{RefreshKind, System, SystemExt};

use crate::{engine::Engine, ws::SharedEngine};

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
#[serde(rename_all = "camelCase")]
struct ExternalSpec {
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

struct Key(u128);

impl Key {
    fn load() -> Key {
        let dir = home::home_dir().expect("home dir");
        let path = dir.join(".remote-uci-key");
        match fs::read_to_string(&path) {
            Ok(secret) => Key(u128::from_str_radix(&secret, 16).expect("parse ~/.remote-uci-key")),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let secret = Key(random::<u128>());
                fs::write(path, secret.to_string()).expect("write ~/.remote-uci-key");
                secret
            }
            Err(err) => panic!("could not read ~/.remote-uci-key: {}", err),
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

fn available_memory() -> u64 {
    let sys = System::new_with_specifics(RefreshKind::new().with_memory());
    (sys.available_memory() / 1024).next_power_of_two() / 2
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::new()
            .filter("REMOTE_UCI_LOG")
            .default_filter_or("info")
            .write_style("REMOTE_UCI_LOG_STYLE"),
    )
    .format_target(false)
    .format_module_path(false)
    .init();

    let opt = Opt::parse();

    let engine = Engine::new(opt.engine).await.expect("spawn engine");

    //let mut locked_pipes = engine.pipes.lock().await;
    //drop(locked_pipes);

    let engine = Arc::new(SharedEngine::new(engine));

    let secret_route = Box::leak(format!("/{}", Key::load()).into_boxed_str());
    let spec = ExternalSpec {
        url: format!("ws://{}{}", opt.bind, secret_route),
        max_threads: thread::available_parallelism()
            .expect("available threads")
            .into(),
        max_hash: available_memory(),
        variants: Vec::new(),
        name: opt.name.unwrap_or_else(|| "remote-uci".to_owned()),
        official_stockfish: opt.promise_official_stockfish,
    };

    println!(
        "https://lichess.org/analysis/external?{}",
        serde_urlencoded::to_string(&spec).expect("serialize spec"),
    );

    let app = Router::new().route(
        secret_route,
        get({
            let engine = Arc::clone(&engine);
            move |socket| ws::handler(engine, socket)
        }),
    );

    axum::Server::bind(&opt.bind)
        .serve(app.into_make_service())
        .await
        .expect("bind");
}
