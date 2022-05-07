use std::sync::Arc;

use clap::Parser;
use ksni::{
    menu::{MenuItem, StandardItem},
    Tray, TrayService,
};
use remote_uci::Opt;
use tokio::sync::Notify;

struct RemoteUciTray {
    shutdown: Arc<Notify>,
}

impl Tray for RemoteUciTray {
    fn icon_name(&self) -> String {
        "help-about".into()
    }

    fn title(&self) -> String {
        "remote-uci-applet".into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![StandardItem {
            label: "Exit".into(),
            icon_name: "application-exit".into(),
            activate: Box::new(|tray: &mut RemoteUciTray| tray.shutdown.notify_one()),
            ..Default::default()
        }
        .into()]
    }
}

#[tokio::main]
async fn main() {
    let opt = Opt::parse();
    let shutdown = Arc::new(Notify::new());
    TrayService::new(RemoteUciTray {
        shutdown: Arc::clone(&shutdown),
    })
    .spawn();
    remote_uci::serve(opt, shutdown.notified()).await;
}
