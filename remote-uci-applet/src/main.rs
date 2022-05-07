use std::{
    process::{Command, Stdio},
    sync::Arc,
};

use clap::Parser;
use ksni::{
    menu::{Disposition, MenuItem, StandardItem},
    Tray, TrayService,
};
use remote_uci::{ExternalWorkerOpts, Opt};
use tokio::sync::Notify;

struct RemoteUciTray {
    shutdown: Arc<Notify>,
    spec: ExternalWorkerOpts,
}

impl Tray for RemoteUciTray {
    fn icon_name(&self) -> String {
        "help-about".into()
    }

    fn title(&self) -> String {
        "remote-uci-applet".into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Connect".into(),
                activate: Box::new(|tray: &mut RemoteUciTray| {
                    match Command::new("xdg-open")
                        .arg(tray.spec.registration_url())
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(_) => log::info!("opened: {}", tray.spec.registration_url()),
                        Err(err) => log::error!("{}", err),
                    }
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Copy connection URL".into(),
                // icon_name: "edit-copy".into(),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "License".into(),
                disposition: Disposition::Informative,
                // icon_name: "help-about".into(),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Shutdown".into(),
                // icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut RemoteUciTray| tray.shutdown.notify_one()),
                ..Default::default()
            }
            .into(),
        ]
    }
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

    let (spec, server) = remote_uci::make_server(opt).await;
    log::info!("registration url: {}", spec.registration_url());

    let shutdown = Arc::new(Notify::new());
    TrayService::new(RemoteUciTray {
        shutdown: Arc::clone(&shutdown),
        spec,
    })
    .spawn();

    server
        .with_graceful_shutdown(shutdown.notified())
        .await
        .expect("bind");
}
