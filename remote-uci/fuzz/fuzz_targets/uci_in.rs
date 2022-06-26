#![no_main]

use libfuzzer_sys::fuzz_target;
use remote_uci::uci::UciIn;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    if let Ok(Some(uci_in)) = UciIn::from_line(&s) {
        let uci_in_rountripped = UciIn::from_line(&uci_in.to_string()).unwrap().unwrap();
        assert_eq!(uci_in, uci_in_rountripped);
    }
});
