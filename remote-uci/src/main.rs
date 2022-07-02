use clap::Parser;
use listenfd::ListenFd;
use remote_uci::{make_server, Opt};

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::new()
            .filter("REMOTE_UCI_LOG")
            .default_filter_or("debug")
            .write_style("REMOTE_UCI_LOG_STYLE"),
    )
    .format_target(false)
    .format_module_path(false)
    .init();

    let (spec, server) = make_server(Opt::parse(), ListenFd::from_env()).await;
    println!("{}", spec.registration_url());
    server.await.expect("bind");
}
