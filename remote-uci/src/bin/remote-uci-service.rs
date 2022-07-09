use windows_service::{service_dispatcher, define_windows_service, service_control_handler::{self, ServiceControlHandlerResult}, service::ServiceControl};
use std::ffi::OsString;
use windows_service::service::{ServiceType, ServiceStatus, ServiceExitCode, ServiceControlAccept, ServiceState};
use std::time::Duration;

define_windows_service!(ffi_service_main, service_main);

fn main() -> Result<(), windows_service::Error> {
	service_dispatcher::start("remote_uci", ffi_service_main)?;
    Ok(())
}

fn service_main(args: Vec<OsString>) {
	let status_handle = service_control_handler::register("remote_uci", move |event| {
		match event {
			ServiceControl::Stop => ServiceControlHandlerResult::NoError,
			ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
			_ => ServiceControlHandlerResult::NotImplemented
		}
	}).expect("register service");
	
	status_handle.set_service_status(ServiceStatus {
		service_type: ServiceType::OWN_PROCESS,
		current_state: ServiceState::Running,
		controls_accepted: ServiceControlAccept::STOP,
		exit_code: ServiceExitCode::Win32(0),
		checkpoint: 0,
		wait_hint: Duration::default(),
		process_id: None,
	}).expect("set service status");
}