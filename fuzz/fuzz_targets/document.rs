#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| if let Ok(d) = libaaf::Document::from_bytes(
    data.to_vec(),
    libaaf::ReaderOptions {
        recover_protools_fades: true,
        remove_sample_accurate_edits: true,
        ..Default::default()
    }
) {
    if let Ok(c) = d.composition() {
        let options = libaaf::inspection::DisplayOptions::default();
        let _ = libaaf::inspection::summary(&d, &c, &options);
        let _ = libaaf::inspection::clips(&c, &options);
        let _ = libaaf::inspection::essences(&d, &c, &options);
    }
    for o in d.objects() {
        for p in o.properties() {
            let _ = o.raw(p.id);
        }
    }
});
