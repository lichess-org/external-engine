use clap::Parser;
use remote_uci::Opt;
use ksni::{Tray, TrayService, menu::{MenuItem, StandardItem}};

struct RemoteUciTray;

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
                label: "Exit".into(),
                icon_name: "application-exit".into(),
                ..Default::default()
            }.into()
        ]
    }
}

#[tokio::main]
async fn main() {
    let opt = Opt::parse();
    TrayService::new(RemoteUciTray).spawn();
    remote_uci::serve(opt).await;
}
