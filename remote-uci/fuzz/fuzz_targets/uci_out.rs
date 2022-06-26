#![no_main]

use libfuzzer_sys::fuzz_target;
use remote_uci::uci::UciOut;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    if let Ok(Some(uci_out)) = UciOut::from_line(&s) {
        let uci_out_rountripped = UciOut::from_line(&uci_out.to_string()).unwrap().unwrap();
        assert_eq!(uci_out, uci_out_rountripped);
    }
});
