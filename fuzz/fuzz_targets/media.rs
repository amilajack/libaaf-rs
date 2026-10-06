#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = libaaf::media::audio_info(data);
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = libaaf::media::decode_uri(s);
    }
});
