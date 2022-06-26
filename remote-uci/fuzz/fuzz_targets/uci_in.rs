#![no_main]

use libfuzzer_sys::fuzz_target;
use remote_uci::uci::Parser;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    if let Ok(mut parser) = Parser::new(&s) {
        let _ = parser.parse_in();
    }
});
