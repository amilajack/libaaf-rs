use libaaf::{Document, ReaderOptions};
use std::path::PathBuf;
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tooling/reference/test/aaf")
}
#[test]
fn decode_complete_fixture_suite() {
    let mut count = 0;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "aaf") {
            let d = Document::open(&path, ReaderOptions::default())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(!d.objects.is_empty());
            let c = d
                .composition()
                .unwrap_or_else(|e| panic!("{} timeline: {e}", path.display()));
            assert!(c.name.is_some());
            count += 1;
        }
    }
    assert_eq!(count, 65);
}
