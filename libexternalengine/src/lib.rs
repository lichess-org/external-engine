#[repr(C)]
struct EngineConfig {
    auth_token: *const u8,
    engine_path: *const u8,
    max_hash: i32,
    max_threads: i32,
}

#[no_mangle]
fn StartListening(config: *const EngineConfig) -> i32 {
    1
}

#[no_mangle]
fn GetStatus() -> i32 {
    2
}

#[no_mangle]
fn StopListening() -> i32 {
    3
}
