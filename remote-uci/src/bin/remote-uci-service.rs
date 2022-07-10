use std::{ffi::OsString, sync::Arc, time::Duration};

use clap::Parser;
use listenfd::ListenFd;
use remote_uci::{make_server, Opts};
use tokio::sync::Notify;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

define_windows_service!(ffi_service_main, service_main);

fn main() -> Result<(), windows_service::Error> {
    service_dispatcher::start("remote_uci", ffi_service_main)?;
    Ok(())
}

fn service_status(state: ServiceState, wait_hint: Duration) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint,
        process_id: None,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn service_main(args: Vec<OsString>) {
    simple_logging::log_to_file("C:\\remote-uci.log", log::LevelFilter::Trace);
    std::panic::set_hook(Box::new(|panic| {
        log::error!("Panic: {:?}", panic);
    }));

    log::debug!("Args: {:?}", args);
    log::debug!("Std env args: {:?}", std::env::args());

    log::debug!("Registering service ...");

    let stop_rx = Arc::new(Notify::new());
    let stop_tx = Arc::clone(&stop_rx);

    let status_handle = service_control_handler::register("remote_uci", move |event| match event {
        ServiceControl::Stop => {
            stop_tx.notify_one();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .expect("register service");

    log::debug!("Start pending ...");

    status_handle
        .set_service_status(service_status(
            ServiceState::StartPending,
            Duration::from_secs(60),
        ))
        .expect("set start pending");

    log::debug!("Making server ...");

    let opts = match Opts::try_parse() {
        Ok(opts) => opts,
        Err(err) => {
            log::error!("error: {err}");
            panic!("invalid opts");
        }
    };

    let (_spec, server) = make_server(opts, ListenFd::empty()).await;

    log::debug!("Running server ...");

    server
        .with_graceful_shutdown(async {
            log::debug!("Set running ...");
            status_handle
                .set_service_status(service_status(ServiceState::Running, Duration::default()))
                .expect("set running");
            log::debug!("Waiting for shutdown event ...");
            stop_rx.notified().await;
            log::debug!("Stop pending ...");
            status_handle
                .set_service_status(service_status(
                    ServiceState::StopPending,
                    Duration::from_secs(60),
                ))
                .expect("set stop pending");
        })
        .await
        .expect("bind");

    log::debug!("About to stop ...");

    status_handle
        .set_service_status(service_status(ServiceState::Stopped, Duration::default()))
        .expect("set stopped");
}
