use std::error::Error;

use clap::Parser;
use listenfd::ListenFd;
use remote_uci::{make_server, Opts};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(
        env_logger::Env::new()
            .filter("REMOTE_UCI_LOG")
            .default_filter_or("info")
            .write_style("REMOTE_UCI_LOG_STYLE"),
    )
    .format_target(false)
    .format_module_path(false)
    .init();

    let (spec, server) = make_server(Opts::parse(), ListenFd::from_env()).await?;
    println!("{}", spec.registration_url());
    server.with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Expect shutdown signal handler");
    println!("\nRecieved Sigterm, shutting down gracefully...");
}