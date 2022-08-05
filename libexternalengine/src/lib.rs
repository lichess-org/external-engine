#[repr(C)]
struct EngineConfig {
    auth_token: *const i8,
    engine_path: *const i8,
    max_hash: i32,
    max_threads: i32,
}

#[no_mangle]
fn StartListening(config: *const EngineConfig) -> i32 {
    let s = unsafe {std::ffi::CStr::from_ptr((*config).auth_token) };
    s.to_str().expect("utf-8").parse().expect("int")
}

#[no_mangle]
fn GetStatus() -> i32 {
    2
}

#[no_mangle]
fn StopListening() -> i32 {
    3
}
