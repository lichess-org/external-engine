use windows_service::{service_dispatcher, define_windows_service, service_control_handler::{self, ServiceControlHandlerResult}, service::ServiceControl};
use std::ffi::OsString;
use windows_service::service::{ServiceType, ServiceStatus, ServiceExitCode, ServiceControlAccept, ServiceState};
use std::time::Duration;
use remote_uci::{make_server, Opts};
use clap::Parser;
use listenfd::ListenFd;
use tokio::sync::Notify;
use std::sync::Arc;

define_windows_service!(ffi_service_main, service_main);

fn main() -> Result<(), windows_service::Error> {
	service_dispatcher::start("remote_uci", ffi_service_main)?;
    Ok(())
}

fn service_status(state: ServiceState) -> ServiceStatus {
	ServiceStatus {
		service_type: ServiceType::OWN_PROCESS,
		current_state: state,
		controls_accepted: ServiceControlAccept::STOP,
		exit_code: ServiceExitCode::Win32(0),
		checkpoint: 0,
		wait_hint: Duration::default(),
		process_id: None,
	}
}

#[tokio::main]
async fn service_main(_args: Vec<OsString>) {
	let stop_rx = Arc::new(Notify::new());
	let stop_tx = Arc::clone(&stop_rx);

	let status_handle = service_control_handler::register("remote_uci", move |event| {
		match event {
			ServiceControl::Stop => {
				stop_tx.notify_one();
				ServiceControlHandlerResult::NoError
			}
			ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
			_ => ServiceControlHandlerResult::NotImplemented
		}
	}).expect("register service");
	
	status_handle.set_service_status(service_status(ServiceState::Running)).expect("set running");
	
	let (_spec, server) = make_server(Opts::parse(), ListenFd::empty()).await;
	
	server.with_graceful_shutdown(async {
		stop_rx.notified().await;
		status_handle.set_service_status(service_status(ServiceState::StopPending)).expect("set stop pending");
	}).await.expect("bind");

	status_handle.set_service_status(service_status(ServiceState::Stopped)).expect("set stopped");
}