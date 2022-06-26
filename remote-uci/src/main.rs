#![feature(byte_slice_trim_ascii)]
#![feature(split_as_slice)]
#![feature(bool_to_option)]

mod engine;
mod uci;
mod ws;

use remote_uci::make_server;
use remote_uci::Opt;
use clap::Parser;

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

    let (spec, server) = make_server(Opt::parse()).await;
    println!("{}", spec.registration_url());
    server.await.expect("bind");
}
