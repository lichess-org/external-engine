use std::future;

use clap::Parser;
use remote_uci::Opt;

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

    remote_uci::serve(Opt::parse(), future::pending()).await;
}
