#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| if let Ok(d) =
    libaaf::Document::from_bytes(data.to_vec(), Default::default())
{
    let _ = d.composition();
    for o in &d.objects {
        for p in &o.properties {
            let _ = o.raw(p.id);
        }
    }
});
