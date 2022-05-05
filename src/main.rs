mod engine;
mod ws;

use std::{error::Error, net::SocketAddr, ops::Not, path::PathBuf, sync::Arc, thread};

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
            move |socket| ws::handler(engine, socket)
        }),
    );

    axum::Server::bind(&opt.bind)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
